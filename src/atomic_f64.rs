use crate::AtomicU64;
use core::sync::atomic::Ordering;

#[derive(Debug)]
pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    #[cfg(not(feature = "loom"))]
    #[inline]
    pub const fn new(float: f64) -> Self {
        Self(AtomicU64::new(u64::from_be_bytes(float.to_be_bytes())))
    }

    #[cfg(feature = "loom")]
    pub fn new(float: f64) -> Self {
        Self(AtomicU64::new(u64::from_be_bytes(float.to_be_bytes())))
    }

    #[inline]
    pub fn load(&self, ordering: Ordering) -> f64 {
        f64::from_bits(self.0.load(ordering))
    }

    #[inline]
    pub fn fetch_sub(&self, val: f64, ordering: Ordering) -> f64 {
        let int = self
            .0
            .fetch_update(ordering, downgrade(ordering), |prev| {
                let new = f64::from_bits(prev) - val;
                Some(u64::from_be_bytes(new.to_be_bytes()))
            })
            .unwrap();
        f64::from_bits(int)
    }
}

#[inline]
fn downgrade(order: Ordering) -> Ordering {
    match order {
        Ordering::Release | Ordering::Relaxed => Ordering::Relaxed,
        Ordering::Acquire | Ordering::AcqRel => Ordering::Acquire,
        Ordering::SeqCst => Ordering::SeqCst,
        _ => todo!(),
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for AtomicF64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f64(self.load(Ordering::SeqCst))
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
