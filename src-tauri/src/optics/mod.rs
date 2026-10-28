// SPDX-License-Identifier: GPL-3.0-or-later
// Optical algorithms translated from Geopter, Copyright (C) 2021 Hiiragi.
pub mod analysis;
pub mod aperture;
pub mod material;
pub mod math;
pub mod project;
pub mod sampling;
pub mod solve;
pub mod spec;
mod trace;
use math::*;
pub use sampling::{PupilSampling, SpectralComponent, Spectrum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub use spec::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub label: String,
    pub radius: f64,
    pub thickness: f64,
    pub material: String,
    pub aperture: f64,
    pub semi_diameter: f64,
    pub clear_aperture: bool,
    pub conic: f64,
    pub even: Vec<f64>,
    pub odd: Vec<f64>,
    pub interaction: Interaction,
    pub gap_solve: solve::GapSolve,
}
impl Surface {
    pub fn sphere(radius: f64, thickness: f64, material: &str, aperture: f64) -> Self {
        Self {
            label: String::new(),
            radius,
            thickness,
            material: material.into(),
            aperture,
            semi_diameter: 0.,
            clear_aperture: true,
            conic: 0.,
            even: vec![],
            odd: vec![],
            interaction: Interaction::Refract,
            gap_solve: solve::GapSolve::Fixed {},
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct System {
    pub name: String,
    pub note: String,
    pub surfaces: Vec<Surface>,
    pub wavelengths: Vec<Wavelength>,
    pub reference_wavelength: usize,
    pub fields: Vec<Field>,
    pub field_type: FieldType,
    pub pupil: Pupil,
    pub pupil_mapping: PupilMapping,
    pub object_distance: Option<f64>,
    pub stop_index: usize,
    pub temperature: f64,
    pub pressure: f64,
    pub custom_glasses: Vec<material::Glass>,
}
impl Default for System {
    fn default() -> Self {
        Self {
            name: "BK7 singlet".into(),
            note: String::new(),
            surfaces: vec![
                Surface::sphere(50., 5., "N-BK7_SCHOTT", 12.5),
                Surface::sphere(-50., 46.64, "AIR", 12.5),
            ],
            wavelengths: vec![
                Wavelength {
                    value: 486.1327,
                    weight: 1.,
                },
                Wavelength {
                    value: 587.5618,
                    weight: 1.,
                },
                Wavelength {
                    value: 656.2725,
                    weight: 1.,
                },
            ],
            reference_wavelength: 1,
            fields: vec![
                Field::meridional(0.),
                Field::meridional(3.),
                Field::meridional(5.),
            ],
            field_type: FieldType::Angle,
            pupil: Pupil {
                kind: PupilType::EntrancePupilDiameter,
                value: 10.,
            },
            pupil_mapping: PupilMapping::RealStop,
            object_distance: None,
            stop_index: 0,
            temperature: 20.,
            pressure: 101325.,
            custom_glasses: vec![],
        }
    }
}
pub fn index(name: &str, nm: f64) -> Result<f64, String> {
    material::refractive_index(name, nm, None)
}
pub fn sag(s: &Surface, r: f64) -> Result<(f64, f64), String> {
    if !r.is_finite() || r < 0. {
        return Err("Invalid radial height".into());
    }
    let c = if s.radius == 0. { 0. } else { 1. / s.radius };
    let q = 1. - (1. + s.conic) * c * c * r * r;
    if q <= 0. {
        return Err("Surface sag outside real domain".into());
    }
    let (mut z, mut dz) = (c * r * r / (1. + q.sqrt()), c * r / q.sqrt());
    for (i, a) in s.even.iter().enumerate() {
        let p = 4 + 2 * i as i32;
        z += a * r.powi(p);
        dz += a * p as f64 * r.powi(p - 1);
    }
    // Geopter's ODD profile includes every radial power beginning with r^3.
    for (i, a) in s.odd.iter().enumerate() {
        let p = 3 + i as i32;
        z += a * r.powi(p);
        dz += a * p as f64 * r.powi(p - 1);
    }
    if z.is_finite() && dz.is_finite() {
        Ok((z, dz))
    } else {
        Err("Nonfinite surface sag".into())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    pub point: V3,
    pub incoming: V3,
    pub direction: V3,
    pub normal: V3,
    pub n_before: f64,
    pub n_after: f64,
    pub opl: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Ray {
    pub points: Vec<V3>,
    pub segments: Vec<Segment>,
    pub opl: f64,
    pub wavelength: f64,
    pub field: f64,
    pub field_x: f64,
    pub direction: V3,
    pub entrance_opl: f64,
    pub launch_direction: V3,
}
impl Ray {
    pub fn image(&self) -> V3 {
        *self.points.last().unwrap()
    }
    pub fn last(&self) -> &Segment {
        self.segments.last().unwrap()
    }
}
impl System {
    pub fn field_frame(&self, field: &Field) -> FieldFrame {
        let direction = if self.field_type == FieldType::Angle {
            [field.x.to_radians().tan(), field.y.to_radians().tan()]
        } else {
            [field.x, field.y]
        };
        let length = direction[0].hypot(direction[1]);
        let tangential = if length > 1e-15 {
            [direction[0] / length, direction[1] / length]
        } else {
            [0., 1.]
        };
        FieldFrame {
            sagittal: [tangential[1], -tangential[0]],
            tangential,
        }
    }
    pub fn paraxial_chief_image(&self, field: &Field, w: f64) -> Result<[f64; 2], String> {
        let ep = self.pupil_matrix(w)?;
        if ep.a.abs() < 1e-15 {
            return Err("Entrance pupil lies at infinity".into());
        }
        let shift = ep.b / ep.a;
        let (matrix, n) = self.matrix(w, 0, self.surfaces.len())?;
        let image_distance = self.surfaces.last().ok_or("Empty prescription")?.thickness;
        let object = if self.object_distance.is_some() {
            Some(self.object_point(field, w)?)
        } else {
            None
        };
        let mut image = [0.; 2];
        for axis in 0..2 {
            let (height, slope) = if let Some(object) = object {
                let distance = self.object_distance.unwrap();
                if (distance + shift).abs() < 1e-12 {
                    return Err("Object lies at entrance pupil".into());
                }
                let slope = -object[axis] / (distance + shift);
                (object[axis] + distance * slope, slope)
            } else {
                let slope = (if axis == 0 { field.x } else { field.y })
                    .to_radians()
                    .tan();
                (-shift * slope, slope)
            };
            image[axis] = matrix.a * height
                + matrix.b * slope
                + image_distance / n * (matrix.c * height + matrix.d * slope);
        }
        if image.iter().all(|v| v.is_finite()) {
            Ok(image)
        } else {
            Err("Nonfinite paraxial chief image".into())
        }
    }
    pub fn reference(&self) -> f64 {
        self.wavelengths[self.reference_wavelength].value
    }
    pub fn material_index(&self, name: &str, w: f64) -> Result<f64, String> {
        let matches: Vec<_> = self
            .custom_glasses
            .iter()
            .filter(|g| g.key() == name || g.name == name)
            .collect();
        if matches.len() > 1 {
            return Err(format!("Ambiguous custom glass {name}"));
        }
        if let Some(g) = matches.first() {
            g.index(w, Some((self.temperature, self.pressure)))
        } else {
            material::refractive_index(name, w, Some((self.temperature, self.pressure)))
        }
    }
    pub fn update(&mut self) -> Result<(), String> {
        self.validate_parameters()?;
        let mut resolved = self.clone();
        solve::recalculate(&mut resolved)?;
        resolved.validate()?;
        resolved.update_semi_diameters()?;
        *self = resolved;
        Ok(())
    }
    pub(super) fn update_semi_diameters(&mut self) -> Result<(), String> {
        let sizes = aperture::semi_diameters(self)?;
        for (surface, size) in self.surfaces.iter_mut().zip(sizes.surfaces) {
            surface.semi_diameter = size.semi_diameter;
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        self.validate_parameters()?;
        self.pupil_radius(self.reference())?;
        Ok(())
    }
    fn validate_parameters(&self) -> Result<(), String> {
        if self.name.len() > 256
            || self.note.len() > 100_000
            || self.surfaces.is_empty()
            || self.surfaces.len() > 128
            || self.wavelengths.is_empty()
            || self.wavelengths.len() > 32
            || self.fields.is_empty()
            || self.fields.len() > 64
            || self.reference_wavelength >= self.wavelengths.len()
            || self.stop_index >= self.surfaces.len()
        {
            return Err("Invalid system dimensions or reference indices".into());
        }
        if !self.pupil.value.is_finite() || self.pupil.value <= 0. {
            return Err("Positive pupil specification required".into());
        }
        if let Some(d) = self.object_distance {
            if !d.is_finite() || d <= 0. {
                return Err("Object distance must be positive or null for infinity".into());
            }
        }
        if self.field_type != FieldType::Angle && self.object_distance.is_none() {
            return Err("Height fields require a finite object distance".into());
        }
        material::air_index(self.reference() / 1000., self.temperature, self.pressure)?;
        if self.custom_glasses.len() > 10_000 {
            return Err("Too many custom glasses".into());
        }
        for (i, g) in self.custom_glasses.iter().enumerate() {
            g.validate()?;
            if self.custom_glasses[..i].iter().any(|p| p.key() == g.key()) {
                return Err("Duplicate custom glass".into());
            }
        }
        for f in &self.fields {
            if ![f.x, f.y, f.weight].iter().all(|v| v.is_finite())
                || f.weight < 0.
                || f.vignetting.iter().any(|v| !v.is_finite() || *v >= 1.)
            {
                return Err("Invalid field or vignetting specification".into());
            }
            if self.field_type == FieldType::Angle && (f.x.abs() >= 80. || f.y.abs() >= 80.) {
                return Err("Field angle must lie within ±80 degrees".into());
            }
        }
        if self.fields.iter().all(|f| f.weight == 0.)
            || self.wavelengths.iter().all(|w| w.weight == 0.)
        {
            return Err("At least one positive field and wavelength weight required".into());
        }
        for w in &self.wavelengths {
            if !w.weight.is_finite()
                || w.weight < 0.
                || !w.value.is_finite()
                || !(100.0..=20000.).contains(&w.value)
            {
                return Err("Invalid wavelength specification".into());
            }
        }
        for (gap, s) in self.surfaces.iter().enumerate() {
            s.gap_solve.validate(self, gap)?;
            if s.label.len() > 256
                || ![s.radius, s.thickness, s.aperture, s.semi_diameter, s.conic]
                    .iter()
                    .all(|x| x.is_finite())
                || s.aperture <= 0.
                || s.semi_diameter < 0.
                || s.even.len() > 16
                || s.odd.len() > 20
                || !s.even.iter().chain(&s.odd).all(|x| x.is_finite())
            {
                return Err("Invalid surface parameters".into());
            }
            if s.clear_aperture {
                sag(s, s.aperture)?;
            }
            for w in &self.wavelengths {
                self.material_index(&s.material, w.value)?;
            }
        }
        Ok(())
    }
    pub fn matrix(&self, w: f64, start: usize, end: usize) -> Result<(Matrix, f64), String> {
        if start > end || end > self.surfaces.len() {
            return Err("Invalid matrix surface range".into());
        }
        let mut matrix = Matrix::identity();
        let mut n = if start == 0 {
            1.
        } else {
            self.material_index(&self.surfaces[start - 1].material, w)?
        };
        for i in start..end {
            let s = &self.surfaces[i];
            let next = self.material_index(&s.material, w)?;
            match s.interaction {
                Interaction::Refract => matrix.refract(if s.radius == 0. {
                    0.
                } else {
                    (next - n) / s.radius
                }),
                Interaction::Reflect => {
                    return Err("Paraxial analysis of folded systems is not implemented".into());
                }
                Interaction::Dummy => {}
            }
            n = if s.interaction == Interaction::Dummy {
                n
            } else {
                next
            };
            if i + 1 < end {
                matrix.transfer(s.thickness, n)
            }
        }
        Ok((matrix, n))
    }
    pub fn pupil_matrix(&self, w: f64) -> Result<Matrix, String> {
        if self.stop_index == 0 {
            return Ok(Matrix::identity());
        }
        let (mut m, n) = self.matrix(w, 0, self.stop_index)?;
        m.transfer(self.surfaces[self.stop_index - 1].thickness, n);
        Ok(m)
    }
    fn specified_pupil_radius(&self, w: f64) -> Result<f64, String> {
        let (m, n) = self.matrix(w, 0, self.surfaces.len())?;
        let r = match self.pupil.kind {
            PupilType::EntrancePupilDiameter => self.pupil.value / 2.,
            PupilType::FNumber => {
                let slope = if let Some(t) = self.object_distance {
                    let ep = self.pupil_matrix(w)?;
                    (m.c * t + m.d) / (t + ep.b / ep.a)
                } else {
                    m.c
                };
                if slope.abs() < 1e-15 {
                    return Err("F-number pupil undefined for afocal system".into());
                }
                1. / (2. * self.pupil.value * slope.abs())
            }
            PupilType::ObjectNa => {
                let d = self
                    .object_distance
                    .ok_or("Object NA requires finite conjugates")?;
                if self.pupil.value >= 1. {
                    return Err("Object NA must be smaller than 1".into());
                }
                let ep = self.pupil_matrix(w)?;
                if ep.a.abs() < 1e-15 {
                    return Err("Entrance pupil lies at infinity".into());
                }
                (d + ep.b / ep.a).abs() * self.pupil.value / (1. - self.pupil.value.powi(2)).sqrt()
            }
            PupilType::ImageNa => {
                if self.pupil.value >= n {
                    return Err("Image NA must be smaller than image medium index".into());
                }
                if m.c.abs() < 1e-15 {
                    return Err("Image NA undefined for afocal system".into());
                }
                let slope = if let Some(t) = self.object_distance {
                    (m.c * t + m.d) / (t + self.pupil_matrix(w)?.b / self.pupil_matrix(w)?.a)
                } else {
                    m.c
                };
                n * self.pupil.value / ((n * n - self.pupil.value.powi(2)).sqrt() * slope.abs())
            }
        };
        if r.is_finite() && r > 0. {
            Ok(r)
        } else {
            Err("Invalid computed pupil radius".into())
        }
    }
    pub fn pupil_radius(&self, w: f64) -> Result<f64, String> {
        let reference = self.reference();
        let r = self.specified_pupil_radius(reference)?;
        if w == reference {
            return Ok(r);
        }
        let ep_ref = self.pupil_matrix(reference)?;
        let ep = self.pupil_matrix(w)?;
        if ep.a.abs() < 1e-15 {
            return Err("Entrance pupil lies at infinity".into());
        }
        Ok(r * (ep_ref.a / ep.a).abs())
    }
    pub fn paraxial(&self, w: f64) -> Result<Value, String> {
        let (m, n) = self.matrix(w, 0, self.surfaces.len())?;
        if m.c.abs() < 1e-14 {
            return Err("Afocal system: focal lengths undefined".into());
        }
        let efl = -n / m.c;
        let bfl = -n * m.a / m.c;
        let ep = self.pupil_matrix(w)?;
        if ep.a.abs() < 1e-15 {
            return Err("Entrance pupil lies at infinity".into());
        }
        let image_distance = if let Some(t) = self.object_distance {
            let den = m.c * t + m.d;
            if den.abs() < 1e-15 {
                return Err("Image conjugate lies at infinity".into());
            }
            -n * (m.a * t + m.b) / den
        } else {
            bfl
        };
        let magnification = self.object_distance.map(|t| 1. / (m.c * t + m.d));
        let r = self.pupil_radius(w)?;
        let q_gain = if let Some(t) = self.object_distance {
            (m.c * t + m.d) / (t + ep.b / ep.a)
        } else {
            m.c
        };
        // Image of the aperture stop in image space, measured from the last vertex.
        let (post, ni) = self.matrix(w, self.stop_index, self.surfaces.len())?;
        let exit_distance = if post.d.abs() > 1e-15 {
            Some(-ni * post.b / post.d)
        } else {
            None
        };
        let stop_radius = ep.a.abs() * r;
        let exit_radius = if post.d.abs() > 1e-15 {
            Some(stop_radius / post.d.abs())
        } else {
            None
        };
        Ok(
            json!({"efl":efl,"bfl":bfl,"f_number":1./(2.*(q_gain*r).abs()),"matrix":[m.a,m.b,m.c,m.d],"image_distance":image_distance,"magnification":magnification,"entrance_pupil_distance":ep.b/ep.a,"entrance_pupil_radius":r,"stop_radius":stop_radius,"exit_pupil_distance":exit_distance,"exit_pupil_radius":exit_radius,"image_index":n}),
        )
    }
    fn object_point(&self, field: &Field, _w: f64) -> Result<V3, String> {
        let d = self.object_distance.ok_or("No finite object")?;
        let (x, y) = match self.field_type {
            FieldType::Angle => {
                let ep = self.pupil_matrix(self.reference())?;
                if ep.a.abs() < 1e-15 {
                    return Err("Entrance pupil lies at infinity".into());
                }
                let l = d + ep.b / ep.a;
                (
                    -l * field.x.to_radians().tan(),
                    -l * field.y.to_radians().tan(),
                )
            }
            FieldType::ObjectHeight => (field.x, field.y),
            FieldType::ImageHeight => {
                let p = self.paraxial(self.reference())?;
                let m = p["magnification"]
                    .as_f64()
                    .ok_or("Undefined magnification")?;
                if m.abs() < 1e-15 {
                    return Err("Zero magnification".into());
                }
                (field.x / m, field.y / m)
            }
        };
        Ok([x, y, -d])
    }
    fn launch(&self, first: [f64; 2], field: &Field, w: f64) -> Result<(V3, V3, f64), String> {
        let radial = first[0].hypot(first[1]);
        let sag_at_vertex = sag(&self.surfaces[0], radial)?.0;
        let plane = sag_at_vertex.min(0.) - radial.clamp(1., 100.) * 2.;
        if let Some(d) = self.object_distance {
            let obj = self.object_point(field, w)?;
            let direction = unit(sub([first[0], first[1], 0.], obj))?;
            // Stay near the first surface for distant objects and never launch behind a finite object.
            let z = plane.max(-d);
            let distance = (z + d) / direction[2];
            let p = [
                first[0] + z * direction[0] / direction[2],
                first[1] + z * direction[1] / direction[2],
                z,
            ];
            Ok((p, direction, distance))
        } else {
            let v = unit([field.x.to_radians().tan(), field.y.to_radians().tan(), 1.])?;
            let p = [
                first[0] + plane * v[0] / v[2],
                first[1] + plane * v[1] / v[2],
                plane,
            ];
            Ok((p, v, dot(p, v)))
        }
    }
    pub fn trace(&self, x: f64, y: f64, field: f64, w: f64) -> Result<Ray, String> {
        self.trace_field(x, y, &Field::meridional(field), w)
    }
    pub fn spot_field(
        &self,
        field: &Field,
        w: f64,
        sampling: PupilSampling,
    ) -> Result<Value, String> {
        self.sample_pupil(field, w, sampling)?.json()
    }
    pub fn layout(&self) -> Value {
        let mut rays = vec![];
        let mut rejected = vec![];
        let w = self.reference();
        let radius = self.pupil_radius(w).unwrap_or(1.);
        for (fi, f) in self.fields.iter().enumerate() {
            for i in -3..=3 {
                match self.trace_field(0., radius * i as f64 / 3., f, w) {
                    Ok(r) => rays.push(r),
                    Err(e) => rejected.push(json!({"field":fi,"pupil":i as f64/3.,"error":e})),
                }
            }
        }
        json!({"rays":rays,"rejected":rejected})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bk7_index() {
        assert!((index("N-BK7", 587.5618).unwrap() - 1.5168).abs() < 1e-5)
    }
    #[test]
    fn singlet_focal_length() {
        let s = System::default();
        let p = s.paraxial(s.reference()).unwrap();
        assert!((p["efl"].as_f64().unwrap() - 49.213).abs() < 0.02);
        assert!((p["bfl"].as_f64().unwrap() - 47.537).abs() < 0.03);
    }
    #[test]
    fn axis_and_symmetry() {
        let s = System::default();
        let a = s.trace(0., 2., 0., s.reference()).unwrap();
        let b = s.trace(0., -2., 0., s.reference()).unwrap();
        assert!((a.image()[1] + b.image()[1]).abs() < 1e-10);
        assert_eq!(s.trace(0., 0., 0., s.reference()).unwrap().image()[1], 0.);
    }
    #[test]
    fn finite_conjugate_focus() {
        let mut s = System::default();
        s.object_distance = Some(200.);
        let p = s.paraxial(s.reference()).unwrap();
        let z = p["image_distance"].as_f64().unwrap();
        s.surfaces.last_mut().unwrap().thickness = z;
        let r = s.trace(0., 0.0001, 0., s.reference()).unwrap();
        assert!(r.image()[1].abs() < 1e-9);
    }
    #[test]
    fn chief_ray_aims_at_internal_stop() {
        let mut s = System::default();
        s.stop_index = 1;
        let r = s.trace(0., 0., 5., s.reference()).unwrap();
        assert!(r.segments[1].point[1].abs() < 1e-9);
    }
    #[test]
    fn rejects_invalid_system() {
        let mut s = System::default();
        s.surfaces[0].material = "fake".into();
        assert!(s.validate().is_err());
    }
    #[test]
    fn spot_is_finite() {
        let s = System::default();
        assert!(
            s.spot_field(
                &Field::meridional(0.),
                s.reference(),
                PupilSampling::Grid { count: 17 }
            )
            .unwrap()["rms_radius"]
                .as_f64()
                .unwrap()
                > 0.
        );
    }
    #[test]
    fn radial_derivative_matches_finite_difference() {
        let mut s = Surface::sphere(30., 2., "AIR", 8.);
        s.odd = vec![1e-4, -2e-6];
        s.even = vec![1e-6];
        let r = 3.;
        let eps = 1e-5;
        let analytic = sag(&s, r).unwrap().1;
        let numeric = (sag(&s, r + eps).unwrap().0 - sag(&s, r - eps).unwrap().0) / (2. * eps);
        assert!((analytic - numeric).abs() < 1e-9);
    }
}
