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
    /// `registers[k]` is the maximum trailing zeros for all 64-bit hashes assigned to kth register
    registers: Box<[AtomicU8]>,
    /// `registers.len() == 1 << precision`
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
    pub fn insert_hash(&self, hash: u64) {
        let index = hash >> (64 - self.precision);
        let zeros = 1 + hash.trailing_zeros() as u8;
        self.registers[index as usize].fetch_max(zeros, Ordering::Relaxed);
    }

    /// Merges another `HyperLogLog` into `self`, updating the count.
    /// Returns `Err(Error::IncompatibleLength)` if the two `HyperLogLog`s have
    /// different length ([`HyperLogLog::len`]).
    #[inline(always)]
    pub fn merge(&self, other: &Self) -> Result<(), Error> {
        if self.len() != other.len() {
            return Err(Error::IncompatibleLength);
        }

        // TODO? if self.hasher != other.hasher { ... }

        for (i, x) in other.iter().enumerate() {
            self.registers[i].fetch_max(x, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Returns the approximate number of elements in `self`.
    #[inline]
    pub fn count(&self) -> usize {
        self.raw_count() as usize
    }

    /// Returns the approximate number of elements in `self`.
    #[inline]
    pub fn raw_count(&self) -> f64 {
        let mut raw = self.estimate_raw();

        // correction for small values
        if raw <= 2.5 * self.len() as f64 {
            let zeros = self.iter().map(|x| (x == 0) as usize).sum();
            if zeros != 0 {
                raw = self.linear_count(zeros);
            }
        }
        raw
    }

    #[inline]
    fn linear_count(&self, zeros: usize) -> f64 {
        self.len() as f64 * (self.len() as f64 / zeros as f64).ln()
    }

    #[inline]
    fn estimate_raw(&self) -> f64 {
        let denom: f64 = self.iter().map(|x| 1.0 / (1u64 << x) as f64).sum();
        let raw = (self.len() * self.len()) as f64 / denom;
        Self::correction(self.len()) * raw
    }

    #[inline]
    fn correction(count: usize) -> f64 {
        // Hardcoded since the result of f64::ln varies by platform
        let base = 0.7213475204444817; // 1.0 / (2.0 * 2.0f64.ln());
        let approx = 1.0794415416798357; // 3.0 * 2.0f64.ln() - 1.0;
        match count {
            16 => 0.673,
            32 => 0.697,
            64 => 0.709,
            _ => base / (1.0 + approx / count as f64),
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
    fn low_error_8() {
        low_error(8, 0.15);
    }

    #[test]
    fn low_error_9() {
        low_error(9, 0.15);
    }

    #[test]
    fn low_error_10() {
        low_error(10, 0.15);
    }

    fn low_error(precision: u8, thresh: f64) {
        let hll = HyperLogLog::seeded(precision, 42);
        let mut counted = 0;
        let mut total_err = 0f64;
        let mut total_diff = 0f64;

        for x in 1..10_000_000 {
            hll.insert(&x);
            if x % 1_00_000 == 0 {
                let real = x as f64;
                let diff = hll.raw_count() - real;
                let err = diff.abs() / real;
                assert!(err < thresh, "{}", err);

                counted += 1;
                total_err += err;
                total_diff += diff / real;
            }
        }

        assert!((total_err - total_diff).abs() / counted as f64 > 0.01);
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
        assert!(my_acc > 0.75, "{}", my_acc);
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
