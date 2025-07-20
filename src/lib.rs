#![allow(rustdoc::bare_urls)]
#![doc = include_str!("../README.md")]
#![no_std]

extern crate alloc;
use alloc::{boxed::Box, vec::Vec};
use core::hash::{BuildHasher, Hash, Hasher};
use core::sync::atomic::Ordering::Relaxed;

#[cfg(feature = "loom")]
pub(crate) use loom::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize};

#[cfg(not(feature = "loom"))]
pub(crate) use core::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize};

#[cfg(all(feature = "loom", feature = "serde"))]
compile_error!("features `loom` and `serde` are mutually exclusive");

mod atomic_f64;
use atomic_f64::AtomicF64;
mod beta;
use beta::beta_horner;
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
    zeros: AtomicUsize,
    sum: AtomicF64,
    correction: f64,
}

impl HyperLogLog {
    /// Returns a new `HyperLogLog` with `1 << precision` registers (1 byte each)
    /// using the default hasher with a random seed.
    pub fn new(precision: u8) -> HyperLogLog<DefaultHasher> {
        HyperLogLog::with_hasher(precision, DefaultHasher::default())
    }

    /// Returns a new `HyperLogLog` with `1 << precision` registers (1 byte each)
    /// using the default hasher seeded with `seed`.
    pub fn seeded(precision: u8, seed: u128) -> HyperLogLog<DefaultHasher> {
        HyperLogLog::with_hasher(precision, DefaultHasher::seeded(&seed.to_be_bytes()))
    }

    /// Returns the HyperLogLog precision that will have the error for calls to [`Self::count()`] and [`Self::raw_count()`].
    #[inline]
    pub fn precision_for_error(error: f64) -> u8 {
        assert!(0.0 < error && error < 1.0);
        let bias_constant = 1.0389617614136892; // (3.0 * 2.0f64.ln() - 1.0).sqrt();
        (bias_constant / error).powf(2.0).log2().ceil() as u8
    }

    /// Returns the approximate error of [`Self::count()`] and [`Self::raw_count()`] given the precision of a [`HyperLogLog`].
    #[inline]
    pub fn error_for_precision(precision: u8) -> f64 {
        Self::validate_precision(precision);
        let bias_constant = 1.0389617614136892; // (3.0 * 2.0f64.ln() - 1.0).sqrt();
        bias_constant / ((1u64 << precision) as f64).sqrt()
    }
}

impl<S: BuildHasher> HyperLogLog<S> {
    #[inline]
    fn validate_precision(precision: u8) {
        assert!(
            (4..=18).contains(&precision),
            "Precisions 4..=18 supported only."
        );
    }

    /// Returns a new `HyperLogLog` with `1 << precision` registers (1 byte each)
    /// using the provided hasher.
    pub fn with_hasher(precision: u8, hasher: S) -> Self {
        Self::validate_precision(precision);
        let num_registers = 1 << precision;
        let mut data = Vec::with_capacity(num_registers);
        for _ in 0..num_registers {
            data.push(AtomicU8::new(0));
        }
        Self {
            hasher,
            precision: precision as u32,
            zeros: AtomicUsize::new(data.len()),
            correction: Self::correction(data.len()),
            registers: data.into(),
            sum: AtomicF64::new(f64::from(num_registers as u32)),
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
        self.registers.iter().map(|x| x.load(Relaxed))
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
        let new = 1 + hash.trailing_zeros() as u8;
        self.update(new, index as usize);
    }

    #[inline(always)]
    fn update(&self, new: u8, index: usize) {
        let old = self.registers[index].fetch_max(new, Relaxed);
        if old == 0 {
            self.zeros.fetch_sub(1, Relaxed);
        }
        if old < new {
            let diff = Self::harmonic_term(old) - Self::harmonic_term(new);
            self.sum.fetch_sub(diff, Relaxed);
        }
    }

    /// Returns `1.0 / ((1 << x) as f64)`.
    #[inline(always)]
    fn harmonic_term(x: u8) -> f64 {
        f64::from_bits(u64::MAX.wrapping_sub(u64::from(x)) << 54 >> 2)
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
            self.update(x, i);
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
        let zeros = self.zeros.load(Relaxed);
        let sum = self.sum.load(Relaxed);
        self.raw_count_inner(zeros, sum)
    }

    #[inline(always)]
    pub fn raw_count_inner(&self, zeros: usize, sum: f64) -> f64 {
        let d = sum + beta_horner(zeros, self.precision);
        self.correction * (self.len() * (self.len() - zeros)) as f64 / d
    }

    #[inline(always)]
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

    /// Inserts all the items in `iter` into the `self`. Immutable version of [`Self::extend`].
    #[inline]
    pub fn insert_all<T: Hash, I: IntoIterator<Item = T>>(&self, iter: I) {
        for val in iter {
            self.insert(&val);
        }
    }
}

impl<T: Hash, S: BuildHasher> Extend<T> for HyperLogLog<S> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.insert_all(iter)
    }
}

impl<S: BuildHasher> PartialEq for HyperLogLog<S> {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        core::iter::zip(self.iter(), other.iter()).all(|(l, r)| l == r)
    }
}
impl<S: BuildHasher> Eq for HyperLogLog<S> {}

#[cfg(not(feature = "loom"))]
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
        for precision in 4..=18 {
            let mut before = HyperLogLog::seeded(precision, 42);
            before.extend(0..=1000);

            let s = serde_cbor::to_vec(&before).unwrap();
            let mut after: HyperLogLog = serde_cbor::from_slice(&s).unwrap();
            assert_eq!(before, after);

            before.extend(1000..=2000);
            after.extend(1000..=2000);
            assert_eq!(before, after);
        }
    }

    #[test]
    fn test_error_helpers() {
        for precision in 4..=63 {
            let err = HyperLogLog::error_for_precision(precision);
            let prec = HyperLogLog::precision_for_error(err);
            assert_eq!(prec, precision);
        }
    }

    #[test]
    fn test_not_loom() {
        let hll = HyperLogLog::seeded(4, 42);
        for x in 1..=100 {
            hll.insert(&x);
        }

        for x in 1..=8 {
            hll.insert(&x);
        }
        assert_eq!(hll.count(), 84);
    }
}

#[cfg(feature = "loom")]
#[cfg(test)]
mod loom_tests {
    use super::*;

    #[test]
    fn test_loom() {
        loom::model(|| {
            let hll = HyperLogLog::seeded(4, 42);
            for x in 8..=100 {
                hll.insert(&x);
            }
            let arc_hll = loom::sync::Arc::new(hll);

            let handles: Vec<_> = (1..=2)
                .map(|_| {
                    let v = arc_hll.clone();
                    loom::thread::spawn(move || {
                        for x in 1..=8 {
                            v.insert(&x);
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }

            assert_eq!(arc_hll.count(), 84);
        });
    }
}
