#![allow(rustdoc::bare_urls)]
#![doc = include_str!("../README.md")]

use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU8, Ordering};

mod hasher;
pub use hasher::DefaultHasher;

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HyperLogLog<S = DefaultHasher> {
    registers: Box<[AtomicU8]>,
    precision: u32,
    builder: S,
}

impl HyperLogLog {
    pub fn new(precision: u8) -> Self {
        Self::with_hasher(precision, DefaultHasher::default())
    }

    pub fn seeded(precision: u8, seed: u128) -> Self {
        Self::with_hasher(precision, DefaultHasher::seeded(&seed.to_be_bytes()))
    }
}

impl<S: BuildHasher> HyperLogLog<S> {
    pub fn with_hasher(precision: u8, builder: S) -> Self {
        let count = Self::register_count(precision);
        let mut data = Vec::with_capacity(count);
        for _ in 0..count {
            data.push(AtomicU8::new(0));
        }
        Self {
            builder,
            precision: precision as u32,
            registers: data.into(),
        }
    }

    #[inline(always)]
    pub fn insert<T: Hash + ?Sized>(&self, value: &T) {
        let mut hasher = self.builder.build_hasher();
        value.hash(&mut hasher);
        self.insert_hash(hasher.finish());
    }

    #[inline(always)]
    fn insert_hash(&self, hash64: u64) {
        let mut hash = (hash64 >> 32) as u32;
        let index: usize = (hash >> (32 - self.precision)) as usize;
        hash = (hash << self.precision) | (1 << (self.precision - 1));

        // TODO: <https://graphics.stanford.edu/~seander/bithacks.html>
        let zeros = hash.leading_zeros() as u8;
        self.registers[index].fetch_max(zeros, Ordering::Relaxed);
    }

    #[inline]
    fn register_count(precision: u8) -> usize {
        1 << precision
    }

    #[inline]
    pub fn count(&self) -> usize {
        self.raw_count() as usize
    }

    fn raw_count(&self) -> f64 {
        let count = self.registers.len();
        let mut raw = self.estimate_raw();
        let zeros = self
            .registers
            .iter()
            .map(|x| (x.load(Ordering::Relaxed) == 0) as usize)
            .sum();
        let two32 = (1u64 << 32) as f64;
        if raw <= 2.5 * count as f64 && zeros != 0 {
            raw = Self::linear_count(count, zeros);
        } else if raw > two32 / 30.0 {
            raw = -1.0 * two32 * (1.0 - raw / two32).ln();
        }
        raw
    }

    fn linear_count(count: usize, zeros: usize) -> f64 {
        count as f64 * (count as f64 / zeros as f64).ln()
    }

    fn estimate_raw(&self) -> f64 {
        let count = self.registers.len();
        let raw: f64 = self
            .registers
            .iter()
            .map(|x| 1.0 / (1u64 << x.load(Ordering::Relaxed)) as f64)
            .sum();
        2.0 * Self::alpha(count) * (count * count) as f64 / raw
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
        if self.registers.len() != other.registers.len() {
            return false;
        }
        (0..self.registers.len()).all(|i| {
            let left = self.registers[i].load(Ordering::Relaxed);
            let right = other.registers[i].load(Ordering::Relaxed);
            left == right
        })
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
