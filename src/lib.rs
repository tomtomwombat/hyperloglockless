#![allow(rustdoc::bare_urls)]
#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::{boxed::Box, vec::Vec};
use core::hash::{BuildHasher, Hash, Hasher};
use core::iter::repeat;
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
mod math;
use math::*;

/// HyperLogLog is a data structure for the "count-distinct problem", approximating the number of distinct elements in a multiset.
///
/// # Example
/// ```rust
/// use hyperloglockless::HyperLogLog;
///
/// let mut hll = HyperLogLog::new(16);
/// hll.insert("42");
/// hll.insert("🦀");
///
/// let count = hll.count();
/// ```
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HyperLogLog<S = DefaultHasher> {
    /// `registers[k]` is the maximum trailing zeros for all 64-bit hashes assigned to kth register
    registers: Box<[u8]>,
    /// `registers.len() == 1 << precision`
    precision: u32,
    hasher: S,
    zeros: usize,
    sum: f64,
    correction: f64,
}

/// HyperLogLog is a data structure for the "count-distinct problem", approximating the number of distinct elements in a multiset.
/// [`AtomicHyperLogLog`] is the thread-safe counterpart of [`HyperLogLog`].
///
/// # Example
/// ```rust
/// use hyperloglockless::AtomicHyperLogLog;
///
/// let hll = AtomicHyperLogLog::new(16);
/// hll.insert("42");
/// hll.insert("🦀");
///
/// let count = hll.count();
/// ```
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AtomicHyperLogLog<S = DefaultHasher> {
    /// `registers[k]` is the maximum trailing zeros for all 64-bit hashes assigned to kth register
    registers: Box<[AtomicU8]>,
    /// `registers.len() == 1 << precision`
    precision: u32,
    hasher: S,
    zeros: AtomicUsize,
    sum: AtomicF64,
    correction: f64,
}

impl<S: BuildHasher> HyperLogLog<S> {
    /// Returns a new `HyperLogLog` with `1 << precision` registers (1 byte each)
    /// using the provided hasher.
    pub fn with_hasher(precision: u8, hasher: S) -> Self {
        validate_precision(precision);
        let num_registers = 1 << precision;
        let data: Vec<_> = repeat(0).take(num_registers).collect();
        Self {
            hasher,
            precision: precision as u32,
            zeros: data.len(),
            correction: correction(data.len()),
            registers: data.into(),
            sum: f64::from(num_registers as u32),
        }
    }
}

impl<S: BuildHasher> AtomicHyperLogLog<S> {
    /// Returns a new `AtomicHyperLogLog` with `1 << precision` registers (1 byte each)
    /// using the provided hasher.
    pub fn with_hasher(precision: u8, hasher: S) -> Self {
        validate_precision(precision);
        let num_registers = 1 << precision;
        let data: Vec<_> = repeat(0).take(num_registers).map(AtomicU8::new).collect();
        Self {
            hasher,
            precision: precision as u32,
            zeros: AtomicUsize::new(data.len()),
            correction: correction(data.len()),
            registers: data.into(),
            sum: AtomicF64::new(f64::from(num_registers as u32)),
        }
    }
}

macro_rules! impl_new {
    ($name:ident) => {
        impl $name {
            /// Returns a new [`Self`] with `1 << precision` registers (1 byte each)
            /// using the default hasher with a random seed.
            pub fn new(precision: u8) -> $name<DefaultHasher> {
                $name::with_hasher(precision, DefaultHasher::default())
            }

            /// Returns a new [`Self`] with `1 << precision` registers (1 byte each)
            /// using the default hasher seeded with `seed`.
            pub fn seeded(precision: u8, seed: u128) -> $name<DefaultHasher> {
                $name::with_hasher(precision, DefaultHasher::seeded(&seed.to_be_bytes()))
            }
        }
    };
}

impl_new!(HyperLogLog);
impl_new!(AtomicHyperLogLog);

macro_rules! impl_common {
    ($name:ident) => {
        impl<S: BuildHasher> $name<S> {
            /// Returns the number registers in `self`.
            #[inline(always)]
            pub fn len(&self) -> usize {
                self.registers.len()
            }

            /// Returns the approximate number of elements in `self`.
            #[inline]
            pub fn count(&self) -> usize {
                self.raw_count() as usize
            }

            #[inline(always)]
            fn raw_count_inner(&self, zeros: usize, sum: f64) -> f64 {
                let d = sum + beta_horner(zeros, self.precision);
                self.correction * (self.len() * (self.len() - zeros)) as f64 / d
            }
        }

        impl<T: Hash, S: BuildHasher> Extend<T> for $name<S> {
            #[inline]
            fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
                self.insert_all(iter)
            }
        }

        impl<S: BuildHasher> PartialEq for $name<S> {
            fn eq(&self, other: &Self) -> bool {
                if self.len() != other.len() {
                    return false;
                }
                core::iter::zip(self.iter(), other.iter()).all(|(l, r)| l == r)
            }
        }
        impl<S: BuildHasher> Eq for $name<S> {}
    };
}

impl_common!(HyperLogLog);
impl_common!(AtomicHyperLogLog);

