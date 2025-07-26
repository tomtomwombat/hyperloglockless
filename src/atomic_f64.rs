//! Adapted from <https://github.com/thomcc/atomic_float>.

use crate::AtomicU64;
use core::sync::atomic::Ordering;

#[derive(Debug)]
pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    #[cfg(not(feature = "loom"))]
    #[inline]
    pub fn new(float: f64) -> Self {
        Self(AtomicU64::new(u64::from_ne_bytes(float.to_ne_bytes())))
    }

    #[cfg(feature = "loom")]
    pub fn new(float: f64) -> Self {
        Self(AtomicU64::new(u64::from_ne_bytes(float.to_ne_bytes())))
    }

    #[inline]
    pub fn load(&self, ordering: Ordering) -> f64 {
        f64::from_bits(self.0.load(ordering))
    }

    #[inline]
    pub fn fetch_sub(&self, val: f64, ordering: Ordering) -> f64 {
        let int = self
            .0
            .fetch_update(ordering, ordering, |prev| {
                let new = f64::from_bits(prev) - val;
                Some(u64::from_ne_bytes(new.to_ne_bytes()))
            })
            .unwrap();
        f64::from_bits(int)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for AtomicF64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f64(self.load(Ordering::Relaxed))
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for AtomicF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        f64::deserialize(deserializer).map(AtomicF64::new)
    }
}
