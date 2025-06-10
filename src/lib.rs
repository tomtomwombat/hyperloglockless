#![allow(rustdoc::bare_urls)]
#![doc = include_str!("../README.md")]

use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU8, Ordering};

mod hasher;
pub use hasher::DefaultHasher;
mod error;
pub use error::Error;

/// HyperLogLog is a data structure for the "count-distinct problem", approximating the number of distinct elements in a multiset.
///
/// # Example
/// ```rust
/// use hyperloglockless::HyperLogLog;
///
/// let hll = HyperLogLog::new(16);
/// hll.insert("42");
/// hll.insert("🦀");
///
/// let count = hll.count();
/// ```
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HyperLogLog<S = DefaultHasher> {
    registers: Box<[AtomicU8]>,
    precision: u32,
    hasher: S,
}

impl HyperLogLog {
    /// Returns a new `HyperLogLog` with `1 << precision` registers (1 byte each)
    /// using the default hasher with a random seed.
    pub fn new(precision: u8) -> Self {
        Self::with_hasher(precision, DefaultHasher::default())
    }

    /// Returns a new `HyperLogLog` with `1 << precision` registers (1 byte each)
    /// using the default hasher seeded with `seed`.
    pub fn seeded(precision: u8, seed: u128) -> Self {
        Self::with_hasher(precision, DefaultHasher::seeded(&seed.to_be_bytes()))
    }
}

impl<S: BuildHasher> HyperLogLog<S> {
    /// Returns a new `HyperLogLog` with `1 << precision` registers (1 byte each)
    /// using the provided hasher.
    pub fn with_hasher(precision: u8, hasher: S) -> Self {
        let count = 1 << precision;
        let mut data = Vec::with_capacity(count);
        for _ in 0..count {
            data.push(AtomicU8::new(0));
        }
        Self {
            hasher,
            precision: precision as u32,
            registers: data.into(),
        }
    }

    /// Returns the number registers in `self`.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.registers.len()
    }

    /// Returns an iterator over the value of each register.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = u8> + use<'_, S> {
        self.registers.iter().map(|x| x.load(Ordering::Relaxed))
    }

    /// Inserts the item into the `HyperLogLog`.
    #[inline(always)]
    pub fn insert<T: Hash + ?Sized>(&self, value: &T) {
        let mut hasher = self.hasher.build_hasher();
        value.hash(&mut hasher);
        self.insert_hash(hasher.finish());
    }

    /// Inserts the hash of an item into the `HyperLogLog`.
    #[inline(always)]
    pub fn insert_hash(&self, mut hash: u64) {
        // left of the hash is used to get index
        let index = (hash >> (64 - self.precision)) as usize;
        // right is used for leading zeros
        hash = hash << self.precision;

        // TODO: <https://graphics.stanford.edu/~seander/bithacks.html>
        let zeros = 1 + hash.leading_zeros() as u8;
        self.registers[index].fetch_max(zeros, Ordering::Relaxed);
    }

    /// Merges another `HyperLogLog` into `self`, updating the count.
    /// Returns `Err(Error::IncompatibleLength)` if the two `HyperLogLog`s have
    /// different length ([`HyperLogLog::len`]).
    #[inline(always)]
    pub fn merge(&self, other: &Self) -> Result<(), Error> {
        if self.len() != other.len() {
            return Err(Error::IncompatibleLength);
        }

        // TODO? if self.builder != other.builder { ... }

        for (i, x) in other.iter().enumerate() {
            self.registers[i].fetch_max(x, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Returns the approximate number of items in `self`.
    #[inline]
    pub fn count(&self) -> usize {
        self.raw_count() as usize
    }

    /// Returns the approximate number of items in `self`.
    #[inline]
    pub fn raw_count(&self) -> f64 {
        let mut raw = self.estimate_raw();
        let zeros = self.iter().map(|x| (x == 0) as usize).sum();

        // correction for small values
        if raw <= 2.5 * self.len() as f64 && zeros != 0 {
            raw = self.linear_count(zeros);
        }
        raw
    }

    #[inline]
    fn linear_count(&self, zeros: usize) -> f64 {
        self.len() as f64 * (self.len() as f64 / zeros as f64).ln()
    }

    #[inline]
    fn harmonic_denom(&self) -> f64 {
        self.iter().map(|x| 1.0 / (1u64 << x) as f64).sum()
    }

    #[inline]
    fn estimate_raw(&self) -> f64 {
        let count = self.len();
        let raw = self.harmonic_denom();
        Self::alpha(count) * (count * count) as f64 / raw
    }

    #[inline]
    fn alpha(count: usize) -> f64 {
        match count {
            16 => 0.673,
            32 => 0.697,
            64 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / count as f64),
        }
    }
}

impl<T: Hash, S: BuildHasher> Extend<T> for HyperLogLog<S> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for val in iter {
            self.insert(&val);
        }
    }
}

impl<S: BuildHasher> PartialEq for HyperLogLog<S> {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        std::iter::zip(self.iter(), other.iter()).all(|(l, r)| l == r)
    }
}
impl<S: BuildHasher> Eq for HyperLogLog<S> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        let c = HyperLogLog::seeded(8, 42);

        for x in 1..10_000_000 {
            c.insert(&x);
            if x % 1_000_000 == 0 {
                let real = x as f64;
                let my_acc = (real - (c.count() as f64 - real).abs()) / real;
                assert!(my_acc > 0.75);
            }
        }
    }

    #[test]
    fn test_merge() {
        let left = HyperLogLog::seeded(8, 42);
        let right = HyperLogLog::seeded(8, 42);

        for x in 1..2000 {
            left.insert(&x);
        }
        for x in 1000..3000 {
            right.insert(&x);
        }

        left.merge(&right).unwrap();

        let real = 3000 as f64;
        let my_acc = (real - (left.count() as f64 - real).abs()) / real;
        assert!(my_acc > 0.9);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde() {
        for precision in 1..=16 {
            let mut before = HyperLogLog::seeded(precision, 42);
            before.extend(0..=1000);

            let s = serde_json::to_vec(&before).unwrap();
            let mut after: HyperLogLog = serde_json::from_slice(&s).unwrap();
            assert_eq!(before, after);

            before.extend(1000..=2000);
            after.extend(1000..=2000);
            assert_eq!(before, after);
        }
    }
}
