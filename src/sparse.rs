use core::hash::{BuildHasher, Hash};
extern crate alloc;
use crate::buf::Buf;
use crate::error::Error;
use crate::vint::VarInt;
use crate::DefaultHasher;
use crate::HyperLogLog;
use alloc::vec::Vec;

/// We compact the u64 hash into an encoded u32 for storage in the diff vec.
/// When converting to dense representation, encoded hashes are decoded.
/// The trailing 0s and register index of the original u64 hash are recovered from the decoded value.
/// The encoding scheme reserves 7 bits for trailing zeros: 6 bits for the max value, 64, and an extra bit to
/// indicate either the value of the trailing zeros is used or part of the hash itself with trailing zeros equal to the
/// full hash itself. Part of the hash is used instead of the trailing zero count to reduce collisions of encoded values, i.e.
/// two values are more likely to share the trailing zero count than being equal.
/// The remaining 25 bits are to encoded the register, and additional entropy from the hash.
const MAX_PRECISION: u8 = 25; // u32::BITS - u64::MAX.trailing_ones() - 1;

#[inline]
fn encode_hash(hash: u64) -> u32 {
    let index = hash >> (64 - MAX_PRECISION);
    let prefix = (index << 7) as u32;
    let tz = hash.trailing_zeros();
    match tz {
        0..=5 => prefix | ((hash & 0b111111) as u32) << 1,
        _ => prefix | (tz << 1) | 1,
    }
}

#[inline]
fn decode_hash(hash: u32, precision: u8) -> (u32, usize) {
    let index = (hash >> (7 + MAX_PRECISION - precision)) as usize;
    match hash & 1 {
        0 => (hash.trailing_zeros(), index),
        _ => (1 + ((hash & 0b111111) >> 1), index),
    }
}

#[inline]
fn correction(num: usize) -> f64 {
    let num = num as f64;
    // Assume most trailing zeros are encoded as the trailing part of the hash, so
    // 5 extra bits of entropy.
    let buckets = (1 << (MAX_PRECISION + 5)) as f64;
    let zeros = buckets - num;
    buckets as f64 * crate::math::ln(buckets as f64 / zeros as f64)
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct DiffVec {
    encoded: Buf,
    last: u32,
    len: u32,
}

impl DiffVec {
    pub fn with_size(size: usize, max_size: usize) -> Self {
        Self {
            encoded: Buf::new(size, max_size),
            last: 0,
            len: 0,
        }
    }

    #[inline]
    pub fn size(&self) -> usize {
        self.encoded.len()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn push(&mut self, val: u32) {
        if val == self.last {
            return;
        }
        let diff = val - self.last;
        self.last = val;
        self.len += 1;
        VarInt::write(&mut self.encoded, diff);
    }
}

impl<'a> IntoIterator for &'a DiffVec {
    type Item = u32;
    type IntoIter = DiffIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        DiffIter {
            index: 0,
            last: 0,
            inner: &self,
        }
    }
}

pub struct DiffIter<'a> {
    index: usize,
    last: u32,
    inner: &'a DiffVec,
}

impl<'a> Iterator for DiffIter<'a> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.inner.size() {
            None
        } else {
            let (dif, index) = VarInt::read(&self.inner.encoded, self.index);
            self.index = index;
            self.last += dif;
            Some(self.last)
        }
    }
}

