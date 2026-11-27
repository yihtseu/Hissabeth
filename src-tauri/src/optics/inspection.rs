// SPDX-License-Identifier: GPL-3.0-or-later
// Inspect saved prescriptions without changing their optical state.
use super::*;
use trace::{Launch, TraceOptions};

#[derive(schemars::JsonSchema, Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InspectionLaunch {
    Pupil {
        pupil: [f64; 2],
        #[schemars(range(min = 0, max = 63))]
        field_index: usize,
    },
    Object {
        point: [f64; 2],
        tangent: [f64; 2],
    },
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Failure {
    kind: trace::FailureKind,
    surface: Option<usize>,
    endpoint: Option<&'static str>,
    point: Option<V3>,
    message: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct Inspection {
    launch: InspectionLaunch,
    wavelength: f64,
    ray: Option<Ray>,
    failure: Option<Failure>,
    coordinates: &'static str,
    optical_path: &'static str,
}

pub fn inspect(s: &System, launch: InspectionLaunch, w: f64) -> Result<Value, String> {
    serde_json::to_value(inspect_report(s, launch, w)?).map_err(|e| e.to_string())
}
pub(crate) fn inspect_report(
    s: &System,
    launch: InspectionLaunch,
    w: f64,
) -> Result<Inspection, String> {
    if !w.is_finite() || !(100. ..=20000.).contains(&w) {
        return Err("Wavelength outside 100–20000 nm".into());
    }
    let last = s
        .surfaces
        .len()
        .checked_sub(1)
        .ok_or("Empty prescription")?;
    let result = match &launch {
        InspectionLaunch::Pupil { pupil, field_index } => {
            if !pupil.iter().all(|v| v.is_finite()) {
                return Err("Nonfinite pupil coordinate".into());
            }
            let field = s
                .fields
                .get(*field_index)
                .ok_or("Field index outside prescription")?;
            let radius = s.pupil_radius(w)?;
            s.trace_options(
                pupil[0] * radius,
                pupil[1] * radius,
                field,
                w,
                TraceOptions {
                    last,
                    image: true,
                    apertures: true,
                    vignetting: true,
                },
            )
        }
        InspectionLaunch::Object { point, tangent } => {
            if !point.iter().chain(tangent).all(|v| v.is_finite()) {
                return Err("Nonfinite object ray input".into());
            }
            let distance = s
                .object_distance
                .ok_or("Object launch requires a finite object distance")?;
            let sag = s.object_surface.sag(point[0].hypot(point[1]))?.0;
            let source = [point[0], point[1], -distance + sag];
            let direction = unit([tangent[0], tangent[1], 1.])?;
            s.trace_path(
                Launch {
                    point: source,
                    direction,
                    entrance_opl: 0.,
                    object_point: Some(source),
                },
                w,
                &Field::meridional(0.),
                last,
                true,
                true,
            )
        }
    };
    let (ray, failure) = match result {
        Ok(ray) => (Some(ray), None),
        Err(e) => (
            e.ray.map(|r| *r),
            Some(Failure {
                kind: e.kind,
                surface: e.surface,
                endpoint: e.endpoint,
                point: e.point,
                message: e.message,
            }),
        ),
    };
    Ok(Inspection {
        launch,
        wavelength: w,
        ray,
        failure,
        coordinates: "global x, y, z in mm; directions are unit vectors",
        optical_path: "segment OPL is signed from ray.points[0]; entrance_opl locates that launch relative to the source or incident plane-wave reference",
    })
}

#[derive(schemars::JsonSchema, Serialize)]
struct ParaxialRow {
    endpoint: &'static str,
    surface_index: Option<usize>,
    point: V3,
    slope_in: [f64; 2],
    slope_out: [f64; 2],
    reduced_in: [f64; 2],
    reduced_out: [f64; 2],
    signed_index_in: f64,
    signed_index_out: f64,
}
fn row(
    endpoint: &'static str,
    surface_index: Option<usize>,
    height: [f64; 2],
    z: f64,
    before: ([f64; 2], f64),
    after: ([f64; 2], f64),
) -> ParaxialRow {
    ParaxialRow {
        endpoint,
        surface_index,
        point: [height[0], height[1], z],
        slope_in: before.0.map(|v| v / before.1),
        slope_out: after.0.map(|v| v / after.1),
        reduced_in: before.0,
        reduced_out: after.0,
        signed_index_in: before.1,
        signed_index_out: after.1,
    }
}
fn table(
    s: &System,
    mut height: [f64; 2],
    mut q: [f64; 2],
    object: Option<V3>,
    w: f64,
) -> Result<Vec<ParaxialRow>, String> {
    let mut n = s.object_index(w)?;
    let mut z = 0.;
    let mut rows = vec![];
    if let Some(point) = object {
        rows.push(row(
            "object",
            None,
            [point[0], point[1]],
            point[2],
            (q, n),
            (q, n),
        ));
    }
    for (i, surface) in s.surfaces.iter().enumerate() {
        let (next, power) = s.paraxial_step(surface, n, w)?;
        let outgoing = [0, 1].map(|axis| q[axis] - power * height[axis]);
        rows.push(row("surface", Some(i), height, z, (q, n), (outgoing, next)));
        q = outgoing;
        n = next;
        height = [0, 1].map(|axis| height[axis] + surface.thickness * q[axis] / n);
        z += surface.thickness;
    }
    rows.push(row("image", None, height, z, (q, n), (q, n)));
    if rows.iter().any(|r| {
        r.point
            .iter()
            .chain(&r.slope_in)
            .chain(&r.slope_out)
            .chain(&r.reduced_in)
            .chain(&r.reduced_out)
            .any(|v| !v.is_finite())
    }) {
        return Err("Nonfinite paraxial ray".into());
    }
    Ok(rows)
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct ParaxialTrace {
    wavelength: f64,
    field: [f64; 2],
    entrance_pupil_radius: f64,
    axis: Vec<ParaxialRow>,
    chief: Vec<ParaxialRow>,
    model: &'static str,
    basis: &'static str,
}

pub fn paraxial_trace(s: &System, f: &Field, w: f64) -> Result<Value, String> {
    serde_json::to_value(paraxial_trace_report(s, f, w)?).map_err(|e| e.to_string())
}
pub(crate) fn paraxial_trace_report(
    s: &System,
    f: &Field,
    w: f64,
) -> Result<ParaxialTrace, String> {
    let n0 = s.object_index(w)?;
    let radius = s.pupil_radius(w)?;
    let shift = s.entrance_pupil_distance(w)?;
    let (axis_height, axis_q, axis_object) = if let Some(d) = s.object_distance {
        if (d + shift).abs() < 1e-12 {
            return Err("Object lies at entrance pupil".into());
        }
        let slope = radius / (d + shift);
        ([0., d * slope], [0., n0 * slope], Some([0., 0., -d]))
    } else {
        ([0., radius], [0., 0.], None)
    };
    let object = s
        .object_distance
        .map(|_| s.object_point(f, w))
        .transpose()?;
    let mut chief_height = [0.; 2];
    let mut chief_q = [0.; 2];
    for axis in 0..2 {
        let (height, slope) = if let Some(p) = object {
            let d = -p[2];
            if (d + shift).abs() < 1e-12 {
                return Err("Object lies at entrance pupil".into());
            }
            let slope = -p[axis] / (d + shift);
            (p[axis] + d * slope, slope)
        } else {
            let slope = [f.x, f.y][axis].to_radians().tan();
            (-shift * slope, slope)
        };
        chief_height[axis] = height;
        chief_q[axis] = n0 * slope;
    }
    Ok(ParaxialTrace {
        wavelength: w,
        field: [f.x, f.y],
        entrance_pupil_radius: radius,
        axis: table(s, axis_height, axis_q, axis_object, w)?,
        chief: table(s, chief_height, chief_q, object, w)?,
        model: "Linearized refraction and reflection at internal vertex planes; image row is the image vertex plane. Finite chief starts at the actual source point; axial ray starts at the source vertex. Physical apertures, field vignetting, conic and polynomial terms do not affect these tables.",
        basis: "height (mm), q = direction_sign * physical_index * (dx/dz, dy/dz); slope is global dx/dz, dy/dz",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(value: &Value) -> V3 {
        serde_json::from_value(value.clone()).unwrap()
    }
    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-10, "{a} != {b}");
    }
    #[test]
    fn normalized_inspection_matches_saved_mapping_and_vignetting() {
        for mapping in [PupilMapping::RealStop, PupilMapping::ReferenceEntrancePupil] {
            let mut s = System {
                stop_index: 1,
                pupil_mapping: mapping,
                ..Default::default()
            };
            s.fields[0].x = 1.;
            s.fields[0].y = 2.;
            s.fields[0].vignetting = [0.2, 0.1, 0.3, 0.4];
            s.update().unwrap();
            let w = 486.1327;
            let r = s.pupil_radius(w).unwrap();
            let expected = s.trace_field(0.3 * r, -0.4 * r, &s.fields[0], w).unwrap();
            let result = inspect(
                &s,
                InspectionLaunch::Pupil {
                    pupil: [0.3, -0.4],
                    field_index: 0,
                },
                w,
            )
            .unwrap();
            assert!(result["failure"].is_null());
            assert_eq!(result["ray"], serde_json::to_value(expected).unwrap());
        }
    }
    #[test]
    fn explicit_curved_source_and_glass_path_obey_snell_and_optical_path() {
        let mut s = System {
            object_distance: Some(100.),
            ..Default::default()
        };
        s.object_surface.radius = 20.;
        s.object_material = "1.500000:60.000000".into();
        s.wavelengths = vec![Wavelength::new(587.562, 1.)];
        s.reference_wavelength = 0;
        s.surfaces = vec![Surface::sphere(0., 10., "AIR", 100.)];
        s.pupil.value = 1.;
        s.update().unwrap();
        let r = inspect(
            &s,
            InspectionLaunch::Object {
                point: [1., 2.],
                tangent: [0.1, -0.05],
            },
            s.reference(),
        )
        .unwrap();
        assert!(r["failure"].is_null());
        let ray = &r["ray"];
        let source = [1., 2., -100. + 20. - 395_f64.sqrt()];
        assert_eq!(ray["points"][0], ray["object_point"]);
        for (p, s) in v(&ray["object_point"]).iter().zip(source) {
            close(*p, s);
        }
        let incoming = unit([0.1, -0.05, 1.]).unwrap();
        let segment = &ray["segments"][0];
        let outgoing = v(&segment["direction"]);
        close(outgoing[0], 1.5 * incoming[0]);
        close(outgoing[1], 1.5 * incoming[1]);
        let distance = -source[2] / incoming[2];
        close(segment["opl"].as_f64().unwrap(), 1.5 * distance);
        let first = v(&segment["point"]);
        let image = v(&ray["image_hit"]["point"]);
        close(first[0], source[0] + distance * incoming[0]);
        close(image[1], first[1] + 10. * outgoing[1] / outgoing[2]);
        close(
            ray["opl"].as_f64().unwrap(),
            1.5 * distance + 10. / outgoing[2],
        );
        close(ray["entrance_opl"].as_f64().unwrap(), 0.);
    }
    #[test]
    fn physical_failures_retain_partial_path_and_named_location() {
        let mut s = System {
            object_distance: Some(100.),
            surfaces: vec![
                Surface::sphere(0., 10., "AIR", 1.),
                Surface::sphere(0., 10., "AIR", 0.2),
            ],
            ..Default::default()
        };
        s.pupil.value = 0.2;
        s.update().unwrap();
        let launch = InspectionLaunch::Object {
            point: [0.5, 0.],
            tangent: [0., 0.],
        };
        let result = inspect(&s, launch.clone(), s.reference()).unwrap();
        assert_eq!(result["failure"]["kind"], "blocked");
        assert_eq!(result["failure"]["surface"], 1);
        assert_eq!(result["ray"]["segments"].as_array().unwrap().len(), 1);
        close(v(&result["failure"]["point"])[2], 10.);
        s.surfaces[1].clear_aperture = false;
        s.image_surface.clear_aperture = true;
        s.image_surface.aperture = 0.2;
        let result = inspect(&s, launch.clone(), s.reference()).unwrap();
        assert_eq!(result["failure"]["endpoint"], "image");
        assert_eq!(result["ray"]["segments"].as_array().unwrap().len(), 2);
        assert!(result["ray"]["image_hit"].is_null());
        s.object_surface.clear_aperture = true;
        s.object_surface.aperture = 0.1;
        let result = inspect(&s, launch, s.reference()).unwrap();
        assert_eq!(result["failure"]["endpoint"], "object");
        assert!(result["ray"]["segments"].as_array().unwrap().is_empty());
        s.object_surface.clear_aperture = false;
        s.object_material = "1.500000:60.000000".into();
        s.surfaces[0].clear_aperture = false;
        let result = inspect(
            &s,
            InspectionLaunch::Object {
                point: [0., 0.],
                tangent: [2., 0.],
            },
            587.562,
        )
        .unwrap();
        assert_eq!(result["failure"]["kind"], "total_internal_reflection");
        assert_eq!(result["failure"]["surface"], 0);
        assert!(result["ray"]["segments"].as_array().unwrap().is_empty());
        assert_eq!(result["ray"]["points"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn object_launch_validation_and_pupil_overfill_are_explicit() {
        let mut s = System::default();
        assert!(
            inspect(
                &s,
                InspectionLaunch::Object {
                    point: [0., 0.],
                    tangent: [0., 0.]
                },
                s.reference()
            )
            .unwrap_err()
            .contains("finite object")
        );
        assert!(
            inspect(
                &s,
                InspectionLaunch::Pupil {
                    pupil: [0., 0.],
                    field_index: 99
                },
                s.reference()
            )
            .is_err()
        );
        s.surfaces.iter_mut().for_each(|v| v.clear_aperture = false);
        let r = inspect(
            &s,
            InspectionLaunch::Pupil {
                pupil: [0., 1.2],
                field_index: 0,
            },
            s.reference(),
        )
        .unwrap();
        assert!(r["failure"].is_null());
        assert!(v(&r["ray"]["segments"][0]["point"])[1] > s.pupil_radius(s.reference()).unwrap());
        assert!(
            inspect(
                &s,
                InspectionLaunch::Pupil {
                    pupil: [f64::NAN, 0.],
                    field_index: 0
                },
                s.reference()
            )
            .is_err()
        );
    }
    #[test]
    fn paraxial_rows_match_independent_interface_equation_and_signed_mirror() {
        let mut s = System {
            wavelengths: vec![Wavelength::new(587.562, 1.)],
            reference_wavelength: 0,
            surfaces: vec![
                Surface::sphere(50., 5., "1.500000:60.000000", 20.),
                Surface::sphere(-50., 47., "AIR", 20.),
            ],
            ..Default::default()
        };
        s.pupil.value = 4.;
        let t = paraxial_trace(&s, &s.fields[0], s.reference()).unwrap();
        let axis = &t["axis"];
        close(axis[0]["point"][1].as_f64().unwrap(), 2.);
        close(axis[0]["reduced_out"][1].as_f64().unwrap(), -0.02);
        close(axis[0]["slope_out"][1].as_f64().unwrap(), -0.02 / 1.5);
        let height = 2. - 5. * 0.02 / 1.5;
        let q = -0.02 - 0.01 * height;
        close(axis[1]["point"][1].as_f64().unwrap(), height);
        close(axis[2]["point"][1].as_f64().unwrap(), height + 47. * q);
        s.surfaces = vec![Surface::sphere(-100., -50., "AIR", 20.)];
        s.surfaces[0].interaction = Interaction::Reflect;
        let t = paraxial_trace(&s, &Field::meridional(0.), s.reference()).unwrap();
        close(t["axis"][0]["signed_index_out"].as_f64().unwrap(), -1.);
        close(t["axis"][0]["slope_out"][1].as_f64().unwrap(), 0.04);
        close(t["axis"][1]["point"][1].as_f64().unwrap(), 0.);
        close(t["axis"][1]["point"][2].as_f64().unwrap(), -50.);
    }
    #[test]
    fn paraxial_chief_aims_at_stop_for_skew_finite_and_infinite_fields() {
        for finite in [false, true] {
            let mut s = System {
                stop_index: 1,
                object_distance: finite.then_some(200.),
                ..Default::default()
            };
            s.object_surface.radius = if finite { 100. } else { 0. };
            s.field_type = if finite {
                FieldType::ObjectHeight
            } else {
                FieldType::Angle
            };
            let mut f = Field::meridional(2.);
            f.x = 1.;
            let t = paraxial_trace(&s, &f, s.reference()).unwrap();
            let chief = t["chief"].as_array().unwrap();
            let stop = &chief[if finite { 2 } else { 1 }];
            close(stop["point"][0].as_f64().unwrap(), 0.);
            close(stop["point"][1].as_f64().unwrap(), 0.);
            let expected = s.paraxial_chief_image(&f, s.reference()).unwrap();
            let image = chief.last().unwrap();
            for axis in 0..2 {
                close(image["point"][axis].as_f64().unwrap(), expected[axis]);
            }
            assert_eq!(
                chief[0]["endpoint"],
                if finite { "object" } else { "surface" }
            );
        }
    }
}
