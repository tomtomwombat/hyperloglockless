#[cfg(feature = "std")]
#[inline]
pub(crate) fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[cfg(not(feature = "std"))]
#[inline]
pub(crate) fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
}

#[cfg(feature = "std")]
#[inline]
pub(crate) fn pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

#[cfg(not(feature = "std"))]
#[inline]
pub(crate) fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

#[cfg(feature = "std")]
#[inline]
pub(crate) fn ln(x: f64) -> f64 {
    x.ln()
}

#[cfg(not(feature = "std"))]
#[inline]
pub(crate) fn ln(x: f64) -> f64 {
    libm::log(x)
}

#[cfg(feature = "std")]
#[inline]
pub(crate) fn log2(x: f64) -> f64 {
    x.log2()
}

#[cfg(not(feature = "std"))]
#[inline]
pub(crate) fn log2(x: f64) -> f64 {
    libm::log2(x)
}

#[cfg(feature = "std")]
#[inline]
pub(crate) fn ceil(x: f64) -> f64 {
    x.ceil()
}

#[cfg(not(feature = "std"))]
#[inline]
pub(crate) fn ceil(x: f64) -> f64 {
    libm::ceil(x)
}
