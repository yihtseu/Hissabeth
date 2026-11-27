//! Scalars for evaluating dispersion formulas either at a point, over a wavelength
//! interval with guaranteed enclosure, or together with their derivative.
//!
//! `Iv` widens every elementary result outward by one ulp on each side, so an
//! enclosure stays valid under floating-point rounding: `+ − × ÷ sqrt` are correctly
//! rounded, integer powers are built from such steps, and `powf` gets two ulps.
//! `Dual` carries a forward-mode derivative; `Dual<Iv>` encloses both the value and
//! the slope over an interval.

use std::ops::{Add, Div, Mul, Sub};

pub trait Real:
    Copy
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Add<f64, Output = Self>
    + Sub<f64, Output = Self>
    + Mul<f64, Output = Self>
    + Div<f64, Output = Self>
{
    fn cst(value: f64) -> Self;
    fn sqrt(self) -> Self;
    fn powi(self, n: i32) -> Self;
    fn powf(self, p: f64) -> Self;
    /// Lower and upper bounds of the value (equal for a point).
    fn lo(&self) -> f64;
    fn hi(&self) -> f64;
    /// Every component is finite.
    fn finite(&self) -> bool;
    fn positive(&self) -> bool {
        self.finite() && self.lo() > 0.
    }
}

impl Real for f64 {
    fn cst(value: f64) -> Self {
        value
    }
    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }
    fn powi(self, n: i32) -> Self {
        f64::powi(self, n)
    }
    fn powf(self, p: f64) -> Self {
        f64::powf(self, p)
    }
    fn lo(&self) -> f64 {
        *self
    }
    fn hi(&self) -> f64 {
        *self
    }
    fn finite(&self) -> bool {
        self.is_finite()
    }
}

/// Closed interval [lo, hi]. An operation whose result is undefined somewhere in
/// its operands (division by an interval containing zero, a root of a negative
/// value) yields a non-finite interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Iv {
    pub lo: f64,
    pub hi: f64,
}

impl Iv {
    pub fn new(lo: f64, hi: f64) -> Self {
        Self { lo, hi }
    }
    fn out(lo: f64, hi: f64) -> Self {
        if lo.is_nan() || hi.is_nan() {
            return Self::undefined();
        }
        Self { lo: lo.next_down(), hi: hi.next_up() }
    }
    fn square(self) -> Self {
        if self.lo >= 0. {
            Self::out(self.lo * self.lo, self.hi * self.hi)
        } else if self.hi <= 0. {
            Self::out(self.hi * self.hi, self.lo * self.lo)
        } else {
            let m = self.lo.abs().max(self.hi);
            Self::out(0., m * m)
        }
    }
    fn undefined() -> Self {
        Self { lo: f64::NEG_INFINITY, hi: f64::INFINITY }
    }
    fn hull(values: [f64; 4]) -> Self {
        let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if values.iter().any(|v| v.is_nan()) {
            return Self::undefined();
        }
        Self::out(lo, hi)
    }
}

impl Add for Iv {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::out(self.lo + o.lo, self.hi + o.hi)
    }
}
impl Sub for Iv {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::out(self.lo - o.hi, self.hi - o.lo)
    }
}
impl Mul for Iv {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self::hull([self.lo * o.lo, self.lo * o.hi, self.hi * o.lo, self.hi * o.hi])
    }
}
impl Div for Iv {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        if !(o.lo > 0. || o.hi < 0.) {
            return Self::undefined();
        }
        Self::hull([self.lo / o.lo, self.lo / o.hi, self.hi / o.lo, self.hi / o.hi])
    }
}
impl Add<f64> for Iv {
    type Output = Self;
    fn add(self, o: f64) -> Self {
        self + Self::cst(o)
    }
}
impl Sub<f64> for Iv {
    type Output = Self;
    fn sub(self, o: f64) -> Self {
        self - Self::cst(o)
    }
}
impl Mul<f64> for Iv {
    type Output = Self;
    fn mul(self, o: f64) -> Self {
        self * Self::cst(o)
    }
}
impl Div<f64> for Iv {
    type Output = Self;
    fn div(self, o: f64) -> Self {
        self / Self::cst(o)
    }
}

impl Real for Iv {
    fn cst(value: f64) -> Self {
        Self { lo: value, hi: value }
    }
    fn sqrt(self) -> Self {
        if self.lo < 0. {
            return Self::undefined();
        }
        Self::out(self.lo.sqrt(), self.hi.sqrt())
    }
    fn powi(self, n: i32) -> Self {
        // Every step is one correctly rounded operation widened outward, so the
        // enclosure holds however many multiplications the power takes.
        match n {
            ..0 => Self::cst(1.) / self.powi(-n),
            0 => Self::cst(1.),
            1 => self,
            _ if n % 2 == 0 => self.square().powi(n / 2),
            _ => self * self.powi(n - 1),
        }
    }
    fn powf(self, p: f64) -> Self {
        if self.lo < 0. {
            return Self::undefined();
        }
        // libm `powf` is not guaranteed correctly rounded: widen by a second ulp.
        let (a, b) = (self.lo.powf(p), self.hi.powf(p));
        let wide = Self::out(a.min(b), a.max(b));
        Self::out(wide.lo, wide.hi)
    }
    fn lo(&self) -> f64 {
        self.lo
    }
    fn hi(&self) -> f64 {
        self.hi
    }
    fn finite(&self) -> bool {
        self.lo.is_finite() && self.hi.is_finite()
    }
}

