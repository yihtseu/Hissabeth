//! Image handedness along branches and paraxial ABCD relays.

use super::super::{
    System,
    bench::{Bench, BranchPath, BranchReport, Kind, PathBreak, Port, Relay, RelayCheckKind, Trace, cross},
    math::{Matrix, V3, dot, scale, sub, unit},
};
use serde::Serialize;
use std::collections::HashMap;

/// Transverse frame of a branch at its end: reflections fold it, transmission
/// carries it by the smallest rotation that follows the ray.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub reflections: u32,
    pub direction: V3,
    pub basis: [V3; 2],
}

impl Frame {
    /// +1 for a right-handed (u, v, d) triple, −1 for a mirrored one.
    pub fn handedness(&self) -> i8 {
        if dot(cross(self.basis[0], self.basis[1]), self.direction) >= 0. { 1 } else { -1 }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct HandednessReading {
    pub branch: String,
    pub reflections: u32,
    /// Reflection count modulo 2.
    pub parity: u32,
    /// Sign of det[u, v, d]: +1 unmirrored, −1 mirrored.
    pub handedness: i8,
    /// Exit direction and transverse basis. The input basis is u = ẑ × d
    /// (x̂ × d for a vertical ray), v = d × u, at the branch's incoming ray.
    pub direction: V3,
    pub basis: [V3; 2],
}

fn direction(trace: &Trace, index: usize) -> Option<V3> {
    let segment = trace.segments.get(index)?;
    unit(sub(segment.to, segment.from)).ok()
}

fn reflect(v: V3, n: V3) -> V3 {
    sub(v, scale(n, 2. * dot(v, n)))
}

/// Smallest rotation taking unit `a` to unit `b`, applied to `x`.
fn carry(a: V3, b: V3, x: V3) -> V3 {
    let k = cross(a, b);
    let s2 = dot(k, k);
    let c = dot(a, b);
    if s2 < 1e-24 {
        return x;
    }
    let kx = cross(k, x);
    std::array::from_fn(|i| x[i] * c + kx[i] + k[i] * dot(k, x) * (1. - c) / s2)
}

/// A single surface turns the ray along its normal: a reflection reverses the
/// normal component, a refraction keeps its sign. A composite turn (slab pass,
/// lens deflection) is a reflection when it reverses the component along any
/// of the element's surface normals.
fn is_reflection(d: V3, next: V3, normals: &[V3]) -> bool {
    let flips = |n: &V3| dot(d, *n) * dot(next, *n) < 0.;
    let Ok(turn) = unit(sub(next, d)) else { return false };
    match normals.iter().find(|n| dot(turn, **n).abs() > 1. - 1e-9) {
        Some(n) => flips(n),
        None => normals.iter().any(flips),
    }
}

/// Frame of a complete path, starting from its incoming ray at the split (or
/// its first flight at a source). A turn at an element is a reflection when it
/// reverses the ray's component along the element's optical axis (front or back
/// coating, internal substrate bounces, curved mirrors), otherwise a refraction.
pub(crate) fn frame(bench: &Bench, trace: &Trace, path: &BranchPath) -> Option<Frame> {
    let first = *path.segment_indices.first()?;
    let d = direction(trace, path.start_segment.unwrap_or(first))?;
    frames_along(bench, trace, d, &path.segment_indices)?.pop()
}

/// The frame on every segment of `indices`, starting from incoming direction `d`.
pub(crate) fn frames_along(bench: &Bench, trace: &Trace, mut d: V3, indices: &[usize]) -> Option<Vec<Frame>> {
    let mut u = unit(cross([0., 0., 1.], d)).or_else(|_| unit(cross([1., 0., 0.], d))).ok()?;
    let mut v = cross(d, u);
    let mut reflections = 0;
    let mut frames = Vec::with_capacity(indices.len());
    for &index in indices {
        let Some(next) = direction(trace, index) else {
            // A zero-length flight keeps the frame, so frames stay aligned with `indices`.
            frames.push(Frame { reflections, direction: d, basis: [u, v] });
            continue;
        };
        let flight = trace.segments[index].flight.as_ref()?;
        let normals = bench
            .elements
            .iter()
            .find(|e| e.id == flight.from_element)
            .map(|e| e.surface_normals(&flight.from_port))
            .unwrap_or_default();
        if is_reflection(d, next, &normals) {
            let n = unit(sub(next, d)).ok()?;
            u = reflect(u, n);
            v = reflect(v, n);
            reflections += 1;
        } else {
            u = carry(d, next, u);
            v = carry(d, next, v);
        }
        d = next;
        // Re-orthonormalise against drift, keeping v's side.
        u = unit(sub(u, scale(d, dot(u, d)))).ok()?;
        let w = cross(d, u);
        v = if dot(v, w) >= 0. { w } else { scale(w, -1.) };
        frames.push(Frame { reflections, direction: d, basis: [u, v] });
    }
    Some(frames)
}

fn single(report: &BranchReport) -> Option<&BranchPath> {
    match report.paths.as_slice() {
        [path] if report.interruption.is_none() && path.interruption.is_none() => Some(path),
        _ => None,
    }
}

pub(crate) fn handedness(bench: &Bench, trace: &Trace) -> Vec<HandednessReading> {
    trace
        .branches
        .iter()
        .filter_map(|report| {
            let f = frame(bench, trace, single(report)?)?;
            Some(HandednessReading {
                branch: report.id.clone(),
                reflections: f.reflections,
                parity: f.reflections % 2,
                handedness: f.handedness(),
                direction: f.direction,
                basis: f.basis,
            })
        })
        .collect()
}

fn frames(bench: &Bench, trace: &Trace, paths: [&BranchPath; 2], at: &Port) -> Result<[Frame; 2], PathBreak> {
    let [a, b] = paths.map(|path| frame(bench, trace, path));
    a.zip(b)
        .map(|(a, b)| [a, b])
        .ok_or_else(|| PathBreak::new(&at.element, &at.port, "frame_undefined"))
}

/// Reflection counts of both branches and their parity difference.
pub(crate) fn parity(bench: &Bench, trace: &Trace, paths: [&BranchPath; 2], at: &Port) -> Result<[u32; 2], PathBreak> {
    let [a, b] = frames(bench, trace, paths, at)?;
    Ok([a.reflections, b.reflections])
}

/// Signed rotation (rad, in (−π, π]) taking the second branch's exit basis to
/// the first's about their shared exit direction. Undefined between mirrored
/// images or diverging exits.
pub(crate) fn rotation(bench: &Bench, trace: &Trace, paths: [&BranchPath; 2], at: &Port) -> Result<f64, PathBreak> {
    let [a, b] = frames(bench, trace, paths, at)?;
    if dot(a.direction, b.direction) < 1. - 1e-9 {
        return Err(PathBreak::new(&at.element, &at.port, "different_exit_direction"));
    }
    if a.handedness() != b.handedness() {
        return Err(PathBreak::new(&at.element, &at.port, "opposite_handedness"));
    }
    Ok(image_rotation(&a, &b))
}

pub(crate) fn image_rotation(a: &Frame, reference: &Frame) -> f64 {
    let [ua, _] = a.basis;
    let [ub, _] = reference.basis;
    dot(cross(ub, ua), a.direction).atan2(dot(ub, ua))
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct PoweredElement {
    pub element: String,
    pub focal_mm: f64,
    /// Reduced distance from the previous powered element (or the object plane), mm.
    pub distance_mm: f64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct RelayCheckResult {
    pub kind: RelayCheckKind,
    /// B (mm) for conjugate, A for magnification, C (1/mm) for afocal, the
    /// largest spacing deviation (mm) for strict_4f.
    pub value: Option<f64>,
    pub target: f64,
    pub tolerance: f64,
    pub residual: Option<f64>,
    pub passed: bool,
    /// strict_4f: object→first − f1, first→second − (f1 + f2), second→image − f2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deviations_mm: Option<[f64; 3]>,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    pub interruption: Option<PathBreak>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct RelayReading {
    pub id: String,
    pub branch: String,
    pub wavelength_nm: Option<f64>,
    /// Object plane to image plane, [A, B, C, D] with B in mm and C in 1/mm.
    pub abcd: Option<[f64; 4]>,
    pub powered: Vec<PoweredElement>,
    /// Reduced distance from the last powered element to the image plane (mm).
    pub image_gap_mm: Option<f64>,
    /// Aperture stop and the element the conjugate pupil is measured from.
    pub pupil_stop: Option<String>,
    pub pupil_target: Option<String>,
    /// Image of the stop plane (the conjugate pupil), measured along the beam
    /// from the target element (reduced mm, positive downstream); null at
    /// infinity or when the stop is undefined.
    pub pupil_image_mm: Option<f64>,
    pub checks: Vec<RelayCheckResult>,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    pub interruption: Option<PathBreak>,
}

pub(crate) fn compose(outer: Matrix, inner: Matrix) -> Matrix {
    Matrix {
        a: outer.a * inner.a + outer.b * inner.c,
        b: outer.a * inner.b + outer.b * inner.d,
        c: outer.c * inner.a + outer.d * inner.c,
        d: outer.c * inner.b + outer.d * inner.d,
    }
}

struct Unfolded {
    total: Matrix,
    /// Object plane to each element plane in path order (after the element's
    /// own power), ending with the branch's end element.
    planes: Vec<(String, Matrix)>,
    powered: Vec<PoweredElement>,
    gap: f64,
    thick: bool,
}

/// Unfolds the branch into a straight paraxial system. Each element's power is
/// applied once, when the path reaches it (start and end elements included); flights inside a lens group are
/// replaced by the prescription matrix, and substrate passes count their
/// reduced thickness.
fn unfold(bench: &Bench, system: &System, trace: &Trace, relay: &Relay, path: &BranchPath) -> Result<Unfolded, String> {
    let mut total = Matrix::identity();
    let mut planes = Vec::new();
    let mut powered = Vec::new();
    let mut gap = relay.object_distance_mm;
    let mut thick = false;
    // The reference planes lie outside the branch's start and end elements, so
    // every element on the path, both ends included, applies its power once.
    let mut arrive = |id: &str, total: &mut Matrix, gap: &mut f64| -> Result<(), String> {
        if let Some(e) = bench.elements.iter().find(|e| e.id == id) {
            let lens = match e.kind {
                Kind::Lens | Kind::CurvedMirror => {
                    let mut m = Matrix::identity();
                    m.refract(1. / e.focal_length);
                    Some((m, e.focal_length))
                }
                Kind::LensGroup => {
                    let (m, _) = system.matrix(path.wavelength_nm, 0, system.surfaces.len())?;
                    thick = true;
                    Some((m, if m.c == 0. { f64::INFINITY } else { -1. / m.c }))
                }
                _ => None,
            };
            if let Some((m, focal)) = lens {
                *total = compose(m, *total);
                powered.push(PoweredElement { element: e.id.clone(), focal_mm: focal, distance_mm: *gap });
                *gap = 0.;
            }
        }
        planes.push((id.to_owned(), *total));
        Ok(())
    };
    total.transfer(relay.object_distance_mm, 1.);
    let mut previous: Option<&str> = None;
    for &index in &path.segment_indices {
        let flight = trace.segments[index].flight.as_ref().ok_or("segment without flight")?;
        let group = bench.elements.iter().any(|e| e.id == flight.from_element && e.kind == Kind::LensGroup);
        if group && flight.to_element == flight.from_element {
            continue;
        }
        if previous != Some(flight.from_element.as_str()) {
            arrive(&flight.from_element, &mut total, &mut gap)?;
        }
        previous = Some(&flight.from_element);
        total.transfer(flight.length_mm, flight.index);
        gap += flight.length_mm / flight.index;
    }
    if let Some(&last) = path.segment_indices.last() {
        let flight = trace.segments[last].flight.as_ref().ok_or("segment without flight")?;
        arrive(&flight.to_element, &mut total, &mut gap)?;
    }
    total.transfer(relay.image_distance_mm, 1.);
    gap += relay.image_distance_mm;
    Ok(Unfolded { total, planes, powered, gap, thick })
}

/// Conjugate image of the stop plane, measured from the target plane (mm of
/// reduced distance, positive downstream); None at infinity.
fn pupil(u: &Unfolded, relay: &Relay) -> Result<(String, String, Option<f64>), String> {
    let stop = match &relay.pupil_stop {
        Some(id) => id.clone(),
        None => u.powered.first().ok_or("no powered element for the default pupil stop")?.element.clone(),
    };
    let target = relay.pupil_target.clone().unwrap_or_else(|| u.planes.last().map(|p| p.0.clone()).unwrap_or_default());
    let s = u.planes.iter().position(|p| p.0 == stop).ok_or_else(|| format!("pupil stop {stop} is not on the branch"))?;
    let t = u.planes[s..]
        .iter()
        .position(|p| p.0 == target)
        .ok_or_else(|| format!("pupil target {target} is not at or after the stop"))?;
    let (ms, mt) = (u.planes[s].1, u.planes[s + t].1);
    // Stop plane to target plane: mt · ms⁻¹.
    let det = ms.a * ms.d - ms.b * ms.c;
    let inverse = Matrix { a: ms.d / det, b: -ms.b / det, c: -ms.c / det, d: ms.a / det };
    let m = compose(mt, inverse);
    Ok((stop, target, (m.d.abs() > 1e-12).then(|| -m.b / m.d)))
}

pub(crate) fn relays(bench: &Bench, system: &System, trace: &Trace) -> Vec<RelayReading> {
    let reports: HashMap<_, _> = trace.branches.iter().map(|r| (r.id.as_str(), r)).collect();
    bench.relays.iter().map(|relay| reading(bench, system, trace, relay, reports.get(relay.branch.as_str()).copied())).collect()
}

fn reading(bench: &Bench, system: &System, trace: &Trace, relay: &Relay, report: Option<&BranchReport>) -> RelayReading {
    let path = report.and_then(single);
    let unfolded = match (report, path) {
        (_, Some(path)) => unfold(bench, system, trace, relay, path)
            .map_err(|detail| PathBreak::new(&relay.branch, "", "relay_unresolved").with_detail(detail)),
        (Some(r), None) => Err(r
            .interruption
            .clone()
            .or_else(|| r.paths.iter().find_map(|p| p.interruption.clone()))
            .unwrap_or_else(|| PathBreak::new(&relay.branch, "", "ambiguous_start"))),
        (None, None) => Err(PathBreak::new(&relay.branch, "", "branch_not_traced")),
    };
    let u = match unfolded {
        Ok(u) => u,
        Err(interruption) => {
            return RelayReading {
                id: relay.id.clone(),
                branch: relay.branch.clone(),
                wavelength_nm: path.map(|p| p.wavelength_nm),
                abcd: None,
                powered: vec![],
                image_gap_mm: None,
                pupil_stop: None,
                pupil_target: None,
                pupil_image_mm: None,
                checks: relay
                    .checks
                    .iter()
                    .map(|check| RelayCheckResult {
                        kind: check.kind,
                        value: None,
                        target: check.target,
                        tolerance: check.tolerance,
                        residual: None,
                        passed: false,
                        deviations_mm: None,
                        interruption: Some(interruption.clone()),
                    })
                    .collect(),
                interruption: Some(interruption),
            };
        }
    };
    let m = u.total;
    let (pupil_stop, pupil_target, pupil_image_mm, pupil_break) = match pupil(&u, relay) {
        Ok((stop, target, z)) => (Some(stop), Some(target), z, None),
        Err(detail) => (None, None, None, Some(PathBreak::new(&relay.branch, "", "pupil_undefined").with_detail(detail))),
    };
    let checks = relay
        .checks
        .iter()
        .map(|check| {
            let (value, deviations, interruption) = match check.kind {
                RelayCheckKind::Conjugate => (Some(m.b), None, None),
                RelayCheckKind::Magnification => (Some(m.a), None, None),
                RelayCheckKind::Afocal => (Some(m.c), None, None),
                RelayCheckKind::Strict4f => match u.powered.as_slice() {
                    [first, second] if !u.thick => {
                        let (f1, f2) = (first.focal_mm, second.focal_mm);
                        let d = [first.distance_mm - f1, second.distance_mm - (f1 + f2), u.gap - f2];
                        (Some(d.iter().fold(0f64, |acc, x| acc.max(x.abs()))), Some(d), None)
                    }
                    _ => (None, None, Some(PathBreak::new(&relay.branch, "", "strict_4f_needs_two_thin_elements"))),
                },
            };
            let residual = value.map(|v| v - check.target);
            RelayCheckResult {
                kind: check.kind,
                value,
                target: check.target,
                tolerance: check.tolerance,
                residual,
                passed: residual.is_some_and(|r| r.abs() <= check.tolerance),
                deviations_mm: deviations,
                interruption,
            }
        })
        .collect();
    RelayReading {
        id: relay.id.clone(),
        branch: relay.branch.clone(),
        wavelength_nm: path.map(|p| p.wavelength_nm),
        abcd: Some([m.a, m.b, m.c, m.d]),
        image_gap_mm: (!u.powered.is_empty()).then_some(u.gap),
        powered: u.powered,
        pupil_stop,
        pupil_target,
        pupil_image_mm,
        checks,
        // A missing default stop (no powered element) only leaves the pupil unset.
        interruption: pupil_break.filter(|_| relay.pupil_stop.is_some() || relay.pupil_target.is_some()),
    }
}
