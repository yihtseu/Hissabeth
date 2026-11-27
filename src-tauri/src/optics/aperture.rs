// SPDX-License-Identifier: GPL-3.0-or-later
// Drawing envelopes and directional vignetting use separate aperture semantics.
use super::{
    trace::{FailureKind, TraceFailure, TraceOptions},
    *,
};

#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct SurfaceSize {
    pub surface: usize,
    pub semi_diameter: f64,
    pub aperture: f64,
    pub clear_aperture: bool,
    pub reached_rays: usize,
}
#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct EndpointSize {
    pub semi_diameter: f64,
    pub aperture: f64,
    pub clear_aperture: bool,
    pub reached_rays: usize,
}
impl EndpointSize {
    fn new(surface: &EndpointSurface) -> Self {
        Self {
            semi_diameter: 0.,
            aperture: surface.aperture,
            clear_aperture: surface.clear_aperture,
            reached_rays: 0,
        }
    }
    fn include(&mut self, point: V3) {
        self.semi_diameter = self.semi_diameter.max(point[0].hypot(point[1]));
        self.reached_rays += 1;
    }
}
#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct CoverageFailure {
    pub field_index: usize,
    pub pupil: [f64; 2],
    pub surface: Option<usize>,
    pub reached_surfaces: usize,
    pub endpoint: Option<&'static str>,
    pub kind: String,
    pub error: String,
}
#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct ApertureReport {
    pub wavelength: f64,
    pub sampling: &'static str,
    pub sampled_rays: usize,
    pub failed_rays: usize,
    pub surfaces: Vec<SurfaceSize>,
    pub object: EndpointSize,
    pub image: EndpointSize,
    pub failures: Vec<CoverageFailure>,
}
fn accumulate(ray: &Ray, sizes: &mut [SurfaceSize], image: &mut EndpointSize) {
    if let Some(hit) = &ray.image_hit {
        image.include(hit.point);
    }
    for (surface, segment) in sizes.iter_mut().zip(&ray.segments) {
        surface.semi_diameter = surface
            .semi_diameter
            .max(segment.point[0].hypot(segment.point[1]));
        surface.reached_rays += 1;
    }
}
pub fn semi_diameters(s: &System) -> Result<ApertureReport, String> {
    let w = s.reference();
    let radius = s.pupil_radius(w)?;
    let mut surfaces = s
        .surfaces
        .iter()
        .enumerate()
        .map(|(surface, s)| SurfaceSize {
            surface,
            semi_diameter: 0.,
            aperture: s.aperture,
            clear_aperture: s.clear_aperture,
            reached_rays: 0,
        })
        .collect::<Vec<_>>();
    let mut failures = vec![];
    let mut object = EndpointSize::new(&s.object_surface);
    let mut image = EndpointSize::new(&s.image_surface);
    for (field_index, field) in s.fields.iter().enumerate() {
        let frame = s.field_frame(field);
        let pupils = [
            [0., 0.],
            frame.tangential,
            frame.tangential.map(|v| -v),
            frame.sagittal,
            frame.sagittal.map(|v| -v),
        ];
        for pupil in pupils {
            if s.object_distance.is_some() {
                if let Ok(point) = s.object_point(field, w) {
                    object.include(point);
                }
            }
            let options = TraceOptions {
                last: s.surfaces.len() - 1,
                image: true,
                apertures: false,
                vignetting: true,
            };
            match s.trace_options(radius * pupil[0], radius * pupil[1], field, w, options) {
                Ok(ray) => accumulate(&ray, &mut surfaces, &mut image),
                Err(failure) => {
                    let mut reached = 0;
                    if let Some(ray) = &failure.ray {
                        accumulate(ray, &mut surfaces, &mut image);
                        reached = ray.segments.len();
                    }
                    if let (Some(i), Some(point)) = (failure.surface, failure.point) {
                        if i >= reached && i < surfaces.len() {
                            surfaces[i].semi_diameter =
                                surfaces[i].semi_diameter.max(point[0].hypot(point[1]));
                            surfaces[i].reached_rays += 1;
                            reached = i + 1;
                        }
                    }
                    let local_pupil = [
                        FieldFrame::project(frame.tangential, pupil),
                        FieldFrame::project(frame.sagittal, pupil),
                    ];
                    failures.push(CoverageFailure {
                        field_index,
                        pupil: local_pupil,
                        surface: failure.surface,
                        reached_surfaces: reached,
                        endpoint: failure.endpoint,
                        kind: failure.kind.as_str().into(),
                        error: failure.message,
                    });
                }
            }
        }
    }
    Ok(ApertureReport {
        wavelength: w,
        sampling: "chief and four field-local pupil rim rays; failure pupil coordinates are normalized p_T/p_S; physical clipping ignored",
        sampled_rays: 5 * s.fields.len(),
        failed_rays: failures.len(),
        surfaces,
        object,
        image,
        failures,
    })
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct Boundary {
    pub direction: &'static str,
    pub pupil_scale: f64,
    pub limiting_surface: Option<usize>,
    pub limiting_endpoint: Option<&'static str>,
    pub status: &'static str,
}
#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct FieldVignetting {
    pub field_index: usize,
    pub vignetting: [f64; 4],
    pub boundaries: Vec<Boundary>,
}
#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct VignettingReport {
    pub wavelength: f64,
    pub iterations: usize,
    pub tolerance: f64,
    pub fields: Vec<FieldVignetting>,
}
const FACTOR_TOLERANCE: f64 = 1e-9;
const DIRECTIONS: [(&str, [f64; 2]); 4] = [
    ("upper_y", [0., 1.]),
    ("lower_y", [0., -1.]),
    ("upper_x", [1., 0.]),
    ("lower_x", [-1., 0.]),
];
fn probe(
    s: &System,
    field: &Field,
    direction: [f64; 2],
    pupil_scale: f64,
    radius: f64,
    w: f64,
) -> Result<Ray, TraceFailure> {
    s.trace_options(
        radius * pupil_scale * direction[0],
        radius * pupil_scale * direction[1],
        field,
        w,
        TraceOptions {
            last: s.surfaces.len() - 1,
            image: true,
            apertures: true,
            vignetting: false,
        },
    )
}
fn boundary(
    s: &System,
    field: &Field,
    direction: &'static str,
    vector: [f64; 2],
    radius: f64,
    w: f64,
) -> Result<(f64, Boundary), String> {
    let (mut lower, mut upper) = (0., 1.);
    let mut failed = None;
    for _ in 0..64 {
        match probe(s, field, vector, upper, radius, w) {
            Ok(_) => {
                lower = upper;
                upper *= 2.;
            }
            Err(error) => {
                failed = Some(error);
                break;
            }
        }
    }
    let mut failed =
        failed.ok_or_else(|| format!("{direction}: no finite aperture boundary found"))?;
    for _ in 0..80 {
        if upper - lower <= 1e-10 * upper {
            break;
        }
        let middle = (lower + upper) / 2.;
        match probe(s, field, vector, middle, radius, w) {
            Ok(_) => lower = middle,
            Err(error) => {
                upper = middle;
                failed = error;
            }
        }
    }
    if !matches!(failed.kind, FailureKind::Blocked) {
        return Err(format!(
            "{direction}: ray geometry limits the pupil before an aperture boundary: {failed}"
        ));
    }
    let mut factor = 1. - lower;
    // Quantize inward so storing 1-scale does not move a tightly clipped ray outside.
    if 1. - factor > lower {
        factor = factor.next_up();
    }
    if !factor.is_finite() || factor >= 1. {
        return Err(format!(
            "{direction}: no numerically resolved transmitting pupil"
        ));
    }
    probe(s, field, vector, 1. - factor, radius, w)
        .map_err(|e| format!("{direction}: stored factor does not transmit: {e}"))?;
    Ok((
        factor,
        Boundary {
            direction,
            pupil_scale: 1. - factor,
            limiting_surface: failed.surface,
            limiting_endpoint: failed.endpoint,
            status: "aperture",
        },
    ))
}
fn calculate(s: &System) -> Result<Vec<FieldVignetting>, String> {
    let w = s.reference();
    let radius = s.pupil_radius(w)?;
    let clipped =
        s.image_surface.clear_aperture || s.surfaces.iter().any(|surface| surface.clear_aperture);
    let mut fields = vec![];
    for (field_index, field) in s.fields.iter().enumerate() {
        let mut vignetting = [0.; 4];
        let mut boundaries = vec![];
        if clipped || s.object_surface.clear_aperture {
            probe(s, field, [0., 0.], 0., radius, w).map_err(|e| format!("Field {field_index}: chief ray cannot define a connected transmitting pupil: {e}"))?;
        }
        if clipped {
            for (i, (name, direction)) in DIRECTIONS.into_iter().enumerate() {
                let (factor, edge) = boundary(s, field, name, direction, radius, w)
                    .map_err(|e| format!("Field {field_index}: {e}"))?;
                vignetting[i] = factor;
                boundaries.push(edge);
            }
        } else {
            boundaries.extend(DIRECTIONS.iter().map(|(direction, _)| Boundary {
                direction: *direction,
                pupil_scale: 1.,
                limiting_surface: None,
                limiting_endpoint: None,
                status: "unlimited",
            }));
        }
        fields.push(FieldVignetting {
            field_index,
            vignetting,
            boundaries,
        });
    }
    Ok(fields)
}
pub fn update_vignetting(system: &System) -> Result<(System, VignettingReport), String> {
    let mut next = system.clone();
    for iteration in 1..=64 {
        let mut fields = calculate(&next)?;
        let error = fields
            .iter()
            .zip(&next.fields)
            .flat_map(|(new, old)| {
                new.vignetting
                    .into_iter()
                    .zip(old.vignetting)
                    .map(|(a, b)| (a - b).abs() / (1. + a.abs()))
            })
            .fold(0_f64, f64::max);
        // A small fixed-point residual alone does not guarantee that the stored rim
        // rays still pass after geometry-dependent gap relations have moved.
        let rims_transmit = if error <= FACTOR_TOLERANCE {
            let w = next.reference();
            let r = next.pupil_radius(w)?;
            !(next.image_surface.clear_aperture
                || next.surfaces.iter().any(|surface| surface.clear_aperture))
                || next.fields.iter().all(|field| {
                    DIRECTIONS.into_iter().all(|(_, direction)| {
                        next.trace_options(
                            r * direction[0],
                            r * direction[1],
                            field,
                            w,
                            TraceOptions {
                                last: next.surfaces.len() - 1,
                                image: true,
                                apertures: true,
                                vignetting: true,
                            },
                        )
                        .is_ok()
                    })
                })
        } else {
            false
        };
        if error <= FACTOR_TOLERANCE && rims_transmit {
            for (report, stored) in fields.iter_mut().zip(&next.fields) {
                report.vignetting = stored.vignetting;
                for (edge, factor) in report.boundaries.iter_mut().zip(stored.vignetting) {
                    edge.pupil_scale = 1. - factor;
                }
            }
            let report = VignettingReport {
                wavelength: next.reference(),
                iterations: iteration,
                tolerance: FACTOR_TOLERANCE,
                fields,
            };
            return Ok((next, report));
        }
        for (field, computed) in next.fields.iter_mut().zip(fields) {
            for (old, new) in field.vignetting.iter_mut().zip(computed.vignetting) {
                *old = if iteration == 1 {
                    new
                } else {
                    (*old + new) / 2.
                };
            }
        }
        next.update()?;
    }
    Err("Vignetting and gap solves did not converge together within 64 iterations".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn planes(radius: f64, pupil: f64) -> System {
        let mut s = System::default();
        s.surfaces = vec![
            Surface::sphere(0., 10., "AIR", 1.),
            Surface::sphere(0., 10., "AIR", radius),
        ];
        s.surfaces[0].clear_aperture = false;
        s.fields = vec![Field::meridional(0.1_f64.atan().to_degrees())];
        s.pupil.value = pupil;
        s.update().unwrap();
        s
    }
    #[test]
    fn directional_boundaries_match_plane_geometry_and_preserve_clear_radii() {
        let s = planes(2., 10.);
        let (next, report) = update_vignetting(&s).unwrap();
        let expected = [0.8, 0.4, 1. - 3_f64.sqrt() / 5., 1. - 3_f64.sqrt() / 5.];
        for (actual, expected) in next.fields[0].vignetting.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 2e-9, "{actual} vs {expected}");
        }
        assert!(
            report.fields[0]
                .boundaries
                .iter()
                .all(|b| b.limiting_surface == Some(1))
        );
        assert_eq!(next.surfaces[1].aperture, 2.);
        assert_eq!(s.fields[0].vignetting, [0.; 4]);
        let sizes = semi_diameters(&s).unwrap();
        assert!((sizes.surfaces[1].semi_diameter - 6.).abs() < 1e-8);
        assert_eq!(sizes.failed_rays, 0);
        assert!(next.surfaces[1].semi_diameter < s.surfaces[1].semi_diameter);
    }
    #[test]
    fn enlarged_pupil_has_no_negative_floor_and_unlimited_resets_factors() {
        let s = planes(10., 4.);
        let (mut next, _) = update_vignetting(&s).unwrap();
        let expected = [-3.5, -4.5, 1. - 99_f64.sqrt() / 2., 1. - 99_f64.sqrt() / 2.];
        for (a, b) in next.fields[0].vignetting.into_iter().zip(expected) {
            assert!((a - b).abs() < 3e-9);
        }
        next.surfaces[1].clear_aperture = false;
        next.update().unwrap();
        let (next, report) = update_vignetting(&next).unwrap();
        assert_eq!(next.fields[0].vignetting, [0.; 4]);
        assert!(
            report.fields[0]
                .boundaries
                .iter()
                .all(|b| b.status == "unlimited")
        );
    }
    #[test]
    fn blocked_chief_and_geometry_boundaries_reject_without_mutation() {
        let s = planes(0.5, 4.);
        let before = serde_json::to_value(&s).unwrap();
        assert!(update_vignetting(&s).unwrap_err().contains("chief ray"));
        assert_eq!(serde_json::to_value(&s).unwrap(), before);
        let mut s = System::default();
        s.fields = vec![Field::meridional(0.)];
        s.surfaces[0].clear_aperture = false;
        s.surfaces[1].aperture = 40.;
        s.update().unwrap();
        let error = update_vignetting(&s).unwrap_err();
        assert!(error.contains("geometry"), "{error}");
    }
    #[test]
    fn failed_drawing_rays_keep_reachable_prefix() {
        let mut s = planes(2., 10.);
        s.fields = vec![Field::meridional(0.)];
        s.surfaces[1].radius = 1.;
        s.surfaces[1].clear_aperture = false;
        s.update().unwrap();
        let report = semi_diameters(&s).unwrap();
        assert_eq!(report.failed_rays, 4);
        assert_eq!(report.surfaces[0].reached_rays, 5);
        assert_eq!(report.surfaces[1].reached_rays, 1);
        assert_eq!(report.surfaces[1].semi_diameter, 0.);
        assert!(
            report
                .failures
                .iter()
                .all(|f| f.surface == Some(1) && f.reached_surfaces == 1)
        );
    }
    #[test]
    fn vignetting_and_marginal_gap_feedback_converge_with_transmitting_rims() {
        let mut s = planes(2., 10.);
        s.surfaces[0].gap_solve = solve::GapSolve::MarginalHeight {
            value: 1.,
            zone: 0.1,
        };
        s.update().unwrap();
        let (next, report) = update_vignetting(&s).unwrap();
        assert!(report.iterations > 2);
        assert!((next.surfaces[0].thickness - 80. / 9.).abs() < 1e-7);
        let r = next.pupil_radius(next.reference()).unwrap();
        for (_, dir) in DIRECTIONS {
            next.trace_field(r * dir[0], r * dir[1], &next.fields[0], next.reference())
                .unwrap();
        }
        let mut again = next.clone();
        again.update().unwrap();
        assert!((again.surfaces[0].thickness - next.surfaces[0].thickness).abs() < 1e-9);
    }
}
