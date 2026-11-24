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
    pub endpoint: Option<&'static str>,
}
impl TraceFailure {
    fn launch(message: String) -> Self {
        Self {
            kind: FailureKind::Aiming,
            surface: None,
            point: None,
            ray: None,
            message,
            endpoint: None,
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
            endpoint: None,
        }
    }
    fn at(mut self, endpoint: &'static str) -> Self {
        self.endpoint = Some(endpoint);
        self
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

pub(super) struct Launch {
    pub point: V3,
    pub direction: V3,
    pub entrance_opl: f64,
    pub object_point: Option<V3>,
}

impl System {
    pub(super) fn trace_path(
        &self,
        launch: Launch,
        w: f64,
        field: &Field,
        last: usize,
        check_apertures: bool,
        image: bool,
    ) -> Result<Ray, TraceFailure> {
        let Launch {
            point: p,
            direction: v,
            entrance_opl,
            object_point,
        } = launch;
        let mut ray = Ray {
            object_point,
            image_hit: None,
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
        if check_apertures && self.object_surface.clear_aperture {
            if let Some(point) = object_point {
                if point[0].hypot(point[1]) > self.object_surface.aperture + 1e-9 {
                    return Err(TraceFailure::path(
                        FailureKind::Blocked,
                        None,
                        Some(point),
                        ray,
                        "Ray blocked at object aperture".into(),
                    )
                    .at("object"));
                }
            }
        }
        let n0 = self.object_index(w).map_err(TraceFailure::launch)?;
        let (mut p, mut v, mut n) = (p, v, n0);
        let mut frame = coord::Frame::identity();
        for (i, s) in self.surfaces.iter().enumerate().take(last + 1) {
            let saved_frame = frame;
            if let Some(t) = &s.transform {
                frame = frame.apply(t);
            }
            let (hit, normal, distance) = if frame.is_centered() {
                if v[2].abs() < 1e-14 {
                    return Err(TraceFailure::path(
                        FailureKind::Intersection,
                        Some(i),
                        None,
                        ray,
                        format!("Ray parallel to surface {i}"),
                    ));
                }
                let intersection = match s.profile().intersect(p, v, frame.origin[2]) {
                    Ok(hit) => hit,
                    Err(e) => {
                        return Err(TraceFailure::path(
                            FailureKind::Intersection,
                            Some(i),
                            None,
                            ray,
                            format!("Surface {i}: {e}"),
                        ));
                    }
                };
                if check_apertures
                    && s.clear_aperture
                    && intersection.point[0].hypot(intersection.point[1]) > s.aperture + 1e-9
                {
                    return Err(TraceFailure::path(
                        FailureKind::Blocked,
                        Some(i),
                        Some(intersection.point),
                        ray,
                        format!("Ray blocked at surface {i}"),
                    ));
                }
                (
                    intersection.point,
                    intersection.normal,
                    intersection.distance,
                )
            } else {
                let pl = frame.to_local_point(p);
                let vl = frame.to_local_vec(v);
                if vl[2].abs() < 1e-14 {
                    return Err(TraceFailure::path(
                        FailureKind::Intersection,
                        Some(i),
                        None,
                        ray,
                        format!("Ray parallel to surface {i}"),
                    ));
                }
                let intersection = match s.profile().intersect(pl, vl, 0.) {
                    Ok(hit) => hit,
                    Err(e) => {
                        return Err(TraceFailure::path(
                            FailureKind::Intersection,
                            Some(i),
                            None,
                            ray,
                            format!("Surface {i}: {e}"),
                        ));
                    }
                };
                let global_hit = frame.to_global_point(intersection.point);
                if check_apertures
                    && s.clear_aperture
                    && intersection.point[0].hypot(intersection.point[1]) > s.aperture + 1e-9
                {
                    return Err(TraceFailure::path(
                        FailureKind::Blocked,
                        Some(i),
                        Some(global_hit),
                        ray,
                        format!("Ray blocked at surface {i}"),
                    ));
                }
                let global_normal = frame.to_global_vec(intersection.normal);
                (global_hit, global_normal, intersection.distance)
            };
            ray.opl += distance * n;
            ray.points.push(hit);
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
            if s.transform.as_ref().is_some_and(|t| t.return_after) {
                frame = saved_frame;
            }
            frame.translate_z(s.thickness);
        }
        if image {
            let (hit_point, hit_normal, hit_distance) = if frame.is_centered() {
                let hit = match self.image_surface.profile().intersect(p, v, frame.origin[2]) {
                    Ok(hit) => hit,
                    Err(e) => {
                        return Err(TraceFailure::path(
                            FailureKind::Image,
                            None,
                            None,
                            ray,
                            format!("Image surface: {e}"),
                        )
                        .at("image"));
                    }
                };
                if check_apertures
                    && self.image_surface.clear_aperture
                    && hit.point[0].hypot(hit.point[1]) > self.image_surface.aperture + 1e-9
                {
                    return Err(TraceFailure::path(
                        FailureKind::Blocked,
                        None,
                        Some(hit.point),
                        ray,
                        "Ray blocked at image aperture".into(),
                    )
                    .at("image"));
                }
                (hit.point, hit.normal, hit.distance)
            } else {
                let pl = frame.to_local_point(p);
                let vl = frame.to_local_vec(v);
                let hit = match self.image_surface.profile().intersect(pl, vl, 0.) {
                    Ok(hit) => hit,
                    Err(e) => {
                        return Err(TraceFailure::path(
                            FailureKind::Image,
                            None,
                            None,
                            ray,
                            format!("Image surface: {e}"),
                        )
                        .at("image"));
                    }
                };
                let global_hit = frame.to_global_point(hit.point);
                if check_apertures
                    && self.image_surface.clear_aperture
                    && hit.point[0].hypot(hit.point[1]) > self.image_surface.aperture + 1e-9
                {
                    return Err(TraceFailure::path(
                        FailureKind::Blocked,
                        None,
                        Some(global_hit),
                        ray,
                        "Ray blocked at image aperture".into(),
                    )
                    .at("image"));
                }
                (global_hit, frame.to_global_vec(hit.normal), hit.distance)
            };
            ray.opl += hit_distance * n;
            ray.points.push(hit_point);
            ray.image_hit = Some(Segment {
                point: hit_point,
                incoming: v,
                direction: v,
                normal: hit_normal,
                n_before: n,
                n_after: n,
                opl: ray.opl,
            });
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
    ) -> Result<Launch, String> {
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
        let shift = self.entrance_pupil_distance(w)?;
        let mut first = if self.object_distance.is_some() {
            let obj = self.object_point(field, w)?;
            let d = -obj[2];
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
        let stop_frame = self.surface_frames().0[self.stop_index];
        let hit_at_stop = |first: [f64; 2]| -> Result<[f64; 2], String> {
            let launch = self.launch(first, field, w)?;
            let r = self
                .trace_path(launch, w, field, self.stop_index, false, false)
                .map_err(|e| e.to_string())?;
            let p = stop_frame.to_local_point(r.last().point);
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
    ) -> Result<Launch, String> {
        if self.pupil_mapping == PupilMapping::RealStop {
            return self.stop_launch(x, y, field, w, apply_vignetting);
        }
        if ![x, y, w, field.x, field.y].iter().all(|v| v.is_finite()) {
            return Err("Nonfinite ray input".into());
        }
        // A reference chief ray defines the center of the reference entrance
        // pupil. Its offset and plane are held fixed across the spectrum.
        let reference = self.reference();
        let chief = self.stop_launch(0., 0., field, reference, false)?;
        let direction = chief.direction;
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
        let scale = if self.object_distance.is_some() {
            let distance = -self.object_point(field, reference)?[2];
            let ep = self.pupil_matrix(reference)?;
            if ep.a.abs() < 1e-14 {
                return Err("Entrance pupil lies at infinity".into());
            }
            let den = distance + self.entrance_pupil_distance(reference)?;
            if den.abs() < 1e-12 {
                return Err("Object lies at entrance pupil".into());
            }
            distance / den
        } else {
            1.
        };
        let first = [0, 1].map(|axis| {
            chief.point[axis] - chief.point[2] * direction[axis] / direction[2]
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
        if self.object_distance.is_some() {
            self.object_point(field, w)
                .map_err(|e| TraceFailure::launch(e).at("object"))?;
        }
        let launch = self
            .aimed_launch(x, y, field, w, options.vignetting)
            .map_err(TraceFailure::launch)?;
        self.trace_path(
            launch,
            w,
            field,
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
    #[test]
    fn tilted_fold_mirror_redirects_beam_90_degrees_into_folded_frame() {
        let mut s = System::default();
        let mut fold = Surface::sphere(0., -20., "AIR", 20.);
        fold.interaction = Interaction::Reflect;
        // Tilt mirror 45 deg about X so local Z points along (0, -sin45, cos45), reflecting +Z -> +Y.
        // Then a coordinate break tilts another 45 deg (total 90 deg) so -Z in folded frame points along +Y.
        fold.transform = Some(CoordinateTransform {
            decenter: [0., 0.],
            tilt: [45., 0., 0.],
            tilt_first: false,
            return_after: false,
        });
        let mut post_cb = Surface::sphere(0., -30., "AIR", 20.);
        post_cb.interaction = Interaction::Dummy;
        post_cb.transform = Some(CoordinateTransform {
            decenter: [0., 0.],
            tilt: [45., 0., 0.],
            tilt_first: false,
            return_after: false,
        });
        fold.thickness = 0.;
        s.surfaces = vec![fold, post_cb];
        s.stop_index = 0;
        s.fields = vec![Field::meridional(0.)];
        s.pupil.value = 4.;
        s.update().unwrap();
        let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
        // After 45 deg reflection, outgoing direction is (0, +1, 0) and image sits at (0, +30, 0).
        assert!(ray.segments[0].direction[0].abs() < 1e-12);
        assert!((ray.segments[0].direction[1] - 1.).abs() < 1e-12);
        assert!(ray.segments[0].direction[2].abs() < 1e-12);
        let img = ray.image();
        assert!(img[0].abs() < 1e-12);
        assert!((img[1] - 30.).abs() < 1e-12);
        assert!(img[2].abs() < 1e-12);
    }
    #[test]
    fn return_after_decenter_shifts_only_perturbed_surface() {
        let mut s = System::default();
        s.fields = vec![Field::meridional(0.)];
        s.surfaces[0].transform = Some(CoordinateTransform {
            decenter: [0., 0.5],
            tilt: [0., 0., 0.],
            tilt_first: false,
            return_after: true,
        });
        s.update().unwrap();
        let (frames, img_frame) = s.surface_frames();
        assert!((frames[0].origin[1] - 0.5).abs() < 1e-12);
        assert!(frames[1].origin[1].abs() < 1e-12);
        assert!(img_frame.origin[1].abs() < 1e-12);
        let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
        assert!(ray.image()[1].abs() > 1e-3);
    }
}