impl ExactSizeIterator for DiffIter<'_> {
    fn len(&self) -> usize {
        self.inner.size() - self.index
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct SparseLogLog {
    /// Small temporary collection of the latset encoded hashes (u32).
    /// It's batch merged into data when it fills up enough.
    new: Vec<u32>,
    indexes: DiffVec,
    precision: u8,
}

impl SparseLogLog {
    /// Fraction (1 / X) of dense memory (1 << precision bytes) before flushing `new` into sparse.
    /// I.e. we flush X many times before converting to dense representation.
    /// Just before the flush, we theoretically use dense_mem / X + sparse_mem.
    /// There's a trade-off in speed vs memory: the larger X is, the less memory we use but slower due to more frequent flushing.
    /// Note we flush anytime count is called regardless of X.
    const NEW_SIZE_FACTOR: usize = 25;

    pub fn new(precision: u8) -> Self {
        Self {
            new: Vec::new(),
            indexes: Default::default(),
            precision,
        }
    }

    #[inline]
    fn hll_size_bytes(precision: u8) -> usize {
        (1 << precision) as usize
    }

    #[inline]
    fn should_flush(len: usize, precision: u8) -> bool {
        let dense_hll_size = Self::hll_size_bytes(precision) << 2;
        len * Self::NEW_SIZE_FACTOR > dense_hll_size
    }

    #[inline]
    pub fn insert_hash(&mut self, hash: u64) {
        let encoded = encode_hash(hash);
        if self.new.len() == self.new.capacity() {
            let dense_hll_size = ((1 << self.precision) as usize) << 2;
            let max_len =
                crate::math::ceil(dense_hll_size as f64 / Self::NEW_SIZE_FACTOR as f64) as usize;
            let new_cap = core::cmp::min(1 + (3 * self.new.len()) >> 1, max_len);
            self.new.reserve_exact(new_cap - self.new.len());
        }
        self.new.push(encoded);
        if Self::should_flush(self.new.len(), self.precision) {
            self.flush()
        }
    }

    #[inline]
    pub(crate) fn flush_inner(&mut self, mut other: impl ExactSizeIterator<Item = u32>) {
        // TODO: empirically derive the size from the precision
        let size = self.indexes.size() + (other.len() * 3);
        let max_size = self.indexes.size() + (other.len() * 5) + 8;
        let mut buf = DiffVec::with_size(size, max_size);
        let binding = core::mem::take(&mut self.indexes);
        let mut this = binding.into_iter();
        let (mut new_hash, mut old_hash) = (other.next(), this.next());
        while new_hash.is_some() && old_hash.is_some() {
            let new_hash_ = new_hash.unwrap();
            let old_hash_ = old_hash.unwrap();
            if new_hash_ == old_hash_ {
                buf.push(new_hash_);
                new_hash = other.next();
                old_hash = this.next();
            } else if new_hash_ > old_hash_ {
                buf.push(old_hash_);
                old_hash = this.next();
            } else {
                buf.push(new_hash_);
                new_hash = other.next();
            }
        }
        while let Some(h) = new_hash {
            buf.push(h);
            new_hash = other.next();
        }
        while let Some(h) = old_hash {
            buf.push(h);
            old_hash = this.next();
        }
        self.indexes = buf;
        self.indexes.encoded.shrink_to_fit();
    }

    #[inline]
    pub(crate) fn flush(&mut self) {
        if self.new.is_empty() {
            return;
        }
        self.new.sort_unstable();
        let new = core::mem::take(&mut self.new);
        self.flush_inner(new.into_iter());
    }

    #[inline]
    pub(crate) fn union(&mut self, other: &Self) -> Result<(), Error> {
        if self.precision != other.precision {
            return Err(Error::IncompatibleLength);
        }
        let mut other_new = other.new.clone();
        other_new.sort_unstable();
        self.flush_inner(other_new.into_iter());
        self.flush_inner(other.indexes.into_iter());
        Ok(())
    }

    #[inline]
    pub fn count(&mut self) -> f64 {
        self.flush();
        correction(self.indexes.len())
    }

    #[inline]
    pub fn full(&self) -> bool {
        self.indexes.size() > Self::hll_size_bytes(self.precision)
    }
}

impl From<SparseLogLog> for HyperLogLog {
    fn from(mut sparse: SparseLogLog) -> Self {
        sparse.flush();
        let registers = sparse.indexes.into_iter();
        let mut hll = HyperLogLog::new(sparse.precision);
        for encoded in registers {
            let (rank, register) = decode_hash(encoded, sparse.precision);
            hll.update::<true>(rank as u8, register);
        }
        hll
    }
}

/// An implementation of the the [HyperLogLog++](https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/40671.pdf) data structure.
///
/// For small cardinalities, a "sparse" representation is used. The sparse representation is more accurate and uses less memory,
/// but has slower insert speed.
/// The error and memory usage of the sparse representation scales roughly linearly with the number of items inserted. When
/// the memory of the sparse representation equals the memory of the dense representation, it switches to dense automatically.
/// This happens inside the `insert`/`insert_hash` call (which is why it needs `&mut self`). The error of the sparse representation
/// never exceeds that of the dense.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HyperLogLogPlus<S = DefaultHasher> {
    sparse: Option<SparseLogLog>,
    dense: Option<HyperLogLog>,
    hasher: S,
}

impl HyperLogLogPlus {
    /// Returns a new [`Self`] using the default hasher with a random seed.
    /// [`Self`] is initialized to use the compact and dynamically sized sparse representation,
    /// but later switches to the dense representation when it uses equal memory
    /// (`1 << precision` registers (1 byte each)).
    pub fn new(precision: u8) -> Self {
        Self::with_hasher(precision, DefaultHasher::default())
    }

