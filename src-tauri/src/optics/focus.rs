// SPDX-License-Identifier: GPL-3.0-or-later
// Analytic infinitesimal bundles along the exact chief ray.
use super::*;
use trace::TraceOptions;

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct FocusFailure {
    stage: &'static str,
    kind: &'static str,
    surface: Option<usize>,
    endpoint: Option<&'static str>,
    point: Option<V3>,
    pub message: String,
}
impl FocusFailure {
    fn bundle(kind: &'static str, surface: Option<usize>, message: &str) -> Self {
        Self {
            stage: "bundle",
            kind,
            surface,
            endpoint: None,
            point: None,
            message: message.into(),
        }
    }
}
#[derive(schemars::JsonSchema, Clone, Serialize)]
pub struct FocusValue {
    pub focus_z: Option<f64>,
    pub shift: Option<f64>,
    pub error: Option<FocusFailure>,
}
impl FocusValue {
    fn failed(error: FocusFailure) -> Self {
        Self {
            focus_z: None,
            shift: None,
            error: Some(error),
        }
    }
}
#[derive(Serialize)]
pub struct FocalPair {
    pub sagittal: FocusValue,
    pub tangential: FocusValue,
    pub chief_image: Option<V3>,
    pub chief_direction: Option<V3>,
    pub frame: FieldFrame,
}
#[derive(Clone, Copy)]
struct Variation {
    position: V3,
    direction: V3,
}

fn propagate(s: &System, chief: &Ray, axes: [[f64; 2]; 2]) -> Result<[Variation; 2], FocusFailure> {
    let finite = chief.object_point;
    let mut previous = finite.unwrap_or(chief.points[0]);
    let mut variations = axes.map(|axis| {
        let basis = [axis[0], axis[1], 0.];
        if finite.is_some() {
            Variation {
                position: [0.; 3],
                direction: sub(
                    basis,
                    scale(chief.launch_direction, dot(chief.launch_direction, basis)),
                ),
            }
        } else {
            Variation {
                position: basis,
                direction: [0.; 3],
            }
        }
    });
    let mut vertex = 0.;
    for (i, segment) in chief.segments.iter().enumerate() {
        let surface = &s.surfaces[i];
        let t = dot(sub(segment.point, previous), segment.incoming);
        let ci = dot(segment.incoming, segment.normal);
        if ci.abs() < 1e-12 {
            return Err(FocusFailure::bundle(
                "grazing_intersection",
                Some(i),
                "Chief ray is tangent to the surface",
            ));
        }
        for variation in &mut variations {
            let at_fixed_distance = add(variation.position, scale(variation.direction, t));
            let position = sub(
                at_fixed_distance,
                scale(
                    segment.incoming,
                    dot(segment.normal, at_fixed_distance) / ci,
                ),
            );
            let local = [
                segment.point[0],
                segment.point[1],
                segment.point[2] - vertex,
            ];
            let dn = surface
                .profile()
                .normal_variation(local, position)
                .map_err(|_| {
                    FocusFailure::bundle(
                        "normal_derivative",
                        Some(i),
                        "Surface normal derivative is undefined",
                    )
                })?;
            let dci = dot(variation.direction, segment.normal) + dot(segment.incoming, dn);
            let direction = match surface.interaction {
                Interaction::Dummy => variation.direction,
                Interaction::Reflect => sub(
                    variation.direction,
                    scale(add(scale(segment.normal, dci), scale(dn, ci)), 2.),
                ),
                Interaction::Refract => {
                    let (before, after) = (segment.n_before, segment.n_after);
                    let discriminant = after * after - before * before * (1. - ci * ci);
                    if discriminant <= 1e-24 * after * after {
                        return Err(FocusFailure::bundle(
                            "critical_refraction",
                            Some(i),
                            "Chief ray is at the critical refraction angle",
                        ));
                    }
                    let root = discriminant.sqrt().copysign(ci);
                    let alpha = root - before * ci;
                    let da = (before * before * ci / root - before) * dci;
                    scale(
                        add(
                            scale(variation.direction, before),
                            add(scale(segment.normal, da), scale(dn, alpha)),
                        ),
                        1. / after,
                    )
                }
            };
            let direction = sub(
                direction,
                scale(segment.direction, dot(segment.direction, direction)),
            );
            if !position.iter().chain(&direction).all(|v| v.is_finite()) {
                return Err(FocusFailure::bundle(
                    "nonfinite",
                    Some(i),
                    "Nonfinite infinitesimal bundle",
                ));
            }
            *variation = Variation {
                position,
                direction,
            };
        }
        previous = segment.point;
        vertex += surface.thickness;
    }
    Ok(variations)
}

