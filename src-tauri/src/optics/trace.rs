// SPDX-License-Identifier: GPL-3.0-or-later
// Sequential intersections, refraction, and stop aiming share one tracing path.
use super::*;
use std::fmt;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FailureKind {
    Invalid,
    Intersection,
    Blocked,
    TotalInternalReflection,
    Aiming,
    Image,
}
impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::Intersection => "intersection",
            Self::Blocked => "blocked",
            Self::TotalInternalReflection => "total_internal_reflection",
            Self::Aiming => "aiming",
            Self::Image => "image",
        }
    }
}
#[derive(Debug)]
pub(super) struct TraceFailure {
    pub kind: FailureKind,
    pub surface: Option<usize>,
    pub point: Option<V3>,
    pub ray: Option<Ray>,
    pub message: String,
}
impl TraceFailure {
    fn launch(message: String) -> Self {
        Self {
            kind: FailureKind::Aiming,
            surface: None,
            point: None,
            ray: None,
            message,
        }
    }
    fn path(
        kind: FailureKind,
        surface: Option<usize>,
        point: Option<V3>,
        ray: Ray,
        message: String,
    ) -> Self {
        Self {
            kind,
            surface,
            point,
            ray: Some(ray),
            message,
        }
    }
}
impl fmt::Display for TraceFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
#[derive(Clone, Copy)]
pub(super) struct TraceOptions {
    pub last: usize,
    pub image: bool,
    pub apertures: bool,
    pub vignetting: bool,
}