    /// Returns a new [`Self`] using the default hasher seeded with `seed`.
    /// [`Self`] is initialized to use the compact and dynamically sized sparse representation,
    /// but later switches to the dense representation when it uses equal memory
    /// (`1 << precision` registers, 1 byte each).
    pub fn seeded(precision: u8, seed: u128) -> Self {
        Self::with_hasher(precision, DefaultHasher::seeded(&seed.to_be_bytes()))
    }
}

impl<S: BuildHasher> HyperLogLogPlus<S> {
    /// Returns a new [`Self`] using the provided hasher.
    /// [`Self`] is initialized to use the compact and dynamically sized sparse representation,
    /// but later switches to the dense representation when it uses equal memory
    /// (`1 << precision` registers, 1 byte each).
    pub fn with_hasher(precision: u8, hasher: S) -> Self {
        crate::validate_precision(precision);
        Self {
            sparse: Some(SparseLogLog::new(precision)),
            dense: None,
            hasher,
        }
    }

    #[inline(always)]
    fn insert_sparse(this: &mut Self, h: u64) {
        this.sparse.as_mut().unwrap().insert_hash(h);
        if this.full() {
            this.swap();
        }
    }

    #[inline(always)]
    fn insert_dense(this: &mut Self, h: u64) {
        this.dense.as_mut().unwrap().insert_hash(h);
    }

    /// Inserts the hash of an item into the HyperLogLogPlus.
    /// `self` switches to dense mode if sparse mode exceeds memory usage of dense mode.
    #[inline]
    pub fn insert_hash(&mut self, hash: u64) {
        [Self::insert_dense, Self::insert_sparse][self.is_sparse() as usize](self, hash);
    }

    /// Inserts the item into the HyperLogLogPlus.
    /// `self` switches to dense mode if sparse mode exceeds memory usage of dense mode.
    #[inline]
    pub fn insert<T: Hash + ?Sized>(&mut self, value: &T) {
        self.insert_hash(crate::hash_one(&self.hasher, value));
    }

    /// Inserts all the items in `iter` into the `self`.
    #[inline]
    pub fn insert_all<'a, T: Hash + 'a, I: Iterator<Item = &'a T>>(&mut self, iter: I) {
        for val in iter {
            self.insert(val);
        }
    }

    #[inline(always)]
    fn full(&self) -> bool {
        match self.sparse.as_ref() {
            Some(s) => s.full(),
            _ => false,
        }
    }

    #[cold]
    fn swap(&mut self) {
        let s = self.sparse.take().unwrap();
        self.dense = Some(s.into());
    }

    /// Returns the approximate number of elements in `self`.
    #[inline]
    pub fn count(&mut self) -> usize {
        crate::math::round(self.raw_count()) as usize
    }

    /// Returns the approximate number of elements in `self`.
    #[inline]
    pub fn raw_count(&mut self) -> f64 {
        match self.sparse.as_mut() {
            Some(s) => s.count() as f64,
            _ => self.dense.as_ref().unwrap().raw_count(),
        }
    }

    /// Returns `true` if the current internal representation is sparse,
    /// `false` if using classic dense (HyperLogLog) representation.
    #[inline]
    pub fn is_sparse(&self) -> bool {
        self.sparse.is_some()
    }

    /// Returns the precision of `self`.
    pub fn precision(&self) -> u8 {
        match self.sparse.as_ref() {
            Some(s) => s.precision,
            _ => self.dense.as_ref().unwrap().precision as u8,
        }
    }

    /// Merges another HyperLogLog into `self`, updating the count.
    /// Returns `Err(Error::IncompatibleLength)` if the two HyperLogLogs have
    /// different precision ([`Self::precision`]).
    ///
    /// This does not verify that the HLLs use equal hashers or seeds.
    /// If they are different then `self` will be "corrupted".
    pub fn union(&mut self, other: &Self) -> Result<(), Error> {
        if self.precision() != other.precision() {
            return Err(Error::IncompatibleLength);
        }
        match (self.is_sparse(), other.is_sparse()) {
            (true, true) => self
                .sparse
                .as_mut()
                .unwrap()
                .union(other.sparse.as_ref().unwrap()),
            (false, false) => self
                .dense
                .as_mut()
                .unwrap()
                .union(other.dense.as_ref().unwrap()),
            (true, false) => {
                self.swap();
                self.dense
                    .as_mut()
                    .unwrap()
                    .union(other.dense.as_ref().unwrap())
            }
            (false, true) => {
                let sparse = other.sparse.as_ref().unwrap();
                let dense = self.dense.as_mut().unwrap();
                for encoded in sparse.new.iter() {
                    let (rank, register) = decode_hash(*encoded, sparse.precision);
                    dense.update::<true>(rank as u8, register);
                }
                for encoded in sparse.indexes.into_iter() {
                    let (rank, register) = decode_hash(encoded, sparse.precision);
                    dense.update::<true>(rank as u8, register);
                }
                Ok(())
            }
        }
    }
}

