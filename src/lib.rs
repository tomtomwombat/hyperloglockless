#![allow(rustdoc::bare_urls)]
#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::{boxed::Box, vec::Vec};
use core::hash::{BuildHasher, Hash};
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

mod sparse;
pub use sparse::HyperLogLogPlus;
mod buf;
mod vint;

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
        let registers: Vec<_> = repeat(0).take(num_registers).collect();
        Self {
            hasher,
            precision: precision as u32,
            zeros: registers.len(),
            correction: correction(registers.len()),
            registers: registers.into(),
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
    #[inline]
    pub fn insert<T: Hash + ?Sized>(&mut self, value: &T) {
        self.insert_hash(hash_one(&self.hasher, value));
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
        self.registers[index] = new.max(old);
        self.zeros -= (old == 0 && new > 0) as usize;
        let diff = INV_POW2[old as usize] - INV_POW2[new as usize];
        self.sum -= diff.max(0.0);
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
    ///
    /// This does not verify that the HLLs use the same hasher or seed.
    /// If they are different then `self` will be "corrupted".
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
        self.raw_count_inner(self.zeros, self.sum)
    }

    /// Low level method to expose de/serializable parts of `self`.
    pub fn parts<'a>(&'a self) -> (&'a [u8], &'a S, usize, f64) {
        (&self.registers, &self.hasher, self.zeros, self.sum)
    }

    /// Low level method to construct [`Self`] de/serializable parts.
    ///
    /// # Example
    /// ```
    /// use hyperloglockless::HyperLogLog;
    ///
    /// let mut before = HyperLogLog::seeded(16, 42);
    /// before.extend(1000..=2000);
    /// let (x, y, z, w) = before.parts();
    /// let after = HyperLogLog::from_parts(x.into(), y.clone(), z, w);
    /// assert_eq!(before, after);
    /// ```
    pub fn from_parts(registers: Box<[u8]>, hasher: S, zeros: usize, sum: f64) -> Self {
        let len = registers.len() as u64;
        let precision = len.trailing_zeros();
        assert_eq!(
            precision + len.leading_zeros(),
            63,
            "resigers.len() not a power of 2"
        );
        assert_eq!(1 << precision, registers.len());
        validate_precision(precision as u8);
        Self {
            hasher,
            precision,
            zeros,
            correction: correction(registers.len()),
            registers,
            sum,
        }
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
        self.insert_hash(hash_one(&self.hasher, value));
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
        if old < new {
            self.zeros.fetch_sub((old == 0) as usize, Relaxed);
            let diff = INV_POW2[old as usize] - INV_POW2[new as usize];
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

    /// Low level method to expose de/serializable parts of `self`.
    pub fn parts<'a>(&'a self) -> (&'a [AtomicU8], &'a S, usize, f64) {
        (
            &self.registers,
            &self.hasher,
            self.zeros.load(Relaxed),
            self.sum.load(Relaxed),
        )
    }

    /// Low level method to construct [`Self`] de/serializable parts.
    pub fn from_parts(registers: Box<[AtomicU8]>, hasher: S, zeros: usize, sum: f64) -> Self {
        let len = registers.len() as u64;
        let precision = len.trailing_zeros();
        assert_eq!(
            precision + len.leading_zeros(),
            63,
            "resigers.len() not a power of 2"
        );
        assert_eq!(1 << precision, registers.len());
        validate_precision(precision as u8);
        Self {
            hasher,
            precision,
            zeros: AtomicUsize::new(zeros),
            correction: correction(registers.len()),
            registers,
            sum: AtomicF64::new(sum),
        }
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

static INV_POW2: [f64; 66] = [
    1.0,
    0.5,
    0.25,
    0.125,
    0.0625,
    0.03125,
    0.015625,
    0.0078125,
    0.00390625,
    0.001953125,
    0.0009765625,
    0.00048828125,
    0.000244140625,
    0.0001220703125,
    0.00006103515625,
    0.000030517578125,
    0.0000152587890625,
    0.00000762939453125,
    0.000003814697265625,
    0.0000019073486328125,
    0.00000095367431640625,
    0.000000476837158203125,
    0.0000002384185791015625,
    0.00000011920928955078125,
    0.000000059604644775390625,
    0.0000000298023223876953125,
    0.00000001490116119384765625,
    0.000000007450580596923828125,
    0.0000000037252902984619140625,
    0.00000000186264514923095703125,
    0.000000000931322574615478515625,
    0.0000000004656612873077392578125,
    0.00000000023283064365386962890625,
    0.000000000116415321826934814453125,
    0.0000000000582076609134674072265625,
    0.00000000002910383045673370361328125,
    0.000000000014551915228366851806640625,
    0.0000000000072759576141834259033203125,
    0.00000000000363797880709171295166015625,
    0.000000000001818989403545856475830078125,
    0.0000000000009094947017729282379150390625,
    0.00000000000045474735088646411895751953125,
    0.000000000000227373675443232059478759765625,
    0.0000000000001136868377216160297393798828125,
    0.00000000000005684341886080801486968994140625,
    0.000000000000028421709430404007434844970703125,
    0.0000000000000142108547152020037174224853515625,
    0.00000000000000710542735760100185871124267578125,
    0.000000000000003552713678800500929355621337890625,
    0.0000000000000017763568394002504646778106689453125,
    0.00000000000000088817841970012523233890533447265625,
    0.000000000000000444089209850062616169452667236328125,
    0.0000000000000002220446049250313080847263336181640625,
    0.00000000000000011102230246251565404236316680908203125,
    0.000000000000000055511151231257827021181583404541015625,
    0.0000000000000000277555756156289135105907917022705078125,
    0.00000000000000001387778780781445675529539585113525390625,
    0.000000000000000006938893903907228377647697925567626953125,
    0.0000000000000000034694469519536141888238489627838134765625,
    0.00000000000000000173472347597680709441192448139190673828125,
    0.000000000000000000867361737988403547205962240695953369140625,
    0.0000000000000000004336808689942017736029811203479766845703125,
    0.00000000000000000021684043449710088680149056017398834228515625,
    0.000000000000000000108420217248550443400745280086994171142578125,
    0.0000000000000000000542101086242752217003726400434970855712890625,
    0.00000000000000000002710505431213761085018632002174854278564453125,
];

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

#[inline(always)]
pub(crate) fn hash_one<S: BuildHasher, T: Hash + ?Sized>(hasher: &S, value: &T) -> u64 {
    use core::hash::Hasher;
    let mut h = hasher.build_hasher();
    value.hash(&mut h);
    h.finish()
}

macro_rules! impl_tests {
    ($modname:ident, $name:ident, $seed:literal) => {
        #[allow(unused_mut)]
        #[cfg(not(feature = "loom"))]
        #[cfg(test)]
        mod $modname {
            use super::*;
            #[test]
            fn test_clone() {
                let mut hll = $name::seeded(4, $seed);
                hll.insert_all(1..10);
                let mut cloned = hll.clone();
                assert_eq!(hll, cloned);
                for x in 0..=1000 {
                    cloned.insert(&x);
                }
                assert!(hll != cloned);
            }

            #[test]
            fn test_low_error() {
                for p in 4..=18 {
                    low_error(p);
                }
            }

            fn low_error(precision: u8) {
                let thresh = error_for_precision(precision) * 1.3; // within 30% of the expected error

                let mut counted = 0;
                let mut total_err = 0f64;
                let mut total_diff = 0f64;

                for seed in 1..=4 {
                    let mut hll = $name::seeded(precision, seed);
                    let mut rng = fastrand::Rng::with_seed(643340961);
                    for x in 1..10_000_000 {
                        let hash = rng.u64(..);
                        hll.insert_hash(hash);
                        if x % 10 == 0 {
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
                for p in 4..=18 {
                    for seed in 0..=100 {
                        let ranges = [(0, 0), (0, 1), (0, 50), (0, 2000), (0, 10000), (100, 1000)];
                        for (li, lj) in ranges.clone() {
                            for (ri, rj) in ranges.clone() {
                                let mut left = $name::seeded(p, seed);
                                let mut right = $name::seeded(p, seed);
                                let mut control = $name::seeded(p, seed);

                                left.insert_all(li..lj);
                                right.insert_all(ri..rj);
                                control.insert_all(li..lj);
                                control.insert_all(ri..rj);

                                left.union(&right).unwrap();
                                assert_eq!(left.raw_count(), control.raw_count());
                                assert_eq!(left, control);
                            }
                        }
                    }
                }
            }

            #[cfg(feature = "serde")]
            #[test]
            fn test_serde() {
                for precision in 4..=18 {
                    let mut before = $name::seeded(precision, $seed);
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

impl_tests!(non_atomic, HyperLogLog, 42);
impl_tests!(non_atomic_0_seed, HyperLogLog, 0);
impl_tests!(atomic, AtomicHyperLogLog, 42);

#[cfg(test)]
mod other_tests {
    use super::*;

    #[test]
    fn test_parts() {
        for precision in 4..=18 {
            let mut before = HyperLogLog::seeded(precision, 42);
            before.extend(1000..=2000);
            let (x, y, z, w) = before.parts();
            let after = HyperLogLog::from_parts(x.into(), y.clone(), z, w);
            assert_eq!(before, after);
        }
    }

    #[test]
    fn test_parts_atomic() {
        for precision in 4..=18 {
            let mut before = AtomicHyperLogLog::seeded(precision, 42);
            before.extend(1000..=2000);
            let (x, y, z, w) = before.parts();
            let f = x
                .iter()
                .map(|g| AtomicU8::new(g.load(Relaxed)))
                .collect::<Vec<AtomicU8>>()
                .into();
            let after = AtomicHyperLogLog::from_parts(f, y.clone(), z, w);
            assert_eq!(before, after);
        }
    }
}

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

#[cfg(test)]
mod misc_tests {
    use super::*;

    #[test]
    fn inv_pow_correct() {
        for i in 0..=65 {
            let expected = 1.0 / ((1u128 << i) as f64);
            assert_eq!(expected, INV_POW2[i]);
        }
    }
}
