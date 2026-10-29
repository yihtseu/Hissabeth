// SPDX-License-Identifier: GPL-3.0-or-later
// Optical algorithms translated from Geopter, Copyright (C) 2021 Hiiragi.
pub mod analysis;
pub mod aperture;
pub mod inspection;
pub mod material;
pub mod math;
pub mod profile;
pub mod project;
#[cfg(test)]
mod reflection;
pub mod sampling;
pub mod solve;
pub mod spec;
mod trace;
use math::*;
pub use profile::{EndpointSurface, Profile};
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
    pub object_material: String,
    pub object_label: String,
    pub image_label: String,
    pub object_surface: EndpointSurface,
    pub image_surface: EndpointSurface,
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
                Wavelength::new(486.1327, 1.),
                Wavelength::new(587.5618, 1.),
                Wavelength::new(656.2725, 1.),
            ],
            reference_wavelength: 1,
            fields: vec![
                Field {
                    color: [0.78, 0.416, 0.239, 1.],
                    ..Field::meridional(0.)
                },
                Field {
                    color: [0.196, 0.486, 0.623, 1.],
                    ..Field::meridional(3.)
                },
                Field {
                    color: [0.553, 0.439, 0.702, 1.],
                    ..Field::meridional(5.)
                },
            ],
            field_type: FieldType::Angle,
            pupil: Pupil {
                kind: PupilType::EntrancePupilDiameter,
                value: 10.,
            },
            pupil_mapping: PupilMapping::RealStop,
            object_distance: None,
            object_material: "AIR".into(),
            object_label: "Object".into(),
            image_label: "Image".into(),
            object_surface: EndpointSurface::plane(),
            image_surface: EndpointSurface::plane(),
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
    s.profile().sag(r)
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
    pub object_point: Option<V3>,
    pub image_hit: Option<Segment>,
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
        let shift = self.entrance_pupil_distance(w)?;
        let (matrix, n) = self.matrix(w, 0, self.surfaces.len())?;
        let n0 = self.object_index(w)?;
        let image_distance = self.surfaces.last().ok_or("Empty prescription")?.thickness;
        let object = if self.object_distance.is_some() {
            Some(self.object_point(field, w)?)
        } else {
            None
        };
        let mut image = [0.; 2];
        for axis in 0..2 {
            let (height, slope) = if let Some(object) = object {
                let distance = -object[2];
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
                + matrix.b * n0 * slope
                + image_distance / n * (matrix.c * height + matrix.d * n0 * slope);
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
    pub fn object_index(&self, w: f64) -> Result<f64, String> {
        self.material_index(&self.object_material, w)
    }
    pub fn entrance_pupil_distance(&self, w: f64) -> Result<f64, String> {
        let ep = self.pupil_matrix(w)?;
        if ep.a.abs() < 1e-15 {
            return Err("Entrance pupil lies at infinity".into());
        }
        Ok(self.object_index(w)? * ep.b / ep.a)
    }
    fn paraxial_index_before(&self, surface: usize, w: f64) -> Result<f64, String> {
        let index = match self.surfaces[..surface]
            .iter()
            .rev()
            .find(|s| s.interaction == Interaction::Refract)
        {
            Some(s) => self.material_index(&s.material, w),
            None => self.object_index(w),
        }?;
        let reflections = self.surfaces[..surface]
            .iter()
            .filter(|s| s.interaction == Interaction::Reflect)
            .count();
        Ok(if reflections % 2 == 0 { index } else { -index })
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
        self.object_surface.semi_diameter = sizes.object.semi_diameter;
        self.image_surface.semi_diameter = sizes.image.semi_diameter;
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
            || self.object_label.len() > 256
            || self.image_label.len() > 256
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
        self.object_surface
            .validate()
            .map_err(|e| format!("Object surface: {e}"))?;
        self.image_surface
            .validate()
            .map_err(|e| format!("Image surface: {e}"))?;
        if self.object_distance.is_none()
            && (!self.object_surface.profile().is_plane() || self.object_surface.clear_aperture)
        {
            return Err(
                "A curved or clipped object surface requires a finite object distance".into(),
            );
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
                || !valid_color(f.color)
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
                || !valid_color(w.color)
                || !w.value.is_finite()
                || !(100.0..=20000.).contains(&w.value)
            {
                return Err("Invalid wavelength specification".into());
            }
            self.object_index(w.value)?;
        }
        for (gap, s) in self.surfaces.iter().enumerate() {
            s.gap_solve.validate(self, gap)?;
            if s.label.len() > 256
                || ![s.radius, s.thickness, s.aperture, s.semi_diameter, s.conic]
                    .iter()
                    .all(|x| x.is_finite())
                || s.aperture <= 0.
                || s.semi_diameter < 0.
            {
                return Err("Invalid surface parameters".into());
            }
            s.profile().validate()?;
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
        // The reduced ray is [height, direction_sign * physical_index * dy/dz].
        // Real backward travel has negative dz and negative paraxial index.
        let mut n = self.paraxial_index_before(start, w)?;
        for i in start..end {
            let s = &self.surfaces[i];
            let (next, power) = self.paraxial_step(s, n, w)?;
            matrix.refract(power);
            n = next;
            if i + 1 < end {
                matrix.transfer(s.thickness, n)
            }
        }
        Ok((matrix, n))
    }
    fn paraxial_step(&self, surface: &Surface, n: f64, w: f64) -> Result<(f64, f64), String> {
        let next = match surface.interaction {
            Interaction::Refract => n.signum() * self.material_index(&surface.material, w)?,
            Interaction::Reflect => -n,
            Interaction::Dummy => n,
        };
        Ok((
            next,
            if surface.radius == 0. {
                0.
            } else {
                (next - n) / surface.radius
            },
        ))
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
        let n0 = self.object_index(w)?;
        let shift = self.entrance_pupil_distance(w)?;
        let q_gain = if let Some(t) = self.object_distance {
            (m.c * t + m.d * n0) / (t + shift)
        } else {
            m.c
        };
        let r = match self.pupil.kind {
            PupilType::EntrancePupilDiameter => self.pupil.value / 2.,
            PupilType::FNumber => {
                let slope = q_gain;
                if slope.abs() < 1e-15 {
                    return Err("F-number pupil undefined for afocal system".into());
                }
                1. / (2. * self.pupil.value * slope.abs())
            }
            PupilType::ObjectNa => {
                let d = self
                    .object_distance
                    .ok_or("Object NA requires finite conjugates")?;
                if self.pupil.value >= n0 {
                    return Err("Object NA must be smaller than object medium index".into());
                }
                let ep = self.pupil_matrix(w)?;
                if ep.a.abs() < 1e-15 {
                    return Err("Entrance pupil lies at infinity".into());
                }
                (d + shift).abs() * self.pupil.value / (n0 * n0 - self.pupil.value.powi(2)).sqrt()
            }
            PupilType::ImageNa => {
                if self.pupil.value >= n.abs() {
                    return Err("Image NA must be smaller than image medium index".into());
                }
                if q_gain.abs() < 1e-15 {
                    return Err("Image NA undefined for afocal system".into());
                }
                let slope = q_gain;
                n.abs() * self.pupil.value
                    / ((n * n - self.pupil.value.powi(2)).sqrt() * slope.abs())
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
        let n0 = self.object_index(w)?;
        let entrance_distance = self.entrance_pupil_distance(w)?;
        let powered = m.c.abs() >= 1e-14;
        // EFL is measured along image-space propagation; axial focus distances
        // retain the global z sign used by stored thicknesses.
        let efl = powered.then(|| -n.abs() / m.c);
        let bfl = powered.then(|| -n * m.a / m.c);
        let ep = self.pupil_matrix(w)?;
        if ep.a.abs() < 1e-15 {
            return Err("Entrance pupil lies at infinity".into());
        }
        let image_distance = if let Some(t) = self.object_distance {
            let den = m.c * t / n0 + m.d;
            if den.abs() < 1e-15 {
                return Err("Image conjugate lies at infinity".into());
            }
            -n * (m.a * t / n0 + m.b) / den
        } else {
            bfl.ok_or("Afocal system: infinite object has no finite image conjugate")?
        };
        if !image_distance.is_finite() {
            return Err("Nonfinite paraxial image distance".into());
        }
        let magnification = self.object_distance.map(|t| 1. / (m.c * t / n0 + m.d));
        let r = self.pupil_radius(w)?;
        let q_gain = if let Some(t) = self.object_distance {
            (m.c * t + m.d * n0) / (t + entrance_distance)
        } else {
            m.c
        };
        let object_na = self
            .object_distance
            .map(|t| (n0 * (r / (t + entrance_distance)).atan().sin()).abs());
        let image_na = (n * (q_gain * r / n).atan().sin()).abs();
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
            json!({"efl":efl,"bfl":bfl,"f_number":1./(2.*(q_gain*r).abs()),"matrix":[m.a,m.b,m.c,m.d],"matrix_basis":"height, direction_sign * physical_index * dy/dz","image_direction":n.signum(),"image_distance":image_distance,"magnification":magnification,"entrance_pupil_distance":entrance_distance,"entrance_pupil_radius":r,"stop_radius":stop_radius,"exit_pupil_distance":exit_distance,"exit_pupil_radius":exit_radius,"image_index":n.abs(),"object_index":n0,"object_na":object_na,"image_na":image_na,"front_focal_distance":powered.then(||n0*m.d/m.c)}),
        )
    }
    fn object_point(&self, field: &Field, _w: f64) -> Result<V3, String> {
        let d = self.object_distance.ok_or("No finite object")?;
        if self.field_type == FieldType::Angle {
            let shift = self.entrance_pupil_distance(self.reference())?;
            let direction = unit([field.x.to_radians().tan(), field.y.to_radians().tan(), 1.])?;
            return self
                .object_surface
                .profile()
                .intersect([0., 0., shift], direction, -d)
                .map(|hit| hit.point)
                .map_err(|e| format!("Object surface: {e}"));
        }
        let (x, y) = match self.field_type {
            FieldType::ObjectHeight => (field.x, field.y),
            FieldType::ImageHeight => {
                let p = self.paraxial(self.reference())?;
                let magnification = p["magnification"]
                    .as_f64()
                    .ok_or("Undefined magnification")?;
                if magnification.abs() < 1e-15 {
                    return Err("Zero magnification".into());
                }
                (field.x / magnification, field.y / magnification)
            }
            FieldType::Angle => unreachable!(),
        };
        let z = self
            .object_surface
            .sag(x.hypot(y))
            .map_err(|e| format!("Object surface: {e}"))?
            .0;
        Ok([x, y, -d + z])
    }
    fn launch(&self, first: [f64; 2], field: &Field, w: f64) -> Result<trace::Launch, String> {
        let radial = first[0].hypot(first[1]);
        let sag_at_vertex = sag(&self.surfaces[0], radial)?.0;
        let plane = sag_at_vertex.min(0.) - radial.clamp(1., 100.) * 2.;
        if self.object_distance.is_some() {
            let obj = self.object_point(field, w)?;
            let object_distance = -obj[2];
            if object_distance.abs() < 1e-12 {
                return Err("Object point lies in the first vertex plane".into());
            }
            let direction = unit([
                (first[0] - obj[0]) / object_distance,
                (first[1] - obj[1]) / object_distance,
                1.,
            ])?;
            let z = plane.max(obj[2]);
            let distance = (z - obj[2]) / direction[2];
            let p = [
                first[0] + z * direction[0] / direction[2],
                first[1] + z * direction[1] / direction[2],
                z,
            ];
            Ok(trace::Launch {
                point: p,
                direction,
                entrance_opl: distance * self.object_index(w)?,
                object_point: Some(obj),
            })
        } else {
            let v = unit([field.x.to_radians().tan(), field.y.to_radians().tan(), 1.])?;
            let p = [
                first[0] + plane * v[0] / v[2],
                first[1] + plane * v[1] / v[2],
                plane,
            ];
            Ok(trace::Launch {
                point: p,
                direction: v,
                entrance_opl: dot(p, v) * self.object_index(w)?,
                object_point: None,
            })
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
        let mut field_indices = vec![];
        let w = self.reference();
        let radius = self.pupil_radius(w).unwrap_or(1.);
        for (fi, f) in self.fields.iter().enumerate() {
            for i in -3..=3 {
                match self.trace_field(0., radius * i as f64 / 3., f, w) {
                    Ok(r) => {
                        rays.push(r);
                        field_indices.push(fi);
                    }
                    Err(e) => rejected.push(json!({"field":fi,"pupil":i as f64/3.,"error":e})),
                }
            }
        }
        json!({"rays":rays,"field_indices":field_indices,"rejected":rejected})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn immersed_interface() -> System {
        let mut s = System::default();
        s.object_material = "1.500000:60.000000".into();
        s.wavelengths = vec![Wavelength::new(587.562, 1.)];
        s.reference_wavelength = 0;
        s.surfaces = vec![Surface::sphere(-50., 200., "AIR", 20.)];
        s.object_distance = Some(300.);
        s.fields = vec![Field::meridional(0.)];
        s.pupil.value = 0.0002;
        s.update().unwrap();
        s
    }
    #[test]
    fn overflowing_finite_object_conjugate_returns_an_error() {
        let mut s = System::default();
        s.object_distance = Some(1e308);
        s.surfaces = vec![
            Surface::sphere(1., 100., "1.500000:60.000000", 0.1),
            Surface::sphere(-1., 10., "AIR", 0.1),
        ];
        s.wavelengths = vec![Wavelength::new(587.562, 1.)];
        s.reference_wavelength = 0;
        assert!(s.paraxial(s.reference()).unwrap_err().contains("Nonfinite"));
        assert!(analysis::longitudinal(&s, s.reference(), 5).is_err());
        assert!(analysis::chromatic_focus(&s, 500., 650., 3).is_err());
    }
    #[test]
    fn immersed_object_conjugates_match_spherical_interface_equation() {
        let s = immersed_interface();
        let p = s.paraxial(s.reference()).unwrap();
        // n0/s + n1/s' = (n1-n0)/R = 0.01 for this interface.
        for (key, expected) in [
            ("object_index", 1.5),
            ("image_index", 1.),
            ("efl", 100.),
            ("image_distance", 200.),
            ("magnification", -1.),
            ("front_focal_distance", -150.),
        ] {
            assert!(
                (p[key].as_f64().unwrap() - expected).abs() < 1e-10,
                "{key}: {}",
                p[key]
            );
        }
        let ray = s.trace(0., 0.0001, 0., s.reference()).unwrap();
        assert!((ray.last().n_before - 1.5).abs() < 1e-14);
        assert!(ray.image()[1].abs() < 1e-9);
        let focus = ray.last().point[2] - ray.last().point[1] * ray.direction[2] / ray.direction[1];
        assert!((focus - 200.).abs() < 1e-4);
    }
    #[test]
    fn immersed_object_autofocus_and_marginal_solve_match_gaussian_conjugate() {
        let mut s = immersed_interface();
        s.surfaces[0].thickness = 123.;
        let focused = solve::autofocus(&s, s.reference()).unwrap();
        assert!((focused.surfaces[0].thickness - 200.).abs() < 1e-8);
        for zone in [0., 0.5] {
            let (solved, _) =
                solve::apply(&s, 0, solve::GapSolve::MarginalHeight { value: 0., zone }).unwrap();
            assert!((solved.surfaces[0].thickness - 200.).abs() < 1e-4);
        }
    }
    #[test]
    fn numerical_apertures_and_working_f_number_use_both_media() {
        let mut s = immersed_interface();
        s.object_distance = Some(10.);
        s.pupil = Pupil {
            kind: PupilType::ObjectNa,
            value: 1.2,
        };
        let r = s.pupil_radius(s.reference()).unwrap();
        assert!((r - 10. * 1.2 / (1.5_f64.powi(2) - 1.2_f64.powi(2)).sqrt()).abs() < 1e-12);
        assert!(
            (s.paraxial(s.reference()).unwrap()["object_na"]
                .as_f64()
                .unwrap()
                - 1.2)
                .abs()
                < 1e-12
        );
        s.pupil.value = 1.5;
        assert!(
            s.pupil_radius(s.reference())
                .unwrap_err()
                .contains("object medium index")
        );
        s.pupil = Pupil {
            kind: PupilType::ImageNa,
            value: 0.3,
        };
        assert!(
            (s.paraxial(s.reference()).unwrap()["image_na"]
                .as_f64()
                .unwrap()
                - 0.3)
                .abs()
                < 1e-12
        );
        s.pupil = Pupil {
            kind: PupilType::FNumber,
            value: 5.,
        };
        assert!(
            (s.paraxial(s.reference()).unwrap()["f_number"]
                .as_f64()
                .unwrap()
                - 5.)
                .abs()
                < 1e-12
        );
    }
    #[test]
    fn object_medium_survives_dummy_surfaces_and_sets_geometric_pupil_distance() {
        let mut s = immersed_interface();
        let mut dummy = Surface::sphere(0., 10., "AIR", 20.);
        dummy.interaction = Interaction::Dummy;
        s.surfaces = vec![dummy, Surface::sphere(0., 10., "AIR", 20.)];
        s.stop_index = 1;
        s.object_distance = Some(100.);
        s.field_type = FieldType::ObjectHeight;
        s.fields = vec![Field::meridional(2.)];
        s.pupil = Pupil {
            kind: PupilType::EntrancePupilDiameter,
            value: 2.,
        };
        s.update().unwrap();
        assert!((s.pupil_matrix(s.reference()).unwrap().b - 10. / 1.5).abs() < 1e-12);
        assert!((s.entrance_pupil_distance(s.reference()).unwrap() - 10.).abs() < 1e-12);
        assert!((s.matrix(s.reference(), 1, 1).unwrap().1 - 1.5).abs() < 1e-12);
        let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
        assert!(ray.segments[1].point[1].abs() < 1e-9);
        assert!((ray.segments[0].n_after - 1.5).abs() < 1e-12);
        assert!((ray.segments[1].n_before - 1.5).abs() < 1e-12);
        let expected_path = 1.5 * 110_f64.hypot(2.);
        assert!((ray.last().opl + ray.entrance_opl - expected_path).abs() < 1e-9);
    }
    #[test]
    fn plane_interface_refraction_and_paraxial_chief_use_object_index() {
        let mut s = immersed_interface();
        s.surfaces = vec![Surface::sphere(0., 10., "AIR", 20.)];
        s.object_distance = Some(200.);
        s.field_type = FieldType::ObjectHeight;
        s.fields = vec![Field::meridional(1.)];
        s.update().unwrap();
        let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
        let incoming_sine = -1. / 200_f64.hypot(1.);
        let outgoing_sine = 1.5 * incoming_sine;
        let expected = 10. * outgoing_sine / (1. - outgoing_sine * outgoing_sine).sqrt();
        assert!((ray.image()[1] - expected).abs() < 1e-11);
        assert!(
            (s.paraxial_chief_image(&s.fields[0], s.reference()).unwrap()[1] + 0.075).abs() < 1e-13
        );
        s.object_distance = None;
        s.field_type = FieldType::Angle;
        s.fields[0].y = 3.;
        s.update().unwrap();
        let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
        assert!((ray.direction[1] - 1.5 * 3_f64.to_radians().sin()).abs() < 1e-12);
    }
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
