use core::hash::{BuildHasher, Hash};
extern crate alloc;
use crate::buf::Buf;
use crate::vint::VarInt;
use crate::DefaultHasher;
use crate::HyperLogLog;
use alloc::vec::Vec;

/// We compact the u64 hash into an encoded u32 for storage in the diff vec.
/// When converting to dense representation, encoded hashes are decoded.
/// The trailing 0s and regsiter index of the original u64 hash are recovered from the decoded value.
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

#[derive(Default, Debug)]
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

impl IntoIterator for DiffVec {
    type Item = u32;
    type IntoIter = DiffIter;

    fn into_iter(self) -> Self::IntoIter {
        DiffIter {
            index: 0,
            last: 0,
            inner: self,
        }
    }
}

pub struct DiffIter {
    index: usize,
    last: u32,
    inner: DiffVec,
}

impl Iterator for DiffIter {
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
    fn hll_size_bytes(&self) -> usize {
        (1 << self.precision) as usize
    }

    #[inline]
    fn should_flush(&self) -> bool {
        let dense_hll_size = self.hll_size_bytes() << 2;
        let new_size = self.new.len();
        new_size * Self::NEW_SIZE_FACTOR > dense_hll_size
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
        if self.should_flush() {
            self.flush()
        }
    }

    #[inline]
    pub(crate) fn flush(&mut self) {
        if self.new.is_empty() {
            return;
        }
        self.new.sort_unstable();

        // TODO: empiraclly derive the size from the precision
        let size = self.indexes.size() + (self.new.len() * 3);
        let max_size = self.indexes.size() + (self.new.len() * 5) + 8;
        let mut buf = DiffVec::with_size(size, max_size);
        let y = core::mem::take(&mut self.indexes);
        let v = core::mem::take(&mut self.new);
        let (mut new, mut diffvec) = (v.into_iter(), y.into_iter());
        let (mut new_hash, mut old_hash) = (new.next(), diffvec.next());

        while new_hash.is_some() && old_hash.is_some() {
            let new_hash_ = new_hash.unwrap();
            let old_hash_ = old_hash.unwrap();
            if new_hash_ == old_hash_ {
                buf.push(new_hash_);
                new_hash = new.next();
                old_hash = diffvec.next();
            } else if new_hash_ > old_hash_ {
                buf.push(old_hash_);
                old_hash = diffvec.next();
            } else {
                buf.push(new_hash_);
                new_hash = new.next();
            }
        }
        while let Some(h) = new_hash {
            buf.push(h);
            new_hash = new.next();
        }
        while let Some(h) = old_hash {
            buf.push(h);
            old_hash = diffvec.next();
        }
        self.indexes = buf;
        self.indexes.encoded.shrink_to_fit();
    }

    #[inline]
    pub fn count(&mut self) -> f64 {
        self.flush();
        correction(self.indexes.len())
    }

    #[inline]
    pub fn full(&self) -> bool {
        self.indexes.size() > self.hll_size_bytes()
    }
}

impl From<SparseLogLog> for HyperLogLog {
    fn from(mut sparse: SparseLogLog) -> Self {
        sparse.flush();
        let registers = sparse.indexes.into_iter();
        let mut hll = HyperLogLog::new(sparse.precision);
        for encoded in registers {
            let (rank, register) = decode_hash(encoded, sparse.precision);
            hll.update(rank as u8, register);
        }
        hll
    }
}

/// An implementation of the the HyperLogLog++ data structure.
///
/// For small cardinalities, a "sparse" representation is used. The sparse representation is more accurate and uses less memory,
/// but has slower insert speed.
/// The error and memory usage of the sparse representation scale roughly linearly with the number of items inserted. When
/// the memory of the sparse representation equals the memory of the dense representation, it switches to dense internally.
/// This happens inside the `insert`/`insert_hash` call (which is why they need `&mut self`). The error of the sparse representation
/// never exceeds that of the dense.
pub struct HyperLogLogPlus<S = DefaultHasher> {
    sparse: Option<SparseLogLog>,
    dense: Option<HyperLogLog>,
    hasher: S,
    insert_fn: fn(&mut Self, hash: u64),
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
    /// (`1 << precision` registers (1 byte each)).
    pub fn seeded(precision: u8, seed: u64) -> Self {
        Self::with_hasher(precision, DefaultHasher::seeded(seed))
    }
}

