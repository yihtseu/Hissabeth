pub type V3 = [f64; 3];
pub fn add(a: V3, b: V3) -> V3 {
    std::array::from_fn(|i| a[i] + b[i])
}
pub fn sub(a: V3, b: V3) -> V3 {
    std::array::from_fn(|i| a[i] - b[i])
}
pub fn scale(a: V3, s: f64) -> V3 {
    a.map(|x| x * s)
}
pub fn dot(a: V3, b: V3) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn unit(a: V3) -> Result<V3, String> {
    let n = norm(a);
    if n.is_finite() && n > 1e-15 {
        Ok(scale(a, 1. / n))
    } else {
        Err("Degenerate ray direction".into())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
}
impl Matrix {
    pub fn identity() -> Self {
        Self {
            a: 1.,
            b: 0.,
            c: 0.,
            d: 1.,
        }
    }
    pub fn refract(&mut self, power: f64) {
        self.c -= power * self.a;
        self.d -= power * self.b;
    }
    pub fn transfer(&mut self, t: f64, n: f64) {
        self.a += t / n * self.c;
        self.b += t / n * self.d;
    }
}