impl<S: BuildHasher> PartialEq for HyperLogLogPlus<S> {
    fn eq(&self, other: &Self) -> bool {
        self.sparse == other.sparse && self.dense == other.dense
    }
}
impl<S: BuildHasher> Eq for HyperLogLogPlus<S> {}

impl<T: Hash, S: BuildHasher> Extend<T> for HyperLogLogPlus<S> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for x in iter.into_iter() {
            self.insert(&x);
        }
    }
}

#[cfg(test)]
mod sparse_tests {
    use super::*;

    #[test]
    fn test_hash_codec() {
        let precision = 18;
        let hash = 90594543u64.wrapping_mul(35235225311);

        let index = (hash >> (64 - precision)) as usize;
        let rank = 1 + hash.trailing_zeros();

        let encoded = encode_hash(hash);
        let (decoded_rank, decoded_index) = decode_hash(encoded, precision);
        assert_eq!(decoded_rank, rank);
        assert_eq!(decoded_index, index);
    }

    #[test]
    fn test_conversion() {
        for seed in 0..=10 {
            let mut sll = HyperLogLogPlus::seeded(12, seed);
            let mut hll = HyperLogLog::seeded(12, seed);
            let num = 10000;
            for x in 0..num {
                sll.insert(&x);
                hll.insert(&x);
            }
            let before = sll.count();
            for x in 0..num {
                sll.insert(&x);
            }
            let after = sll.count();
            assert_eq!(before, after);
            assert_eq!(after, hll.count());
            assert!(!sll.is_sparse());
            assert_eq!(hll, sll.dense.unwrap());
        }
    }

    #[test]
    fn insert_repeat() {
        let mut sll = SparseLogLog::new(16);
        let num = 1000;
        let randnum = fastrand::u64(..);
        for _ in 0..num {
            sll.insert_hash(randnum);
        }
        assert_eq!(crate::math::round(sll.count()) as usize, 1);
    }

    #[test]
    fn test_union() {
        for seed in 0..=100 {
            let ranges = [(0, 0), (0, 1), (0, 50), (0, 2000), (0, 10000), (100, 1000)];
            for (li, lj) in ranges.clone() {
                for (ri, rj) in ranges.clone() {
                    let mut left = HyperLogLogPlus::seeded(12, seed);
                    left.extend(li..lj);
                    let mut right = HyperLogLogPlus::seeded(12, seed);
                    right.extend(ri..rj);
                    let mut control = HyperLogLogPlus::seeded(12, seed);
                    control.extend(li..lj);
                    control.extend(ri..rj);

                    left.union(&right).unwrap();
                    assert_eq!(
                        left.count(),
                        control.count(),
                        "Left: {:?}, Right: {:?}",
                        (li, lj),
                        (ri, rj)
                    );
                    assert_eq!(left, control);
                }
            }
        }
    }
}

#[cfg(test)]
mod var_int_tests {
    use super::*;

    #[test]
    fn diff_vec() {
        let vals = [0, 1, 2, 42, 256, 5515, 99049043, u32::MAX - 1, u32::MAX];

        let mut v = DiffVec::with_size(100, 100);
        for x in vals.iter() {
            v.push(*x);
        }

        for (i, x) in vals.into_iter().enumerate() {
            assert_eq!(x, vals[i]);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_sparse() {
        for seed in 0..=42 {
            for precision in 4..=18 {
                let mut before = HyperLogLogPlus::seeded(precision, seed);
                before.extend(0..=1000);

                let s = serde_cbor::to_vec(&before).unwrap();
                let mut after: HyperLogLogPlus = serde_cbor::from_slice(&s).unwrap();
                assert_eq!(before, after);

                before.extend(1000..=2000);
                after.extend(1000..=2000);
                assert_eq!(before, after);
            }
        }
    }
}