impl<S: BuildHasher> HyperLogLogPlus<S> {
    /// Returns a new [`Self`] using the provided hasher.
    /// [`Self`] is initialized to use the compact and dynamically sized sparse representation,
    /// but later switches to the dense representation when it uses equal memory
    /// (`1 << precision` registers (1 byte each)).
    pub fn with_hasher(precision: u8, hasher: S) -> Self {
        crate::validate_precision(precision);
        Self {
            sparse: Some(SparseLogLog::new(precision)),
            dense: None,
            hasher,
            insert_fn: Self::insert_sparse,
        }
    }

    #[inline(always)]
    fn insert_sparse(this: &mut Self, h: u64) {
        this.sparse.as_mut().unwrap().insert_hash(h);
    }

    #[inline(always)]
    fn insert_dense(this: &mut Self, h: u64) {
        this.dense.as_mut().unwrap().insert_hash(h);
    }

    /// Inserts the hash of an item into the HyperLogLogPlus.
    /// `self` switches to dense mode if sparse mode exceeds memory usage of dense mode.
    #[inline(always)]
    pub fn insert_hash(&mut self, hash: u64) {
        if self.full() {
            self.swap();
        }
        (self.insert_fn)(self, hash);
    }

    /// Inserts the item into the HyperLogLogPlus.
    /// `self` switches to dense mode if sparse mode exceeds memory usage of dense mode.
    #[inline(always)]
    pub fn insert<T: Hash + ?Sized>(&mut self, value: &T) {
        self.insert_hash(crate::hash_one(&self.hasher, value));
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
        self.insert_fn = Self::insert_dense;
    }

    /// Returns the approximate number of elements in `self`.
    #[inline]
    pub fn count(&mut self) -> usize {
        self.raw_count() as usize
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
    pub fn is_sparse(&self) -> bool {
        self.sparse.is_some()
    }

    /*
    /// Returns the expected error for this `HyperLogLogPlus`.
    /// Expected error is lower in sparse mode.
    pub fn expected_error(&mut self) -> f64 {
        match self.sparse.as_mut() {
            Some(s) => {
                s.flush();
                expected_error(s.indexes.len())
            }
            _ => crate::error_for_precision(self.dense.as_ref().unwrap().precision as u8),
        }
    }
    */
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
        let hasher = foldhash::fast::RandomState::default();
        let mut sll = HyperLogLogPlus::with_hasher(12, hasher.clone());
        let mut hll = HyperLogLog::with_hasher(12, hasher);
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
        let hll = hll.count();
        assert_eq!(before, after);
        assert_eq!(after, hll);
        assert!(!sll.is_sparse());
    }

    #[test]
    fn insert_repeat() {
        let mut sll = SparseLogLog::new(16);
        let num = 1000;
        let randnum = fastrand::u64(..);
        for _ in 0..num {
            sll.insert_hash(randnum);
        }
        assert_eq!(sll.count().round() as usize, 1);
    }

    /*
    #[test]
    fn test_error() {
        let mut sll = SparseLogLog::new(16);
        let mut rng = fastrand::Rng::with_seed(643340961);
        let mut true_size = 0;
        while !sll.full() {
            let hash = rng.u64(..);
            sll.insert_hash(hash);
            true_size += 1;

            if true_size & 0b111111 == 0 {
                let real = true_size as f64;
                let est = sll.count();
                let diff = est - real;
                let err = diff.abs() / real;

                let expected = expected_error(sll.indexes.len());
            }
        }
    }
    */
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
}