impl<S: BuildHasher> HyperLogLog<S> {
    /// Returns an iterator over the value of each register.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = u8> + '_ {
        self.registers.iter().map(|x| *x)
    }

    /// Inserts the item into the HyperLogLog.
    #[inline(always)]
    pub fn insert<T: Hash + ?Sized>(&mut self, value: &T) {
        let mut hasher = self.hasher.build_hasher();
        value.hash(&mut hasher);
        self.insert_hash(hasher.finish());
    }

    /// Inserts the hash of an item into the HyperLogLog.
    #[inline(always)]
    pub fn insert_hash(&mut self, hash: u64) {
        let index = hash >> (64 - self.precision);
        let new = 1 + hash.trailing_zeros() as u8;
        self.update(new, index as usize);
    }

    #[inline(always)]
    fn update(&mut self, new: u8, index: usize) {
        let old = self.registers[index];
        self.registers[index] = core::cmp::max(self.registers[index], new);
        if old == 0 {
            self.zeros -= 1;
        }
        if old < new {
            let diff = harmonic_term(old) - harmonic_term(new);
            self.sum -= diff;
        }
    }

    /// Inserts all the items in `iter` into the `self`.
    #[inline]
    pub fn insert_all<T: Hash, I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for val in iter {
            self.insert(&val);
        }
    }

    /// Merges another HyperLogLog into `self`, updating the count.
    /// Returns `Err(Error::IncompatibleLength)` if the two HyperLogLogs have
    /// different length ([`Self::len`]).
    #[inline(always)]
    pub fn union(&mut self, other: &Self) -> Result<(), Error> {
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
    pub fn raw_count(&self) -> f64 {
        let zeros = self.zeros;
        let sum = self.sum;
        self.raw_count_inner(zeros, sum)
    }
}

impl<S: BuildHasher> AtomicHyperLogLog<S> {
    /// Returns an iterator over the value of each register.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = u8> + '_ {
        self.registers.iter().map(|x| x.load(Relaxed))
    }

    /// Inserts the item into the HyperLogLog.
    #[inline(always)]
    pub fn insert<T: Hash + ?Sized>(&self, value: &T) {
        let mut hasher = self.hasher.build_hasher();
        value.hash(&mut hasher);
        self.insert_hash(hasher.finish());
    }

    /// Inserts the hash of an item into the HyperLogLog.
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
            let diff = harmonic_term(old) - harmonic_term(new);
            self.sum.fetch_sub(diff, Relaxed);
        }
    }

    /// Inserts all the items in `iter` into the `self`. Immutable version of [`Self::extend`].
    #[inline]
    pub fn insert_all<T: Hash, I: IntoIterator<Item = T>>(&self, iter: I) {
        for val in iter {
            self.insert(&val);
        }
    }

    /// Merges another HyperLogLog into `self`, updating the count.
    /// Returns `Err(Error::IncompatibleLength)` if the two HyperLogLogs have
    /// different length ([`Self::len`]).
    #[inline(always)]
    pub fn union(&self, other: &Self) -> Result<(), Error> {
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
    pub fn raw_count(&self) -> f64 {
        let zeros = self.zeros.load(Relaxed);
        let sum = self.sum.load(Relaxed);
        self.raw_count_inner(zeros, sum)
    }
}

impl<S: BuildHasher + Clone> Clone for AtomicHyperLogLog<S> {
    fn clone(&self) -> Self {
        Self {
            hasher: self.hasher.clone(),
            precision: self.precision,
            zeros: AtomicUsize::new(self.zeros.load(Relaxed)),
            correction: self.correction,
            registers: self.iter().map(AtomicU8::new).collect::<Vec<_>>().into(),
            sum: AtomicF64::new(self.sum.load(Relaxed)),
        }
    }
}

#[inline]
fn validate_precision(precision: u8) {
    assert!(
        (4..=18).contains(&precision),
        "Precisions 4..=18 supported only."
    );
}

/// Returns `1.0 / ((1 << x) as f64)`.
#[inline(always)]
fn harmonic_term(x: u8) -> f64 {
    f64::from_bits(u64::MAX.wrapping_sub(u64::from(x)) << 54 >> 2)
}

/// Returns the HyperLogLog precision that will have the error for calls to `count` and `raw_count`.
#[inline]
pub fn precision_for_error(error: f64) -> u8 {
    assert!(0.0 < error && error < 1.0);
    let bias_constant = 1.0389617614136892; // (3.0 * 2.0f64.ln() - 1.0).sqrt();
    ceil(log2(pow(bias_constant / error, 2.0))) as u8
}