impl System {
    fn trace_path(
        &self,
        p: V3,
        v: V3,
        w: f64,
        field: &Field,
        entrance_opl: f64,
        last: usize,
        check_apertures: bool,
        image: bool,
    ) -> Result<Ray, TraceFailure> {
        let mut ray = Ray {
            points: vec![p],
            segments: vec![],
            opl: 0.,
            wavelength: w,
            field: field.y,
            field_x: field.x,
            direction: v,
            entrance_opl,
            launch_direction: v,
        };
        let (mut p, mut v, mut vertex, mut n) = (p, v, 0., 1.);
        for (i, s) in self.surfaces.iter().enumerate().take(last + 1) {
            if v[2].abs() < 1e-14 {
                return Err(TraceFailure::path(
                    FailureKind::Intersection,
                    Some(i),
                    None,
                    ray,
                    format!("Ray parallel to surface {i}"),
                ));
            }
            let intersection = (|| -> Result<(V3, f64, f64), String> {
                let mut t = (vertex - p[2]) / v[2];
                for _ in 0..50 {
                    let hit = add(p, scale(v, t));
                    let r = hit[0].hypot(hit[1]);
                    let (z, slope) = sag(s, r).map_err(|e| format!("Surface {i}: {e}"))?;
                    let error = hit[2] - vertex - z;
                    if error.abs() < 1e-10 {
                        return Ok((hit, slope, t));
                    }
                    let grad = if r > 1e-15 {
                        slope * (hit[0] * v[0] + hit[1] * v[1]) / r
                    } else {
                        0.
                    };
                    let den = v[2] - grad;
                    if den.abs() < 1e-14 || error.abs() < 1e-10 {
                        break;
                    }
                    t -= error / den;
                }
                Err(format!("Intersection failed at surface {i}"))
            })();
            let (hit, slope, t) = match intersection {
                Ok(v) => v,
                Err(e) => {
                    return Err(TraceFailure::path(
                        FailureKind::Intersection,
                        Some(i),
                        None,
                        ray,
                        e,
                    ));
                }
            };
            if check_apertures && s.clear_aperture && hit[0].hypot(hit[1]) > s.aperture + 1e-9 {
                return Err(TraceFailure::path(
                    FailureKind::Blocked,
                    Some(i),
                    Some(hit),
                    ray,
                    format!("Ray blocked at surface {i}"),
                ));
            }
            ray.opl += t * n;
            ray.points.push(hit);
            let r = hit[0].hypot(hit[1]);
            let normal = match unit(if r > 1e-15 {
                [-slope * hit[0] / r, -slope * hit[1] / r, 1.]
            } else {
                [0., 0., 1.]
            }) {
                Ok(v) => v,
                Err(e) => {
                    return Err(TraceFailure::path(
                        FailureKind::Intersection,
                        Some(i),
                        Some(hit),
                        ray,
                        e,
                    ));
                }
            };
            let incoming = v;
            let before = n;
            match s.interaction {
                Interaction::Refract => {
                    let next = match self.material_index(&s.material, w) {
                        Ok(n) => n,
                        Err(e) => {
                            return Err(TraceFailure::path(
                                FailureKind::Invalid,
                                Some(i),
                                Some(hit),
                                ray,
                                e,
                            ));
                        }
                    };
                    let ci = dot(v, normal).clamp(-1., 1.);
                    let disc = next * next - n * n * (1. - ci * ci);
                    if disc < 0. {
                        return Err(TraceFailure::path(
                            FailureKind::TotalInternalReflection,
                            Some(i),
                            Some(hit),
                            ray,
                            format!("Total internal reflection at surface {i}"),
                        ));
                    }
                    let alpha = disc.sqrt() * if ci >= 0. { 1. } else { -1. } - n * ci;
                    v = match unit(scale(add(scale(v, n), scale(normal, alpha)), 1. / next)) {
                        Ok(v) => v,
                        Err(e) => {
                            return Err(TraceFailure::path(
                                FailureKind::Intersection,
                                Some(i),
                                Some(hit),
                                ray,
                                e,
                            ));
                        }
                    };
                    n = next;
                }
                Interaction::Reflect => {
                    v = unit(sub(v, scale(normal, 2. * dot(v, normal))))
                        .map_err(TraceFailure::launch)?
                }
                Interaction::Dummy => {}
            }
            ray.segments.push(Segment {
                point: hit,
                incoming,
                direction: v,
                normal,
                n_before: before,
                n_after: n,
                opl: ray.opl,
            });
            ray.direction = v;
            p = hit;
            vertex += s.thickness;
        }
        if image {
            if v[2].abs() < 1e-14 {
                return Err(TraceFailure::path(
                    FailureKind::Image,
                    None,
                    None,
                    ray,
                    "Ray parallel to image plane".into(),
                ));
            }
            let t = (vertex - p[2]) / v[2];
            ray.opl += t * n;
            ray.points.push(add(p, scale(v, t)));
        }
        Ok(ray)
    }
    fn stop_launch(
        &self,
        x: f64,
        y: f64,
        field: &Field,
        w: f64,
        apply_vignetting: bool,
    ) -> Result<(V3, V3, f64), String> {
        if ![x, y, field.x, field.y, w].iter().all(|v| v.is_finite()) {
            return Err("Nonfinite ray input".into());
        }
        if self.field_type == FieldType::Angle && (field.x.abs() >= 80. || field.y.abs() >= 80.) {
            return Err("Field angle outside ±80 degrees".into());
        }
        let ep = self.pupil_matrix(w)?;
        if ep.a.abs() < 1e-14 {
            return Err("Entrance pupil lies at infinity".into());
        }
        let radius = self.pupil_radius(w)?;
        let mut pupil = [x / radius, y / radius];
        if apply_vignetting {
            pupil[0] *= 1.
                - if pupil[0] >= 0. {
                    field.vignetting[2]
                } else {
                    field.vignetting[3]
                };
            pupil[1] *= 1.
                - if pupil[1] >= 0. {
                    field.vignetting[0]
                } else {
                    field.vignetting[1]
                };
        }
        let effective = [pupil[0] * radius, pupil[1] * radius];
        // Signed stop magnification preserves an inverted pupil orientation.
        let target = [effective[0] * ep.a, effective[1] * ep.a];
        let shift = ep.b / ep.a;
        let mut first = if let Some(d) = self.object_distance {
            let obj = self.object_point(field, w)?;
            let den = d + shift;
            if den.abs() < 1e-12 {
                return Err("Object lies at entrance pupil".into());
            }
            [
                obj[0] + d * (effective[0] - obj[0]) / den,
                obj[1] + d * (effective[1] - obj[1]) / den,
            ]
        } else {
            [
                effective[0] - shift * field.x.to_radians().tan(),
                effective[1] - shift * field.y.to_radians().tan(),
            ]
        };
        let hit_at_stop = |first: [f64; 2]| -> Result<[f64; 2], String> {
            let (p, v, e) = self.launch(first, field, w)?;
            let r = self
                .trace_path(p, v, w, field, e, self.stop_index, false, false)
                .map_err(|e| e.to_string())?;
            let p = r.last().point;
            Ok([p[0], p[1]])
        };
        for _ in 0..20 {
            let h = hit_at_stop(first)?;
            let error = [h[0] - target[0], h[1] - target[1]];
            if error[0].hypot(error[1]) < 1e-9 {
                return self.launch(first, field, w);
            }
            let delta = 1e-5_f64.max(radius * 1e-6);
            let hx = hit_at_stop([first[0] + delta, first[1]])?;
            let hy = hit_at_stop([first[0], first[1] + delta])?;
            let (a, b, c, d) = (
                (hx[0] - h[0]) / delta,
                (hy[0] - h[0]) / delta,
                (hx[1] - h[1]) / delta,
                (hy[1] - h[1]) / delta,
            );
            let det = a * d - b * c;
            if det.abs() < 1e-15 {
                break;
            }
            first[0] -= (d * error[0] - b * error[1]) / det;
            first[1] -= (-c * error[0] + a * error[1]) / det;
        }
        Err("Pupil ray aiming did not converge".into())
    }
    fn aimed_launch(
        &self,
        x: f64,
        y: f64,
        field: &Field,
        w: f64,
        apply_vignetting: bool,
    ) -> Result<(V3, V3, f64), String> {
        if self.pupil_mapping == PupilMapping::RealStop {
            return self.stop_launch(x, y, field, w, apply_vignetting);
        }
        if ![x, y, w, field.x, field.y].iter().all(|v| v.is_finite()) {
            return Err("Nonfinite ray input".into());
        }
        // A reference chief ray defines the center of the reference entrance
        // pupil. Its offset and plane are held fixed across the spectrum.
        let reference = self.reference();
        let (chief, direction, _) = self.stop_launch(0., 0., field, reference, false)?;
        let radius = self.pupil_radius(w)?;
        let mut pupil = [x / radius, y / radius];
        if apply_vignetting {
            pupil[0] *= 1.
                - if pupil[0] >= 0. {
                    field.vignetting[2]
                } else {
                    field.vignetting[3]
                };
            pupil[1] *= 1.
                - if pupil[1] >= 0. {
                    field.vignetting[0]
                } else {
                    field.vignetting[1]
                };
        }
        let reference_radius = self.pupil_radius(reference)?;
        let scale = if let Some(distance) = self.object_distance {
            let ep = self.pupil_matrix(reference)?;
            if ep.a.abs() < 1e-14 {
                return Err("Entrance pupil lies at infinity".into());
            }
            let den = distance + ep.b / ep.a;
            if den.abs() < 1e-12 {
                return Err("Object lies at entrance pupil".into());
            }
            distance / den
        } else {
            1.
        };
        let first = [0, 1].map(|axis| {
            chief[axis] - chief[2] * direction[axis] / direction[2]
                + reference_radius * scale * pupil[axis]
        });
        self.launch(first, field, w)
    }
    pub(super) fn trace_options(
        &self,
        x: f64,
        y: f64,
        field: &Field,
        w: f64,
        options: TraceOptions,
    ) -> Result<Ray, TraceFailure> {
        if options.last >= self.surfaces.len() {
            return Err(TraceFailure::launch(
                "Surface index outside prescription".into(),
            ));
        }
        let (p, v, e) = self
            .aimed_launch(x, y, field, w, options.vignetting)
            .map_err(TraceFailure::launch)?;
        self.trace_path(
            p,
            v,
            w,
            field,
            e,
            options.last,
            options.apertures,
            options.image,
        )
    }
    pub fn trace_field(&self, x: f64, y: f64, field: &Field, w: f64) -> Result<Ray, String> {
        let last = self
            .surfaces
            .len()
            .checked_sub(1)
            .ok_or("Empty prescription")?;
        self.trace_field_to(x, y, field, w, last, true)
    }
    pub(super) fn trace_field_to(
        &self,
        x: f64,
        y: f64,
        field: &Field,
        w: f64,
        last: usize,
        image: bool,
    ) -> Result<Ray, String> {
        self.trace_options(
            x,
            y,
            field,
            w,
            TraceOptions {
                last,
                image,
                apertures: true,
                vignetting: true,
            },
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inverted_entrance_pupil_keeps_signed_stop_orientation() {
        let mut s = System::default();
        s.surfaces = vec![
            Surface::sphere(1., 5., "N-BK7_SCHOTT", 0.5),
            Surface::sphere(-10., 10., "AIR", 2.),
        ];
        s.stop_index = 1;
        s.pupil.value = 0.2;
        s.fields = vec![Field::meridional(0.)];
        s.update().unwrap();
        let ep = s.pupil_matrix(s.reference()).unwrap();
        assert!(ep.a < 0.);
        let ray = s
            .trace_field(0., 0.05, &s.fields[0], s.reference())
            .unwrap();
        assert!(ray.segments[0].point[1] > 0.);
        assert!((ray.segments[1].point[1] - 0.05 * ep.a).abs() < 1e-9);
    }
    #[test]
    fn negative_gaps_and_virtual_image_keep_signed_optical_path() {
        let mut s = System::default();
        s.surfaces = vec![
            Surface::sphere(0., 5., "AIR", 2.),
            Surface::sphere(0., -3., "AIR", 2.),
            Surface::sphere(0., -1., "AIR", 2.),
        ];
        s.fields = vec![Field::meridional(0.)];
        s.pupil.value = 1.;
        s.update().unwrap();
        let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
        assert_eq!(
            ray.segments.iter().map(|p| p.point[2]).collect::<Vec<_>>(),
            vec![0., 5., 2.]
        );
        assert_eq!(ray.image()[2], 1.);
        assert!((ray.opl - 3.).abs() < 1e-12);
        assert!((ray.segments[2].opl - 4.).abs() < 1e-12);
    }
    #[test]
    fn negative_glass_gap_uses_prescribed_medium_and_signed_path() {
        let mut s = System::default();
        s.surfaces = vec![
            Surface::sphere(0., -3., "N-BK7_SCHOTT", 2.),
            Surface::sphere(0., 1., "AIR", 2.),
        ];
        s.fields = vec![Field::meridional(0.)];
        s.pupil.value = 1.;
        s.update().unwrap();
        let n = s.material_index("N-BK7_SCHOTT", s.reference()).unwrap();
        let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
        assert_eq!(ray.segments[1].point[2], -3.);
        assert!((ray.segments[1].n_before - n).abs() < 1e-12);
        assert!((ray.segments[1].opl - (2. - 3. * n)).abs() < 1e-12);
        assert!((ray.opl - (3. - 3. * n)).abs() < 1e-12);
    }
    #[test]
    fn reference_entrance_mapping_keeps_incident_geometry_across_wavelengths() {
        let mut s = System::default();
        s.stop_index = 1;
        s.pupil_mapping = PupilMapping::ReferenceEntrancePupil;
        s.fields = vec![Field::meridional(3.)];
        s.update().unwrap();
        let f = &s.fields[0];
        let w = s.reference();
        let ray = s
            .trace_field(0., 0.8 * s.pupil_radius(w).unwrap(), f, w)
            .unwrap();
        let other = s
            .trace_field(0., 0.8 * s.pupil_radius(486.1327).unwrap(), f, 486.1327)
            .unwrap();
        for axis in 0..3 {
            assert!((ray.points[0][axis] - other.points[0][axis]).abs() < 1e-12);
            assert!((ray.launch_direction[axis] - other.launch_direction[axis]).abs() < 1e-12);
        }
        let mut aimed = s.clone();
        aimed.pupil_mapping = PupilMapping::RealStop;
        aimed.update().unwrap();
        let real = aimed
            .trace_field(0., 0.8 * aimed.pupil_radius(w).unwrap(), f, w)
            .unwrap();
        let target = s.pupil_matrix(w).unwrap().a * s.pupil_radius(w).unwrap() * 0.8;
        assert!((real.segments[1].point[1] - target).abs() < 1e-9);
        assert!((ray.segments[1].point[1] - target).abs() > 1e-5);
        let a = s.trace_field(0., 0., f, w).unwrap();
        let b = aimed.trace_field(0., 0., f, w).unwrap();
        for axis in 0..2 {
            assert!((a.image()[axis] - b.image()[axis]).abs() < 1e-9);
        }
    }
    #[test]
    fn finite_reference_entrance_mapping_matches_plane_projection() {
        let mut s = System::default();
        s.object_distance = Some(200.);
        s.stop_index = 1;
        s.field_type = FieldType::ObjectHeight;
        s.fields = vec![Field::meridional(0.4)];
        s.pupil_mapping = PupilMapping::ReferenceEntrancePupil;
        s.update().unwrap();
        let f = &s.fields[0];
        let w = s.reference();
        let ep = s.pupil_matrix(w).unwrap();
        let chief = s.trace_field(0., 0., f, w).unwrap();
        let first = |ray: &Ray, axis: usize| {
            ray.points[0][axis]
                - ray.points[0][2] * ray.launch_direction[axis] / ray.launch_direction[2]
        };
        let scale = s.pupil_radius(w).unwrap() * 200. / (200. + ep.b / ep.a);
        for wavelength in [w, 656.2725] {
            let radius = s.pupil_radius(wavelength).unwrap();
            let ray = s
                .trace_field(0.5 * radius, -0.25 * radius, f, wavelength)
                .unwrap();
            assert!((first(&ray, 0) - first(&chief, 0) - 0.5 * scale).abs() < 1e-11);
            assert!((first(&ray, 1) - first(&chief, 1) + 0.25 * scale).abs() < 1e-11);
        }
    }
}