/// Value and first derivative with respect to one variable.
#[derive(Clone, Copy, Debug)]
pub struct Dual<T> {
    pub v: T,
    pub d: T,
}

impl<T: Real> Dual<T> {
    pub fn var(v: T) -> Self {
        Self { v, d: T::cst(1.) }
    }
}

impl<T: Real> Add for Dual<T> {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self { v: self.v + o.v, d: self.d + o.d }
    }
}
impl<T: Real> Sub for Dual<T> {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self { v: self.v - o.v, d: self.d - o.d }
    }
}
impl<T: Real> Mul for Dual<T> {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self { v: self.v * o.v, d: self.v * o.d + self.d * o.v }
    }
}
impl<T: Real> Div for Dual<T> {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        Self { v: self.v / o.v, d: (self.d * o.v - self.v * o.d) / (o.v * o.v) }
    }
}
impl<T: Real> Add<f64> for Dual<T> {
    type Output = Self;
    fn add(self, o: f64) -> Self {
        Self { v: self.v + o, d: self.d }
    }
}
impl<T: Real> Sub<f64> for Dual<T> {
    type Output = Self;
    fn sub(self, o: f64) -> Self {
        Self { v: self.v - o, d: self.d }
    }
}
impl<T: Real> Mul<f64> for Dual<T> {
    type Output = Self;
    fn mul(self, o: f64) -> Self {
        Self { v: self.v * o, d: self.d * o }
    }
}
impl<T: Real> Div<f64> for Dual<T> {
    type Output = Self;
    fn div(self, o: f64) -> Self {
        Self { v: self.v / o, d: self.d / o }
    }
}

impl<T: Real> Real for Dual<T> {
    fn cst(value: f64) -> Self {
        Self { v: T::cst(value), d: T::cst(0.) }
    }
    fn sqrt(self) -> Self {
        let root = self.v.sqrt();
        Self { v: root, d: self.d / (root * 2.) }
    }
    fn powi(self, n: i32) -> Self {
        if n == 0 {
            return Self::cst(1.);
        }
        Self { v: self.v.powi(n), d: self.v.powi(n - 1) * self.d * f64::from(n) }
    }
    fn powf(self, p: f64) -> Self {
        Self { v: self.v.powf(p), d: self.v.powf(p - 1.) * self.d * p }
    }
    fn lo(&self) -> f64 {
        self.v.lo()
    }
    fn hi(&self) -> f64 {
        self.v.hi()
    }
    fn finite(&self) -> bool {
        self.v.finite() && self.d.finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_encloses_point_samples_and_derivative() {
        let f = |x: Dual<Iv>| (x * x * 0.3 + 1.).sqrt() / (x - 0.1) + x.powf(3.5) - x.powi(-2) * 0.01;
        let g = |x: f64| (0.3 * x * x + 1.).sqrt() / (x - 0.1) + x.powf(3.5) - 0.01 * x.powi(-2);
        let h = 1e-6;
        let (a, b) = (0.4, 0.7);
        let enclosure = f(Dual::var(Iv::new(a, b)));
        for i in 0..=100 {
            let x = a + (b - a) * f64::from(i) / 100.;
            let value = g(x);
            let slope = (g(x + h) - g(x - h)) / (2. * h);
            assert!(enclosure.v.lo <= value && value <= enclosure.v.hi);
            assert!(enclosure.d.lo <= slope + 1e-6 && slope - 1e-6 <= enclosure.d.hi);
            let point = f(Dual::var(Iv::cst(x)));
            assert!((point.v.lo - value).abs() < 1e-12 && (point.d.lo - slope).abs() < 1e-6);
        }
        // Review counterexample: x⁴ by consecutive f64 products is ~1.1 ulp above
        // the exact value. Compare against x⁴ in double-double (error ≪ 1 ulp):
        // x² = s + e exactly, x⁴ = s² + 2se + e² with s² = h + l exactly.
        let x = 0.1004_f64;
        let x4 = Iv::cst(x).powi(4);
        let s = x * x;
        let e = x.mul_add(x, -s);
        let h = s * s;
        let l = s.mul_add(s, -h) + 2. * s * e + e * e;
        // h − bound is exact (Sterbenz), so these signs are those of exact − bound.
        assert!((h - x4.lo) + l > 0. && (x4.hi - h) - l > 0., "{x4:?} vs {h} + {l}");
        assert!(!(Iv::new(-1., 1.).sqrt()).finite());
        assert!(!(Iv::cst(1.) / Iv::new(-1., 1.)).finite());
    }
}