/// Returns the approximate error of `count` and `raw_count` given the precision of a [`HyperLogLog`] or [`AtomicHyperLogLog`].
#[inline]
pub fn error_for_precision(precision: u8) -> f64 {
    validate_precision(precision);
    let bias_constant = 1.0389617614136892; // (3.0 * 2.0f64.ln() - 1.0).sqrt();
    bias_constant / sqrt((1u64 << precision) as f64)
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

macro_rules! impl_tests {
    ($modname:ident, $name:ident) => {
        #[allow(unused_mut)]
        #[cfg(not(feature = "loom"))]
        #[cfg(test)]
        mod $modname {
            use super::*;
            #[test]
            fn test_clone() {
                let mut hll = $name::seeded(4, 42);
                hll.insert_all(1..10);
                let mut cloned = hll.clone();
                assert_eq!(hll, cloned);
                cloned.insert(&42);
                assert!(hll != cloned);
            }

            #[test]
            fn test_low_error() {
                for p in 4..=18 {
                    low_error(p);
                }
            }

            fn low_error(precision: u8) {
                let thresh = error_for_precision(precision) * 1.20; // within 20% of the expected error

                let mut counted = 0;
                let mut total_err = 0f64;
                let mut total_diff = 0f64;

                for seed in 1..=4 {
                    let mut hll = $name::seeded(precision, seed);
                    for x in 1..10_000_000 {
                        hll.insert(&x);
                        if x % 1_000 == 0 {
                            let real = x as f64;
                            let diff = hll.raw_count() - real;
                            total_err += diff.abs() / real;
                            total_diff += diff / real;
                            counted += 1;
                        }
                    }
                }

                let avg_err = total_err / counted as f64;
                assert!(
                    avg_err < thresh,
                    "(p = {}) err = {}, expected {}",
                    precision,
                    avg_err,
                    thresh
                );

                let bias = total_diff.abs() / counted as f64;
                assert!(
                    bias < thresh,
                    "(p = {}) bias = {}, expected {}",
                    precision,
                    bias,
                    thresh
                );
            }

            #[test]
            fn test_union() {
                let mut left = $name::seeded(8, 42);
                let mut right = $name::seeded(8, 42);

                for x in 1..2000 {
                    left.insert(&x);
                }
                for x in 1000..3000 {
                    right.insert(&x);
                }

                left.union(&right).unwrap();

                let real = 3000 as f64;
                let my_acc = (real - (left.count() as f64 - real).abs()) / real;
                assert!(my_acc > 0.75, "{}", my_acc);
            }

            #[cfg(feature = "serde")]
            #[test]
            fn test_serde() {
                for precision in 4..=18 {
                    let mut before = $name::seeded(precision, 42);
                    before.extend(0..=1000);

                    let s = serde_cbor::to_vec(&before).unwrap();
                    let mut after: $name = serde_cbor::from_slice(&s).unwrap();
                    assert_eq!(before, after);

                    before.extend(1000..=2000);
                    after.extend(1000..=2000);
                    assert_eq!(before, after);
                }
            }

            #[test]
            fn test_error_helpers() {
                for precision in 4..=18 {
                    let err = error_for_precision(precision);
                    let prec = precision_for_error(err);
                    assert_eq!(prec, precision);
                }
            }
        }
    };
}

impl_tests!(non_atomic, HyperLogLog);
impl_tests!(atomic, AtomicHyperLogLog);

#[cfg(not(feature = "loom"))]
#[cfg(test)]
mod atomic_parity_tests {
    use super::*;

    #[test]
    fn count_parity() {
        for precision in 4..=18 {
            let mut non = HyperLogLog::seeded(precision, 42);
            non.insert_all(0..=1000);
            let atomic = AtomicHyperLogLog::seeded(precision, 42);
            atomic.insert_all(0..=1000);
            assert_eq!(non.raw_count(), atomic.raw_count());
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_parity() {
        for precision in 4..=18 {
            let mut non = HyperLogLog::seeded(precision, 42);
            non.insert_all(0..=1000);
            let atomic = AtomicHyperLogLog::seeded(precision, 42);
            atomic.insert_all(0..=1000);

            let non_bytes = serde_cbor::to_vec(&non).unwrap();
            let atomic_bytes = serde_cbor::to_vec(&atomic).unwrap();
            assert_eq!(non_bytes, atomic_bytes);

            let non_from_atomic: HyperLogLog = serde_cbor::from_slice(&atomic_bytes).unwrap();
            let atomic_from_non: AtomicHyperLogLog = serde_cbor::from_slice(&non_bytes).unwrap();
            assert_eq!(non_from_atomic, non);
            assert_eq!(atomic_from_non, atomic);
        }
    }
}

#[cfg(feature = "loom")]
#[cfg(test)]
mod loom_tests {
    use super::*;

    #[test]
    fn test_loom() {
        loom::model(|| {
            let hll = loom::sync::Arc::new(AtomicHyperLogLog::seeded(4, 42));
            let expected = AtomicHyperLogLog::seeded(4, 42);
            expected.insert_all(1..=4);
            let handles: Vec<_> = [(1..=2), (2..=4)]
                .into_iter()
                .map(|data| {
                    let v = hll.clone();
                    loom::thread::spawn(move || v.insert_all(data))
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }
            let res = hll.iter().collect::<Vec<_>>();
            assert_eq!(res, expected.iter().collect::<Vec<_>>());
            assert_eq!(hll.zeros.load(Relaxed), expected.zeros.load(Relaxed));
            assert_eq!(hll.sum.load(Relaxed), expected.sum.load(Relaxed));
        });
    }
}