pub fn chief_foci(s: &System, field: &Field, w: f64) -> FocalPair {
    let frame = s.field_frame(field);
    let last = s.surfaces.len() - 1;
    let result = s.trace_options(
        0.,
        0.,
        field,
        w,
        TraceOptions {
            last,
            image: true,
            apertures: true,
            vignetting: true,
        },
    );
    let chief = match result {
        Ok(ray) => ray,
        Err(e) => {
            let failure = FocusFailure {
                stage: "chief",
                kind: e.kind.as_str(),
                surface: e.surface,
                endpoint: e.endpoint,
                point: e.point,
                message: e.message,
            };
            return FocalPair {
                sagittal: FocusValue::failed(failure.clone()),
                tangential: FocusValue::failed(failure),
                chief_image: None,
                chief_direction: None,
                frame,
            };
        }
    };
    let common_failure = |failure: FocusFailure| FocalPair {
        sagittal: FocusValue::failed(failure.clone()),
        tangential: FocusValue::failed(failure),
        chief_image: Some(chief.image()),
        chief_direction: Some(chief.direction),
        frame,
    };
    let axes = [frame.sagittal, frame.tangential];
    let variations = match propagate(s, &chief, axes) {
        Ok(v) => v,
        Err(e) => return common_failure(e),
    };
    let image_vertex = s.surfaces.iter().map(|v| v.thickness).sum::<f64>();
    let last = chief.last();
    let focus = |axis: [f64; 2], variation: Variation| {
        let vz = chief.direction[2];
        if vz.abs() < 1e-12 {
            let mut e = FocusFailure::bundle(
                "axial_direction",
                None,
                "Chief ray is parallel to the image vertex plane",
            );
            e.stage = "focus";
            return FocusValue::failed(e);
        }
        let slope = [chief.direction[0] / vz, chief.direction[1] / vz];
        let height = [0, 1].map(|a| variation.position[a] - slope[a] * variation.position[2]);
        let derivative =
            [0, 1].map(|a| (variation.direction[a] - slope[a] * variation.direction[2]) / vz);
        let h = FieldFrame::project(axis, height);
        let u = FieldFrame::project(axis, derivative);
        let z = last.point[2] - h / u;
        if u.abs() <= 1e-14 * norm(variation.direction).max(1e-30)
            || !z.is_finite()
            || !(z - image_vertex).is_finite()
        {
            let mut e =
                FocusFailure::bundle("at_infinity", None, "Infinitesimal focus lies at infinity");
            e.stage = "focus";
            return FocusValue::failed(e);
        }
        FocusValue {
            focus_z: Some(z),
            shift: Some(z - image_vertex),
            error: None,
        }
    };
    FocalPair {
        sagittal: focus(axes[0], variations[0]),
        tangential: focus(axes[1], variations[1]),
        chief_image: Some(chief.image()),
        chief_direction: Some(chief.direction),
        frame,
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct FieldFocus {
    fraction: f64,
    field: [f64; 2],
    radius: f64,
    sagittal: FocusValue,
    tangential: FocusValue,
    chief_image: Option<V3>,
    frame: FieldFrame,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct FieldCurve {
    wavelength_index: usize,
    wavelength: f64,
    weight: f64,
    color: [f64; 4],
    points: Vec<FieldFocus>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct FieldCurvature {
    field_end: [f64; 2],
    max_field: f64,
    samples: usize,
    field_type: FieldType,
    field_units: &'static str,
    image_vertex: f64,
    curves: Vec<FieldCurve>,
    units: &'static str,
    model: &'static str,
}

pub fn field_curvature(
    s: &System,
    field_end: Option<[f64; 2]>,
    samples: usize,
) -> Result<Value, String> {
    serde_json::to_value(field_curvature_report(s, field_end, samples)?).map_err(|e| e.to_string())
}
pub(crate) fn field_curvature_report(
    s: &System,
    field_end: Option<[f64; 2]>,
    samples: usize,
) -> Result<FieldCurvature, String> {
    if !(2..=257).contains(&samples) {
        return Err("Field samples must be 2–257".into());
    }
    let end = field_end.unwrap_or([
        0.,
        s.fields.iter().map(|f| f.x.hypot(f.y)).fold(0., f64::max),
    ]);
    if !end.iter().all(|v| v.is_finite()) {
        return Err("Nonfinite field endpoint".into());
    }
    let mut curves = vec![];
    for (index, w) in s.wavelengths.iter().enumerate() {
        let points: Vec<_> = (0..samples)
            .map(|i| {
                let fraction = i as f64 / (samples - 1) as f64;
                let field = Field {
                    x: end[0] * fraction,
                    y: end[1] * fraction,
                    ..Field::meridional(0.)
                };
                let pair = chief_foci(s, &field, w.value);
                FieldFocus {
                    fraction,
                    field: [field.x, field.y],
                    radius: field.x.hypot(field.y),
                    sagittal: pair.sagittal,
                    tangential: pair.tangential,
                    chief_image: pair.chief_image,
                    frame: pair.frame,
                }
            })
            .collect();
        curves.push(FieldCurve {
            wavelength_index: index,
            wavelength: w.value,
            weight: w.weight,
            color: w.color,
            points,
        });
    }
    Ok(FieldCurvature {
        field_end: end,
        max_field: end[0].hypot(end[1]),
        samples,
        field_type: s.field_type,
        field_units: if s.field_type == FieldType::Angle {
            "degrees"
        } else {
            "mm"
        },
        image_vertex: s.surfaces.iter().map(|v| v.thickness).sum::<f64>(),
        curves,
        units: "mm",
        model: "Analytic infinitesimal bundles along each real chief ray, differentiating surface intersections, normals, Snell refraction and reflection. Focus z and shift use global axial coordinates; every shift is relative to the stored image vertex, including curved images. Physical apertures check the chief only; off-chief bundle clipping is not modeled. All stored wavelengths are diagnostic, including duplicate and zero-weight entries. Temporary fields have zero vignetting and follow the saved pupil mapping.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: f64, b: f64, tolerance: f64) {
        assert!((a - b).abs() < tolerance, "{a} != {b}");
    }
    fn plane_glass() -> System {
        let mut s = System::default();
        s.wavelengths = vec![Wavelength::new(587.562, 1.)];
        s.reference_wavelength = 0;
        s.surfaces = vec![Surface::sphere(40., 80., "1.500000:60.000000", 20.)];
        s.pupil.value = 1.;
        s
    }
    #[test]
    fn profile_normal_jacobian_matches_radial_derivatives_and_vertical_conic_rim() {
        let mut surface = Surface::sphere(30., 1., "AIR", 10.);
        surface.conic = -0.5;
        surface.even = vec![0.0001, -0.000001];
        surface.odd = vec![0.0002];
        let p = surface.profile();
        for xy in [[0_f64, 0.], [1.2, -0.8]] {
            for axis in [[1., 0.], [0., 1.]] {
                let r = xy[0].hypot(xy[1]);
                let (z, dz) = p.sag(r).unwrap();
                let tangent = [
                    axis[0],
                    axis[1],
                    if r > 0. {
                        dz * (xy[0] * axis[0] + xy[1] * axis[1]) / r
                    } else {
                        0.
                    },
                ];
                let actual = p.normal_variation([xy[0], xy[1], z], tangent).unwrap();
                let normal = |fraction: f64| {
                    let a = [xy[0] + fraction * axis[0], xy[1] + fraction * axis[1]];
                    let r = a[0].hypot(a[1]);
                    let (_, slope) = p.sag(r).unwrap();
                    unit(if r > 0. {
                        [-slope * a[0] / r, -slope * a[1] / r, 1.]
                    } else {
                        [0., 0., 1.]
                    })
                    .unwrap()
                };
                let numeric = scale(sub(normal(1e-5), normal(-1e-5)), 50000.);
                assert!(norm(sub(actual, numeric)) < if r == 0. { 1e-8 } else { 1e-10 });
            }
        }
        let p = Surface::sphere(2., 1., "AIR", 2.);
        let rim = p
            .profile()
            .normal_variation([2., 0., 2.], [0., 0., 2.])
            .unwrap();
        assert_eq!(rim, [0., 0., -1.]);
    }
    #[test]
    fn oblique_spherical_refraction_matches_coddington_closed_form() {
        for profile in 0..3 {
            let mut s = plane_glass();
            if profile > 0 {
                s.surfaces[0].conic = -0.4;
            }
            if profile == 1 {
                s.surfaces[0].even = vec![1e-6, -1e-8];
            }
            if profile == 2 {
                s.surfaces[0].odd = vec![1e-6, -1e-8];
            }
            for theta in [0_f64, 10., 35.] {
                let f = Field::meridional(theta);
                let pair = chief_foci(&s, &f, s.reference());
                let incident = theta.to_radians();
                let refracted = (incident.sin() / 1.5).asin();
                let power = (1.5 * refracted.cos() - incident.cos()) / 40.;
                let sagittal = 1.5 * refracted.cos() / power;
                let tangential = 1.5 * refracted.cos().powi(3) / power;
                close(pair.sagittal.focus_z.unwrap(), sagittal, 2e-10);
                close(pair.tangential.focus_z.unwrap(), tangential, 2e-10);
                close(pair.sagittal.shift.unwrap(), sagittal - 80., 2e-10);
            }
        }
    }
    #[test]
    fn oblique_mirror_coddington_and_finite_curved_source_virtual_focus() {
        let mut s = plane_glass();
        s.surfaces[0].radius = -100.;
        s.surfaces[0].thickness = -50.;
        s.surfaces[0].interaction = Interaction::Reflect;
        for theta in [0_f64, 10., 30.] {
            let pair = chief_foci(&s, &Field::meridional(theta), s.reference());
            close(pair.sagittal.focus_z.unwrap(), -50., 2e-10);
            close(
                pair.tangential.focus_z.unwrap(),
                -50. * theta.to_radians().cos().powi(2),
                2e-10,
            );
        }
        s.object_distance = Some(100.);
        s.object_surface.radius = 50.;
        s.field_type = FieldType::ObjectHeight;
        s.surfaces[0].radius = 0.;
        s.surfaces[0].thickness = 150.;
        s.image_surface.radius = 20.;
        let mut f = Field::meridional(2.);
        f.x = 1.;
        let pair = chief_foci(&s, &f, s.reference());
        let expected = 50. + 2495_f64.sqrt();
        close(pair.sagittal.focus_z.unwrap(), expected, 2e-10);
        close(pair.tangential.focus_z.unwrap(), expected, 2e-10);
        close(pair.tangential.shift.unwrap(), expected - 150., 2e-10);
    }
    fn numerical_focus(s: &System, f: &Field, w: f64, axis: [f64; 2], epsilon: f64) -> f64 {
        let chief = s.trace_field(0., 0., f, w).unwrap();
        let p = chief.last().point;
        let a = s
            .trace_field(epsilon * axis[0], epsilon * axis[1], f, w)
            .unwrap();
        let b = s
            .trace_field(-epsilon * axis[0], -epsilon * axis[1], f, w)
            .unwrap();
        let at = |ray: &Ray| {
            [0, 1].map(|k| {
                ray.last().point[k]
                    + (p[2] - ray.last().point[2]) * ray.direction[k] / ray.direction[2]
            })
        };
        let h = FieldFrame::project(axis, [at(&a)[0] - at(&b)[0], at(&a)[1] - at(&b)[1]]);
        let u = FieldFrame::project(
            axis,
            [
                a.direction[0] / a.direction[2] - b.direction[0] / b.direction[2],
                a.direction[1] / a.direction[2] - b.direction[1] / b.direction[2],
            ],
        );
        p[2] - h / u
    }
    #[test]
    fn mixed_aspheres_match_convergent_real_bundle_and_rotated_skew_fields() {
        for finite in [false, true] {
            for mapping in [PupilMapping::RealStop, PupilMapping::ReferenceEntrancePupil] {
                let mut s = System::default();
                s.stop_index = 1;
                s.pupil_mapping = mapping;
                s.object_distance = finite.then_some(200.);
                s.object_surface.radius = if finite { 200. } else { 0. };
                s.field_type = if finite {
                    FieldType::ObjectHeight
                } else {
                    FieldType::Angle
                };
                s.surfaces[0].even = vec![1e-5];
                s.surfaces[1].odd = vec![1e-5, -1e-6];
                let f = Field::meridional(if finite { 3. } else { 5. });
                let mut rotated = f.clone();
                let a = 0.6_f64;
                if finite {
                    rotated.x = f.y * a.sin();
                    rotated.y = f.y * a.cos();
                } else {
                    rotated.x = (f.y.to_radians().tan() * a.sin()).atan().to_degrees();
                    rotated.y = (f.y.to_radians().tan() * a.cos()).atan().to_degrees();
                }
                let w = 486.1327;
                let base = chief_foci(&s, &f, w);
                let skew = chief_foci(&s, &rotated, w);
                for (axis, value, other) in [
                    (base.frame.sagittal, base.sagittal, skew.sagittal),
                    (base.frame.tangential, base.tangential, skew.tangential),
                ] {
                    let actual = value.focus_z.unwrap();
                    close(actual, other.focus_z.unwrap(), 1e-8);
                    let coarse = numerical_focus(&s, &f, w, axis, 0.02);
                    let medium = numerical_focus(&s, &f, w, axis, 0.002);
                    let fine = numerical_focus(&s, &f, w, axis, 0.0002);
                    // The r^3 term is C2 at the axis; its central slope derivative converges linearly.
                    assert!((medium - actual).abs() < (coarse - actual).abs() / 5.);
                    assert!(
                        (fine - actual).abs() < (medium - actual).abs() / 5.,
                        "finite={finite} mapping={mapping:?} axis={axis:?} actual={actual:.14} coarse={coarse:.14} medium={medium:.14} fine={fine:.14}"
                    );
                    let finest = numerical_focus(&s, &f, w, axis, 0.00002);
                    let extrapolated = (10. * finest - fine) / 9.;
                    close(actual, extrapolated, 1e-5);
                }
            }
        }
    }
    #[test]
    fn curvature_keeps_every_spectral_entry_and_exact_field_grid() {
        let mut s = System::default();
        let mut duplicate = s.wavelengths[0].clone();
        duplicate.weight = 0.;
        duplicate.color = [0.8, 0.2, 0.6, 0.25];
        s.wavelengths.push(duplicate);
        s.fields = vec![Field {
            x: 3.,
            ..Field::meridional(4.)
        }];
        let result = field_curvature(&s, None, 17).unwrap();
        assert_eq!(result["field_end"], json!([0., 5.]));
        assert_eq!(result["curves"].as_array().unwrap().len(), 4);
        assert_eq!(result["curves"][3]["weight"], 0.);
        assert_eq!(result["curves"][3]["color"], json!([0.8, 0.2, 0.6, 0.25]));
        assert_eq!(result["curves"][0]["points"], result["curves"][3]["points"]);
        let custom = field_curvature(&s, Some([3., 4.]), 17).unwrap();
        let point = &custom["curves"][1]["points"][8];
        assert_eq!(point["field"], json!([1.5, 2.]));
        assert_eq!(point["radius"], 2.5);
        let field = Field {
            x: 1.5,
            ..Field::meridional(2.)
        };
        let direct = super::super::analysis::astigmatism(&s, &field, s.reference()).unwrap();
        close(
            point["sagittal"]["shift"].as_f64().unwrap(),
            direct["sagittal_shift"].as_f64().unwrap(),
            1e-12,
        );
        assert!(field_curvature(&s, None, 1).is_err());
        assert!(field_curvature(&s, Some([f64::INFINITY, 0.]), 17).is_err());
    }
    #[test]
    fn failed_chiefs_keep_grid_and_one_infinite_branch_does_not_drop_the_other() {
        let mut s = plane_glass();
        s.object_distance = Some(100.);
        s.object_surface.clear_aperture = true;
        s.object_surface.aperture = 0.01;
        s.field_type = FieldType::ObjectHeight;
        let result = field_curvature(&s, Some([0., 1.]), 5).unwrap();
        let points = result["curves"][0]["points"].as_array().unwrap();
        assert_eq!(points.len(), 5);
        assert!(points[0]["sagittal"]["error"].is_null());
        for p in &points[1..] {
            assert_eq!(p["sagittal"]["error"]["stage"], "chief");
            assert_eq!(p["sagittal"]["error"]["endpoint"], "object");
            assert!(p["sagittal"]["shift"].is_null());
        }
        s.object_distance = None;
        s.object_surface = profile::EndpointSurface::plane();
        s.field_type = FieldType::Angle;
        let mut dummy = Surface::sphere(0., 10., "AIR", 20.);
        dummy.interaction = Interaction::Dummy;
        let mut mirror = Surface::sphere(0., -30., "AIR", 20.);
        mirror.interaction = Interaction::Reflect;
        mirror.odd = vec![4., -7., 3.];
        mirror.clear_aperture = false;
        s.surfaces = vec![dummy, mirror];
        let pair = chief_foci(
            &s,
            &Field::meridional(0.1_f64.atan().to_degrees()),
            s.reference(),
        );
        assert!(pair.sagittal.focus_z.is_some());
        assert_eq!(pair.tangential.error.unwrap().kind, "at_infinity");
        s.surfaces[1].odd.clear();
        s.surfaces[1].interaction = Interaction::Dummy;
        let empty = field_curvature(&s, Some([0., 0.]), 2).unwrap();
        for p in empty["curves"][0]["points"].as_array().unwrap() {
            assert_eq!(p["sagittal"]["error"]["kind"], "at_infinity");
            assert_eq!(p["tangential"]["error"]["kind"], "at_infinity");
        }
    }
}
