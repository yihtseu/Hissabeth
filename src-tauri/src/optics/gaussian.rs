// SPDX-License-Identifier: GPL-3.0-or-later
// Paraxial Gaussian beam (q parameter) propagation through the system matrix
// and Gaussian overlap fiber coupling. Unclipped, unaberrated, on-axis model.
use super::{System, math::Matrix};
use serde_json::Value;

#[derive(Clone, Copy)]
struct Q {
    re: f64,
    im: f64,
}
impl Q {
    // Reduced-coordinate matrix [a,b;c,d] between media n1 and n2 maps to the
    // geometrical ABCD law q2 = (A q + B)/(C q + D) with A=a, B=b n1, C=c/n2, D=d n1/n2.
    fn transform(self, m: Matrix, n1: f64, n2: f64) -> Self {
        let a = m.a;
        let b = m.b * n1;
        let c = m.c / n2;
        let d = m.d * n1 / n2;
        let num_re = a * self.re + b;
        let num_im = a * self.im;
        let den_re = c * self.re + d;
        let den_im = c * self.im;
        let norm = den_re * den_re + den_im * den_im;
        Self {
            re: (num_re * den_re + num_im * den_im) / norm,
            im: (num_im * den_re - num_re * den_im) / norm,
        }
    }
    fn transfer(self, t: f64) -> Self {
        Self {
            re: self.re + t,
            im: self.im,
        }
    }
    fn radius(self, n: f64, wavelength_mm: f64) -> f64 {
        let squared = self.re * self.re + self.im * self.im;
        (wavelength_mm * squared / (std::f64::consts::PI * n.abs() * self.im)).sqrt()
    }
    fn curvature(self) -> Option<f64> {
        if self.re.abs() < 1e-15 {
            None
        } else {
            Some((self.re * self.re + self.im * self.im) / self.re)
        }
    }
}
struct Beam {
    q: Q,
    index: f64,
    wavelength_mm: f64,
}
impl Beam {
    fn radius(&self) -> f64 {
        self.q.radius(self.index, self.wavelength_mm)
    }
    fn image(&self) -> ImageBeam {
        let q = self.q;
        let waist = (self.wavelength_mm * q.im / (std::f64::consts::PI * self.index.abs())).sqrt();
        ImageBeam {
            radius: self.radius(),
            curvature: q.curvature(),
            waist,
            waist_offset: -q.re,
            rayleigh_range: q.im,
            divergence: self.wavelength_mm / (std::f64::consts::PI * self.index.abs() * waist),
        }
    }
}
fn propagate(
    s: &System,
    w: f64,
    waist: f64,
    waist_z: f64,
) -> Result<(Vec<BeamSurface>, Beam), String> {
    if !waist.is_finite() || waist <= 0. {
        return Err("Input waist must be positive".into());
    }
    if !waist_z.is_finite() {
        return Err("Invalid waist position".into());
    }
    let wavelength_mm = w * 1e-6;
    let n0 = s.object_index(w)?;
    let q0 = Q {
        re: -waist_z,
        im: std::f64::consts::PI * waist * waist * n0 / wavelength_mm,
    };
    let mut table = vec![];
    for i in 0..s.surfaces.len() {
        // matrix(0, i + 1) ends just past refraction at surface i, on its vertex plane.
        let (m, n) = s.matrix(w, 0, i + 1)?;
        let beam = Beam {
            q: q0.transform(m, n0, n),
            index: n,
            wavelength_mm,
        };
        let z: f64 = s.surfaces.iter().take(i).map(|s| s.thickness).sum();
        table.push(BeamSurface {
            surface: i,
            z,
            radius: beam.radius(),
            curvature: beam.q.curvature(),
        });
    }
    let (m, n) = s.matrix(w, 0, s.surfaces.len())?;
    let last = s.surfaces.last().unwrap();
    let image = Beam {
        q: q0.transform(m, n0, n).transfer(last.thickness),
        index: n,
        wavelength_mm,
    };
    Ok((table, image))
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct InputWaist {
    waist: f64,
    waist_z: f64,
    rayleigh_range: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct BeamSurface {
    surface: usize,
    z: f64,
    radius: f64,
    curvature: Option<f64>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct ImageBeam {
    radius: f64,
    curvature: Option<f64>,
    waist: f64,
    waist_offset: f64,
    rayleigh_range: f64,
    divergence: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct GaussianBeam {
    wavelength: f64,
    input: InputWaist,
    surfaces: Vec<BeamSurface>,
    image: ImageBeam,
    units: &'static str,
    model: &'static str,
}

pub fn gaussian_beam(s: &System, w: f64, waist: f64, waist_z: f64) -> Result<Value, String> {
    serde_json::to_value(gaussian_beam_report(s, w, waist, waist_z)?).map_err(|e| e.to_string())
}
pub(crate) fn gaussian_beam_report(
    s: &System,
    w: f64,
    waist: f64,
    waist_z: f64,
) -> Result<GaussianBeam, String> {
    let (table, image) = propagate(s, w, waist, waist_z)?;
    Ok(GaussianBeam {
        wavelength: w,
        input: InputWaist {
            waist,
            waist_z,
            rayleigh_range: std::f64::consts::PI * waist * waist * s.object_index(w)? / (w * 1e-6),
        },
        surfaces: table,
        image: image.image(),
        units: "mm",
        model: "unclipped paraxial q-parameter propagation through the reduced system matrix; no aberration or aperture truncation",
    })
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct FiberCoupling {
    wavelength: f64,
    mode_field_diameter: f64,
    beam_radius: f64,
    wavefront_curvature: Option<f64>,
    efficiency: f64,
    optimal_mode_field_diameter: f64,
    optimal_efficiency: f64,
    units: &'static str,
    model: &'static str,
}

pub fn fiber_coupling(
    s: &System,
    w: f64,
    waist: f64,
    waist_z: f64,
    mode_field_diameter: f64,
) -> Result<Value, String> {
    serde_json::to_value(fiber_coupling_report(
        s,
        w,
        waist,
        waist_z,
        mode_field_diameter,
    )?)
    .map_err(|e| e.to_string())
}
pub(crate) fn fiber_coupling_report(
    s: &System,
    w: f64,
    waist: f64,
    waist_z: f64,
    mode_field_diameter: f64,
) -> Result<FiberCoupling, String> {
    if !mode_field_diameter.is_finite() || mode_field_diameter <= 0. {
        return Err("Mode field diameter must be positive".into());
    }
    let (_, image) = propagate(s, w, waist, waist_z)?;
    let wavelength_mm = w * 1e-6 / image.index.abs();
    let beam = image.radius();
    let fiber = mode_field_diameter / 2.;
    let phase = match image.q.curvature() {
        Some(r) => (std::f64::consts::PI * beam * fiber / (wavelength_mm * r)).powi(2),
        None => 0.,
    };
    let efficiency = 4. / ((beam / fiber + fiber / beam).powi(2) + phase);
    let optimal_fiber = beam
        / (1.
            + (std::f64::consts::PI * beam * beam
                / (wavelength_mm * image.q.curvature().unwrap_or(f64::INFINITY)))
            .powi(2))
        .sqrt()
        .sqrt();
    let optimal_phase = match image.q.curvature() {
        Some(r) => (std::f64::consts::PI * beam * optimal_fiber / (wavelength_mm * r)).powi(2),
        None => 0.,
    };
    let optimal_efficiency =
        4. / ((beam / optimal_fiber + optimal_fiber / beam).powi(2) + optimal_phase);
    Ok(FiberCoupling {
        wavelength: w,
        mode_field_diameter,
        beam_radius: beam,
        wavefront_curvature: image.q.curvature(),
        efficiency,
        optimal_mode_field_diameter: 2. * optimal_fiber,
        optimal_efficiency,
        units: "mm",
        model: "Gaussian overlap at the stored image plane; fiber mode is a flat-phase Gaussian; no offsets, tilt or aberration",
    })
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    #[test]
    fn free_space_matches_analytic_waist_growth() {
        let mut s = System::default();
        for surface in &mut s.surfaces {
            surface.radius = 0.;
            surface.material = "AIR".into();
        }
        s.surfaces[0].thickness = 10.;
        s.surfaces[1].thickness = 40.;
        s.update().unwrap();
        let w = s.reference();
        let waist = 0.2;
        let r = gaussian_beam(&s, w, waist, -15.).unwrap();
        let lambda = w * 1e-6;
        let n0 = s.object_index(w).unwrap();
        let z_r = std::f64::consts::PI * waist * waist * n0 / lambda;
        let analytic = |z: f64| waist * (1. + (z / z_r).powi(2)).sqrt();
        for row in r["surfaces"].as_array().unwrap() {
            let z = row["z"].as_f64().unwrap();
            let expected = analytic(z + 15.);
            let got = row["radius"].as_f64().unwrap();
            assert!(
                (got - expected).abs() / expected < 1e-9,
                "z={z}: {got} vs {expected}"
            );
        }
        assert!((r["image"]["radius"].as_f64().unwrap() - analytic(65.)).abs() < 1e-9);
        assert!((r["image"]["waist"].as_f64().unwrap() - waist).abs() < 1e-12);
        assert!((r["image"]["waist_offset"].as_f64().unwrap() + 65.).abs() < 1e-9);
    }
    #[test]
    fn single_surface_matches_analytic_q_law() {
        let mut s = System::default();
        s.surfaces.truncate(1);
        s.surfaces[0].radius = 50.;
        s.surfaces[0].material = "N-BK7_SCHOTT".into();
        s.surfaces[0].thickness = 30.;
        s.update().unwrap();
        let w = 587.5618;
        let n1 = s.object_index(w).unwrap();
        let n2 = s.material_index("N-BK7_SCHOTT", w).unwrap();
        let lambda = w * 1e-6;
        let waist = 0.5;
        let waist_z = -100.;
        let z_r = std::f64::consts::PI * waist * waist * n1 / lambda;
        let power = (n2 - n1) / 50.;
        // q2 = n2 q1/(1 - Phi q1) at the vertex, then free propagation in glass.
        let (a, b) = (-waist_z, z_r);
        let den_re = 1. - power * a;
        let den_im = -power * b;
        let norm = den_re * den_re + den_im * den_im;
        let q2_re = n2 * (a * den_re + b * den_im) / norm;
        let q2_im = n2 * (b * den_re - a * den_im) / norm;
        let q_img_re = q2_re + 30.;
        let squared = q_img_re * q_img_re + q2_im * q2_im;
        let expected_radius = (lambda * squared / (std::f64::consts::PI * n2 * q2_im)).sqrt();
        let r = gaussian_beam(&s, w, waist, waist_z).unwrap();
        let got = r["image"]["radius"].as_f64().unwrap();
        assert!((got - expected_radius).abs() / expected_radius < 1e-9);
        let waist_expected = (lambda * q2_im / (std::f64::consts::PI * n2)).sqrt();
        assert!((r["image"]["waist"].as_f64().unwrap() - waist_expected).abs() < 1e-12);
        assert!((r["image"]["waist_offset"].as_f64().unwrap() + q_img_re).abs() < 1e-9);
    }
    #[test]
    fn coupling_limits() {
        let mut s = System::default();
        let w = s.reference();
        // Move the stored image plane onto the Gaussian focus so the phase is flat.
        let focus = gaussian_beam(&s, w, 1., 0.).unwrap()["image"]["waist_offset"]
            .as_f64()
            .unwrap();
        s.surfaces.last_mut().unwrap().thickness += focus;
        s.update().unwrap();
        let beam = gaussian_beam(&s, w, 1., 0.).unwrap()["image"]["radius"]
            .as_f64()
            .unwrap();
        let matched = fiber_coupling(&s, w, 1., 0., 2. * beam).unwrap();
        assert!((matched["efficiency"].as_f64().unwrap() - 1.).abs() < 1e-9);
        let wide = fiber_coupling(&s, w, 1., 0., 8. * beam).unwrap();
        let expected = 4. / (0.25 + 4_f64).powi(2);
        assert!((wide["efficiency"].as_f64().unwrap() - expected).abs() < 1e-9);
        assert!(
            wide["efficiency"].as_f64().unwrap() < wide["optimal_efficiency"].as_f64().unwrap()
        );
    }
}
