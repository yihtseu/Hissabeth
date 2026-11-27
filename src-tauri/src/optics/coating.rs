//! Polarization-averaged Fresnel coefficients and thin-film stack coefficients.
//! Bench elements are flat plates in air; angles come from the actual ray.

/// Unpolarized reflectance at a bare interface for |cos incidence|.
/// None means total internal reflection (reflectance 1).
pub fn fresnel(n1: f64, n2: f64, cos_i: f64) -> Option<f64> {
    let ci = cos_i.abs().clamp(0., 1.);
    let s2 = (n1 / n2).powi(2) * (1. - ci * ci);
    if s2 >= 1. {
        return None;
    }
    let ct = (1. - s2).sqrt();
    let rs = ((n1 * ci - n2 * ct) / (n1 * ci + n2 * ct)).powi(2);
    let rp = ((n1 * ct - n2 * ci) / (n1 * ct + n2 * ci)).powi(2);
    Some(0.5 * (rs + rp))
}

type C = [f64; 2];
fn cadd(a: C, b: C) -> C {
    [a[0] + b[0], a[1] + b[1]]
}
fn cmul(a: C, b: C) -> C {
    [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]]
}
fn cdiv(a: C, b: C) -> C {
    let d = b[0] * b[0] + b[1] * b[1];
    [
        (a[0] * b[0] + a[1] * b[1]) / d,
        (a[1] * b[0] - a[0] * b[1]) / d,
    ]
}

/// Characteristic-matrix stack between air (index 1) and a real substrate.
/// Layers are (index, thickness nm) from the incident side. Returns the
/// polarization-averaged (reflectance, transmittance); None on TIR in a layer.
pub fn stack(
    wavelength: f64,
    cos_i: f64,
    layers: &[(f64, f64)],
    substrate: f64,
) -> Option<(f64, f64)> {
    let ci = cos_i.abs().clamp(1e-15, 1.);
    let sin0 = (1. - ci * ci).sqrt();
    let indices: Vec<f64> = layers.iter().map(|l| l.0).collect();
    let mut cosines = Vec::with_capacity(indices.len() + 1);
    for n in indices.iter().chain([&substrate]) {
        let s = sin0 / n;
        if s >= 1. {
            return None;
        }
        cosines.push((1. - s * s).sqrt());
    }
    let mut totals = [0., 0.];
    for p_polarized in [false, true] {
        let admittance = |n: f64, c: f64| if p_polarized { n / c } else { n * c };
        let eta0 = admittance(1., ci);
        let eta_s = admittance(substrate, cosines[indices.len()]);
        // M = product of layer matrices, applied to [1; eta_s].
        let (mut b, mut c) = ([1., 0.], [eta_s, 0.]);
        for (k, &n) in indices.iter().enumerate() {
            let eta = admittance(n, cosines[k]);
            let delta = std::f64::consts::TAU * n * layers[k].1 * cosines[k] / wavelength;
            let (cd, sd) = (delta.cos(), delta.sin());
            let m00 = [cd, 0.];
            let m01 = [0., sd / eta];
            let m10 = [0., sd * eta];
            let m11 = [cd, 0.];
            let (nb, nc) = (
                cadd(cmul(m00, b), cmul(m01, c)),
                cadd(cmul(m10, b), cmul(m11, c)),
            );
            b = nb;
            c = nc;
        }
        let denominator = cadd(cmul([eta0, 0.], b), c);
        let r = cdiv(cadd(cmul([eta0, 0.], b), [-c[0], -c[1]]), denominator);
        let t = cdiv([2. * eta0, 0.], denominator);
        let reflectance = r[0] * r[0] + r[1] * r[1];
        let transmittance = eta_s / eta0 * (t[0] * t[0] + t[1] * t[1]);
        totals[0] += reflectance;
        totals[1] += transmittance.clamp(0., 1.);
    }
    Some((totals[0] / 2., totals[1] / 2.))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bare_interface_matches_closed_form_and_reciprocity() {
        let r = fresnel(1., 1.5, 1.).unwrap();
        assert!((r - 0.04).abs() < 1e-15);
        let c45 = std::f64::consts::FRAC_PI_4.cos();
        let forward = fresnel(1., 1.5, c45).unwrap();
        let sin_t = (1. - c45 * c45).sqrt() / 1.5;
        let reverse = fresnel(1.5, 1., (1. - sin_t * sin_t).sqrt()).unwrap();
        assert!((forward - reverse).abs() < 1e-15, "reciprocity");
        assert!(fresnel(1.5, 1., 0.2).is_none(), "TIR");
        assert_eq!(fresnel(1., 1., 0.7), Some(0.));
    }
    #[test]
    fn stack_without_layers_is_bare_fresnel() {
        for cos in [1., 0.9, std::f64::consts::FRAC_PI_4.cos(), 0.3] {
            let (r, t) = stack(550., cos, &[], 1.5).unwrap();
            let bare = fresnel(1., 1.5, cos).unwrap();
            assert!((r - bare).abs() < 1e-12, "cos {cos}");
            assert!((r + t - 1.).abs() < 1e-12, "conservation cos {cos}");
        }
    }
    #[test]
    fn quarter_wave_layer_matches_closed_form_and_zero_thickness_vanishes() {
        let (r, t) = stack(550., 1., &[(1.38, 550. / 4. / 1.38)], 1.5).unwrap();
        let expected = ((1.5_f64 - 1.38 * 1.38) / (1.5 + 1.38 * 1.38)).powi(2);
        assert!((r - expected).abs() < 1e-12);
        assert!((r + t - 1.).abs() < 1e-9);
        let (r0, _) = stack(550., 1., &[(1.38, 0.), (2.1, 0.)], 1.5).unwrap();
        assert!((r0 - 0.04).abs() < 1e-12);
    }
}
