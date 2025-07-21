#[cfg(all(feature = "atomic", feature = "loom"))]
pub(crate) use loom::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize};

#[cfg(all(feature = "atomic", not(feature = "loom")))]
pub(crate) use core::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize};

#[cfg(feature = "atomic")]
pub(crate) type Register = AtomicU8;

#[cfg(not(feature = "atomic"))]
pub(crate) type Register = u8;

#[cfg(feature = "atomic")]
pub(crate) type Float = crate::atomic_f64::AtomicF64;

#[cfg(not(feature = "atomic"))]
pub(crate) type Float = f64;

#[cfg(feature = "atomic")]
pub(crate) type Usize = AtomicUsize;

#[cfg(not(feature = "atomic"))]
pub(crate) type Usize = usize;

#[cfg(not(feature = "atomic"))]
#[inline(always)]
pub(crate) fn register(x: u8) -> Register {
    x
}

#[cfg(feature = "atomic")]
#[inline(always)]
pub(crate) fn register(x: u8) -> Register {
    Register::new(x)
}

#[cfg(not(feature = "atomic"))]
#[inline(always)]
pub(crate) fn float(x: f64) -> Float {
    x
}

#[cfg(feature = "atomic")]
#[inline(always)]
pub(crate) fn float(x: f64) -> Float {
    Float::new(x)
}

#[cfg(not(feature = "atomic"))]
#[inline(always)]
pub(crate) fn usize(x: usize) -> Usize {
    x
}

#[cfg(feature = "atomic")]
#[inline(always)]
pub(crate) fn usize(x: usize) -> Usize {
    Usize::new(x)
}
