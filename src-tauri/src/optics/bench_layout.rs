//! Height layers, periscopes, zones and folds read off a traced bench.

use super::super::{
    Surface, System,
    bench::{Bench, BranchReport, Element, Kind, PathBreak, Port, Segment, Trace, inside_polygon, local, polygon_area, rotate},
    math::{V3, add, dot, norm, scale, sub, unit},
};
use serde::Serialize;
use std::collections::HashSet;

/// Beam clearance around one periscope: its vertical leg and the leg leaving the
/// upper mirror, against every other element's optical body and mount.
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct PeriscopeReading {
    pub id: String,
    /// Height (table-frame Z, mm) and unit direction of the beam leaving the upper mirror.
    pub exit_height_mm: Option<f64>,
    pub exit_direction: Option<V3>,
    /// Smallest gap (mm) between the two legs' axes (every centre ray) and another
    /// element's optical body or mount box. The body is the exact substrate slab
    /// (wedge included), each glass piece of a lens group between its curved
    /// faces (air gaps free), or a flat disc of radius `body_radius`. A leg the
    /// tracer stops on an element counts 0. Region paths no ray lights are skipped.
    pub min_clearance_mm: Option<f64>,
    pub nearest_element: Option<String>,
    /// Elements closer to a leg's axis than that leg's source's geometric beam radius.
    pub conflicts: Vec<String>,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    pub interruption: Option<PathBreak>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct ZoneReading {
    pub id: String,
    pub label: String,
    pub area_mm2: f64,
    /// Table-frame XY bounding box: `[min, max]`, each `[x, y]` (mm).
    pub bounds_mm: [[f64; 2]; 2],
    /// Declared members, or the elements centred in the zone when none are declared.
    pub members: Vec<String>,
    /// Members whose footprint (mount base, else the body radius square) leaves the zone.
    pub out_of_bounds: Vec<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct FoldReading {
    pub id: String,
    /// The elements the fold starts and ends at.
    pub from: String,
    pub to: String,
    pub paths: Vec<FoldPath>,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    pub interruption: Option<PathBreak>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct FoldPath {
    pub source: String,
    pub wavelength_nm: f64,
    /// Geometric propagation length summed over the traced segments (mm).
    pub length_mm: f64,
    pub opl_mm: f64,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    pub interruption: Option<PathBreak>,
}

pub(crate) fn folds(bench: &Bench, trace: &Trace) -> Vec<FoldReading> {
    trace.folds.iter().map(|report| fold(bench, report)).collect()
}

fn fold(bench: &Bench, report: &BranchReport) -> FoldReading {
    let declared = bench.folds.iter().find(|f| f.id == report.id);
    FoldReading {
        id: report.id.clone(),
        from: declared.map(|f| f.from.element.clone()).unwrap_or_default(),
        to: declared.map(|f| f.to.element.clone()).unwrap_or_default(),
        paths: report.paths.iter().map(|p| FoldPath {
            source: p.source.clone(),
            wavelength_nm: p.wavelength_nm,
            length_mm: p.length_mm,
            opl_mm: p.opl_mm,
            interruption: p.interruption.clone(),
        }).collect(),
        interruption: report.interruption.clone(),
    }
}

/// Minimum of a convex function on [0, 1] by ternary search.
fn convex_min(f: impl Fn(f64) -> f64) -> f64 {
    let (mut a, mut b) = (0., 1.);
    for _ in 0..200 {
        let (m1, m2) = (a + (b - a) / 3., b - (b - a) / 3.);
        if f(m1) <= f(m2) {
            b = m2;
        } else {
            a = m1;
        }
    }
    f(0.).min(f(1.)).min(f((a + b) / 2.))
}

/// A substrate's slab, exactly the tracer's two faces (wedge included), in the
/// element's local frame (optical axis +x):
/// the cylinder `y² + z² ≤ radius²` cut by the front plane `x ≤ front` and the
/// back half-space `back_normal · (q − back) ≥ 0`. `None` for a bare disc.
struct Solid {
    radius: f64,
    front: f64,
    back: V3,
    back_normal: V3,
}

impl Solid {
    fn of(e: &Element) -> Option<Self> {
        let substrate = e.substrate.as_ref()?;
        let wedge = substrate.wedge_deg.unwrap_or(0.).to_radians();
        Some(Self {
            radius: e.body_radius,
            front: 0.,
            back: [-substrate.thickness_mm, 0., 0.],
            back_normal: [wedge.cos(), wedge.sin(), 0.],
        })
    }

    fn contains(&self, q: V3, tol: f64) -> bool {
        q[0] <= self.front + tol
            && dot(sub(q, self.back), self.back_normal) >= -tol
            && q[1].hypot(q[2]) <= self.radius + tol
    }

    /// Distance from a local point: the projection lies on the intersection of
    /// some set of active constraints, so it is the nearest feasible point among
    /// the projections onto each face, edge and corner.
    fn distance(&self, q: V3) -> f64 {
        let (r, n, b) = (self.radius, self.back_normal, self.back);
        let tol = 1e-9 * (1. + r + self.front.abs() + norm(b));
        if self.contains(q, 0.) {
            return 0.;
        }
        let rho = q[1].hypot(q[2]);
        let rim = |x: f64| if rho > 0. { [x, r * q[1] / rho, r * q[2] / rho] } else { [x, r, 0.] };
        let mut candidates = vec![
            [self.front, q[1], q[2]],
            sub(q, scale(n, dot(sub(q, b), n))),
            rim(q[0]),
            rim(self.front),
        ];
        // Back face rim: the ellipse where the back plane meets the cylinder.
        if n[0].abs() > 1e-12 {
            let at = |t: f64| {
                let (c, s) = (t.cos(), t.sin());
                [b[0] - n[1] / n[0] * (r * c - b[1]), r * c, r * s]
            };
            let gap = |t: f64| norm(sub(at(t), q));
            let steps = 256;
            let best = (0..steps).map(|i| std::f64::consts::TAU * f64::from(i) / f64::from(steps))
                .min_by(|a, b| gap(*a).total_cmp(&gap(*b)))
                .unwrap_or_default();
            let h = std::f64::consts::TAU / f64::from(steps);
            let (mut lo, mut hi) = (best - h, best + h);
            for _ in 0..100 {
                let (m1, m2) = (lo + (hi - lo) / 3., hi - (hi - lo) / 3.);
                if gap(m1) <= gap(m2) { hi = m2 } else { lo = m1 }
            }
            candidates.push(at((lo + hi) / 2.));
        }
        // Edge where the front and back planes meet (x = front), and its corners on the cylinder.
        if n[1].abs() > 1e-12 {
            let y = b[1] - n[0] * (self.front - b[0]) / n[1];
            candidates.push([self.front, y, q[2]]);
            if y.abs() <= r {
                let z = (r * r - y * y).sqrt();
                candidates.extend([[self.front, y, z], [self.front, y, -z]]);
            }
        }
        candidates.into_iter()
            .filter(|c| self.contains(*c, tol))
            .map(|c| norm(sub(c, q)))
            .fold(f64::INFINITY, f64::min)
    }
}

/// One glass piece of a lens group: the points within `radius` of the local x
/// axis with `front(y, z) ≤ x ≤ back(y, z)`, each face a prescription surface's
/// sag from its vertex (`None`: flat). The tracer's surface frame is
/// (local y, local z, local x), so a surface's sag at (u, v) lies at local y = u, z = v.
struct Glass {
    radius: f64,
    front: (f64, Option<Surface>),
    back: (f64, Option<Surface>),
}

impl Glass {
    /// One piece per glass gap (a surface whose following medium is not `AIR`),
    /// between that surface and the next; air gaps are free.
    fn pieces(e: &Element, system: &System) -> Vec<Self> {
        let surfaces = &system.surfaces;
        let mut vertex = 0.;
        let mut out = vec![];
        for (i, surface) in surfaces.iter().enumerate() {
            let next = vertex + surface.thickness;
            if surface.material != "AIR" {
                out.push(Self {
                    radius: e.body_radius,
                    front: (vertex, Some(surface.clone())),
                    back: (next, surfaces.get(i + 1).cloned()),
                });
            }
            vertex = next;
        }
        out
    }

    /// Both faces turned about the axis (spheres, conics, even aspheres, radial
    /// Zernike terms): the body is then a solid of revolution.
    fn turned(&self) -> bool {
        [&self.front, &self.back].iter().all(|f| f.1.as_ref().is_none_or(|s| s.profile().is_radial()))
    }

    /// Every face is defined over the whole body disc (checked on the polar grid
    /// and the rim). Where a surface's real domain ends inside the disc (a sphere
    /// or cylinder narrower than the body), the glass would gain side walls that
    /// the face and rim search does not see, so such a piece is not measured.
    fn defined(&self) -> bool {
        let grid = (0..=48).flat_map(|i| (0..96).map(move |j| (i, j))).map(|(i, j)| {
            let (rho, angle) = (self.radius * f64::from(i) / 48., std::f64::consts::TAU * f64::from(j) / 96.);
            (rho * angle.cos(), rho * angle.sin())
        });
        let rim = (0..512).map(|j| {
            let angle = std::f64::consts::TAU * f64::from(j) / 512.;
            (self.radius * angle.cos(), self.radius * angle.sin())
        });
        grid.chain(rim).all(|(y, z)| Self::face(&self.front, y, z).is_some() && Self::face(&self.back, y, z).is_some())
    }

    /// Face position at transverse (y, z); `None` outside the surface's domain.
    fn face(face: &(f64, Option<Surface>), y: f64, z: f64) -> Option<f64> {
        match &face.1 {
            Some(surface) => surface.profile().sag2(y, z).ok().map(|(x, _, _)| face.0 + x),
            None => Some(face.0),
        }
    }

    /// Axial extent of the glass at transverse (y, z), where it exists.
    fn span(&self, y: f64, z: f64) -> Option<(f64, f64)> {
        let (lo, hi) = (Self::face(&self.front, y, z)?, Self::face(&self.back, y, z)?);
        (lo <= hi).then_some((lo, hi))
    }

    /// Distance from a local point to the glass: zero inside, else the nearest of
    /// the two faces and the rim.
    fn distance(&self, q: V3) -> f64 {
        if q[1].hypot(q[2]) <= self.radius && self.span(q[1], q[2]).is_some_and(|(lo, hi)| lo <= q[0] && q[0] <= hi) {
            0.
        } else if self.turned() {
            self.turned_distance(q)
        } else {
            self.general_distance(q)
        }
    }

    /// Solid of revolution: the nearest glass lies in the point's own meridional
    /// half-plane, so this is the 2D distance from (h, r) to the region between the
    /// two face curves and the rim.
    fn turned_distance(&self, q: V3) -> f64 {
        let (h, r) = (q[0], q[1].hypot(q[2]));
        let mut best = f64::INFINITY;
        for side in [0, 1] {
            let curve = |rho: f64| {
                self.span(rho, 0.).map_or(f64::INFINITY, |span| {
                    (if side == 0 { span.0 } else { span.1 } - h).hypot(rho - r)
                })
            };
            best = best.min(sampled_min(curve, 0., self.radius, 512));
        }
        if let Some((lo, hi)) = self.span(self.radius, 0.) {
            best = best.min((lo - h).max(h - hi).max(0.).hypot(r - self.radius));
        }
        best
    }

    /// Any prescription (cylinders, toroids, non-radial Zernike terms): each face
    /// is minimised over the disc on a polar grid refined by compass search, and
    /// the rim over its angle.
    fn general_distance(&self, q: V3) -> f64 {
        let mut best = f64::INFINITY;
        for side in [0, 1] {
            let at = |y: f64, z: f64| {
                if y.hypot(z) > self.radius {
                    return f64::INFINITY;
                }
                self.span(y, z).map_or(f64::INFINITY, |span| {
                    norm(sub([if side == 0 { span.0 } else { span.1 }, y, z], q))
                })
            };
            best = best.min(disc_min(at, self.radius));
        }
        let rim = |angle: f64| {
            let (y, z) = (self.radius * angle.cos(), self.radius * angle.sin());
            self.span(y, z).map_or(f64::INFINITY, |(lo, hi)| {
                norm(sub([q[0].clamp(lo, hi), y, z], q))
            })
        };
        best.min(sampled_min(rim, 0., std::f64::consts::TAU, 512))
    }
}

/// Minimum of `f(y, z)` over the disc of `radius`: the best of a polar grid,
/// refined by compass search shrinking from the grid spacing.
fn disc_min(f: impl Fn(f64, f64) -> f64, radius: f64) -> f64 {
    let (rings, spokes) = (48, 96);
    let mut best = (f(0., 0.), 0., 0.);
    for i in 1..=rings {
        let rho = radius * f64::from(i) / f64::from(rings);
        for j in 0..spokes {
            let angle = std::f64::consts::TAU * f64::from(j) / f64::from(spokes);
            let (y, z) = (rho * angle.cos(), rho * angle.sin());
            let value = f(y, z);
            if value < best.0 {
                best = (value, y, z);
            }
        }
    }
    let mut step = radius / f64::from(rings);
    while step > radius * 1e-13 {
        let moved = [(step, 0.), (-step, 0.), (0., step), (0., -step)].into_iter()
            .map(|(dy, dz)| (f(best.1 + dy, best.2 + dz), best.1 + dy, best.2 + dz))
            .find(|c| c.0 < best.0);
        match moved {
            Some(c) => best = c,
            None => step /= 2.,
        }
    }
    best.0
}

/// Minimum of `f` on [a, b]: the best of `n` samples, refined by ternary search
/// between its neighbours.
fn sampled_min(f: impl Fn(f64) -> f64, a: f64, b: f64, n: u32) -> f64 {
    let step = (b - a) / f64::from(n);
    let at = |i: u32| a + step * f64::from(i);
    let best = (0..=n).min_by(|i, j| f(at(*i)).total_cmp(&f(at(*j)))).unwrap_or(0);
    let (mut lo, mut hi) = (at(best.saturating_sub(1)), at((best + 1).min(n)));
    for _ in 0..100 {
        let (m1, m2) = (lo + (hi - lo) / 3., hi - (hi - lo) / 3.);
        if f(m1) <= f(m2) { hi = m2 } else { lo = m1 }
    }
    f(at(best)).min(f((lo + hi) / 2.))
}

/// Distance from a point to a disc (centre, unit normal, radius).
fn disc_distance(p: V3, centre: V3, normal: V3, radius: f64) -> f64 {
    let r = sub(p, centre);
    let h = dot(r, normal);
    let planar = norm(sub(r, scale(normal, h)));
    if planar <= radius { h.abs() } else { h.hypot(planar - radius) }
}

/// Distance from a point to an element's upright, yaw-rotated mount box.
fn mount_distance(e: &Element, p: V3, table_z: f64) -> Option<f64> {
    let m = e.mount.as_ref()?;
    let top = e.position[2] - table_z - e.body_radius;
    if top <= 0. {
        return None;
    }
    let q = local(sub(p, [e.position[0], e.position[1], table_z]), [0., 0., e.rotation()[2]]);
    let low = [-m.base_size[0] / 2., -m.base_size[1] / 2., 0.];
    let high = [m.base_size[0] / 2., m.base_size[1] / 2., top];
    let gap: Vec<f64> = (0..3).map(|i| (low[i] - q[i]).max(q[i] - high[i]).max(0.)).collect();
    Some(norm([gap[0], gap[1], gap[2]]))
}

/// A lens group with a glass piece the clearance cannot measure ([`Glass::defined`]).
fn unmeasured(system: &System, e: &Element) -> bool {
    e.kind == Kind::LensGroup && Glass::pieces(e, system).iter().any(|piece| !piece.defined())
}

/// Gap between segment `a`–`b` and element `e`; the shared-post mount of a
/// periscope's upper mirror is not a separate obstacle. `receiver` leaves out the
/// optical body of the element the leg is meant to land on (its mount still counts).
fn clearance(bench: &Bench, system: &System, e: &Element, a: V3, b: V3, receiver: bool) -> f64 {
    let at = |t: f64| add(a, scale(sub(b, a), t));
    let body = if receiver {
        f64::INFINITY
    } else if e.kind == Kind::LensGroup {
        // Glass between curved faces need not be convex: sample along the leg.
        let to_local = |t: f64| local(sub(at(t), e.position), e.rotation());
        Glass::pieces(e, system).iter()
            .map(|piece| sampled_min(|t| piece.distance(to_local(t)), 0., 1., 256))
            .fold(f64::INFINITY, f64::min)
    } else if let Some(slab) = Solid::of(e) {
        convex_min(|t| slab.distance(local(sub(at(t), e.position), e.rotation())))
    } else {
        convex_min(|t| disc_distance(at(t), e.position, rotate([1., 0., 0.], e.rotation()), e.body_radius))
    };
    let post = !bench.periscopes.iter().any(|p| p.upper == e.id);
    let mount = if post && e.mount.is_some() {
        convex_min(|t| mount_distance(e, at(t), bench.origin[2]).unwrap_or(f64::INFINITY))
    } else {
        f64::INFINITY
    };
    body.min(mount)
}

/// `centre` must be a centre trace (no rings, scatter or ghosts), so every segment
/// leaving a periscope mirror is the axis of one beam; `full` is the bench's own
/// trace, which says which beams carry power: a centre leg whose beam (source and
/// upstream route) is lit there counts even if its own centre ray is a zero-power
/// region probe, because sampled rays light that region.
pub(crate) fn periscopes(bench: &Bench, system: &System, centre: &Trace, full: &Trace) -> Vec<PeriscopeReading> {
    let lit: HashSet<(&str, &[Port])> = full.segments.iter()
        .filter(|s| s.power > 0.)
        .map(|s| (s.source.as_str(), s.beam.as_slice()))
        .collect();
    let trace = centre;
    bench.periscopes.iter().map(|p| {
        // Every lit beam's centre ray (each source, each split path) leaving the
        // mirror's reflected port; an unlit region's geometric probe is no beam.
        let legs = |element: &str| -> Vec<&Segment> {
            trace.segments.iter().filter(|s| {
                s.flight.as_ref().is_some_and(|f| f.from_element == element && f.from_port == "reflected")
                    && s.status != "substrate"
                    && (s.power > 0. || lit.contains(&(s.source.as_str(), s.beam.as_slice())))
            }).collect()
        };
        let (rises, exits) = (legs(&p.lower), legs(&p.upper));
        let mut reading = PeriscopeReading {
            id: p.id.clone(),
            exit_height_mm: exits.first().map(|s| s.from[2]),
            exit_direction: exits.first().and_then(|s| unit(sub(s.to, s.from)).ok()),
            min_clearance_mm: None,
            nearest_element: None,
            conflicts: vec![],
            interruption: None,
        };
        let reached = |s: &Segment, to: &str| {
            s.status == "hit" && s.flight.as_ref().is_some_and(|f| f.to_element == to)
        };
        // The exit leg is meant to land on the upper mirror's connection target.
        let receiver = bench.connections.iter()
            .find(|c| c.from == p.upper && c.port == "reflected")
            .map(|c| c.to.as_str());
        let level = |s: &Segment| unit(sub(s.to, s.from)).is_ok_and(|d| d[2].abs() <= 1e-9);
        let vertical = |s: &Segment| unit(sub(s.to, s.from)).is_ok_and(|d| d[0].abs() <= 1e-9 && d[1].abs() <= 1e-9);
        let blocked = receiver.and_then(|r| exits.iter().find(|s| !reached(s, r)));
        if rises.is_empty() || rises.iter().any(|s| !reached(s, &p.upper)) {
            reading.interruption = Some(PathBreak::new(&p.lower, "reflected", "periscope_leg_not_traced"));
        } else if rises.iter().any(|s| !vertical(s)) {
            reading.interruption = Some(PathBreak::new(&p.lower, "reflected", "periscope_leg_not_vertical"));
        } else if exits.is_empty() {
            reading.interruption = Some(PathBreak::new(&p.upper, "reflected", "periscope_leg_not_traced"));
        } else if let Some(s) = blocked {
            reading.interruption = Some(
                PathBreak::new(&p.upper, "reflected", "periscope_leg_blocked").with_detail(format!(
                    "{} exit leg ends at {} ({}) instead of {}",
                    s.source,
                    s.flight.as_ref().map_or("nothing", |f| f.to_element.as_str()),
                    s.status,
                    receiver.unwrap_or_default()
                )),
            );
        } else if let Some(s) = exits.iter().find(|s| !level(s)) {
            let direction = unit(sub(s.to, s.from)).unwrap_or_default();
            reading.interruption = Some(PathBreak::new(&p.upper, "reflected", "periscope_exit_not_level")
                .with_detail(format!("{} exit direction {direction:?} is not horizontal", s.source)));
        }
        // A lens group whose glass shape is not measurable makes the clearance
        // unknown: report it rather than a gap that leaves its walls out.
        let others = || bench.elements.iter().filter(|e| ![p.lower.as_str(), p.upper.as_str()].contains(&e.id.as_str()));
        let unmeasured: Vec<&str> = others().filter(|e| unmeasured(system, e)).map(|e| e.id.as_str()).collect();
        if let Some(id) = unmeasured.first() && reading.interruption.is_none() {
            reading.interruption = Some(PathBreak::new(id, "in", "periscope_clearance_unresolved").with_detail(format!(
                "lens group {id}: a prescription surface is not defined over the whole body radius, so its glass has walls the clearance does not measure"
            )));
        }
        reading.conflicts.extend(unmeasured.iter().map(|id| id.to_string()));
        let legs = rises.iter().map(|s| (*s, Some(p.upper.as_str())))
            .chain(exits.iter().map(|s| (*s, receiver)));
        for (s, intended) in legs {
            let landed = intended.filter(|&id| reached(s, id));
            // Where the tracer stopped the leg on another element, that element touches it.
            let struck = s.flight.as_ref().map(|f| f.to_element.as_str())
                .filter(|&id| landed != Some(id));
            let beam = bench.sources.iter().find(|src| src.id == s.source).map_or(0., |src| src.beam_radius);
            for e in others() {
                if unmeasured.contains(&e.id.as_str()) {
                    continue;
                }
                let gap = if struck == Some(e.id.as_str()) {
                    0.
                } else {
                    clearance(bench, system, e, s.from, s.to, landed == Some(e.id.as_str()))
                };
                if reading.min_clearance_mm.is_none_or(|m| gap < m) {
                    reading.min_clearance_mm = Some(gap);
                    reading.nearest_element = Some(e.id.clone());
                }
                if gap < beam.max(1e-9) && !reading.conflicts.contains(&e.id) {
                    reading.conflicts.push(e.id.clone());
                }
            }
        }
        reading
    }).collect()
}

/// Table-frame XY footprint corners of an element.
fn footprint(e: &Element) -> Vec<[f64; 2]> {
    let [x, y, _] = e.position;
    let (half, yaw) = match &e.mount {
        Some(m) => ([m.base_size[0] / 2., m.base_size[1] / 2.], e.rotation.map_or(0., |r| r[2])),
        None => ([e.body_radius; 2], 0.),
    };
    let (s, c) = yaw.to_radians().sin_cos();
    [[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]].iter().map(|[i, j]| {
        let (u, v) = (i * half[0], j * half[1]);
        [x + u * c - v * s, y + u * s + v * c]
    }).collect()
}

fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Whether the edge `a`–`b` lies in the closed polygon: split it wherever it meets
/// the outline (crossings, touches and collinear overlaps) and test each piece.
fn edge_inside(outline: &[[f64; 2]], a: [f64; 2], b: [f64; 2]) -> bool {
    let n = outline.len();
    let r = [b[0] - a[0], b[1] - a[1]];
    let rr = r[0] * r[0] + r[1] * r[1];
    let mut cuts = vec![0., 1.];
    for j in 0..n {
        let (c, d) = (outline[j], outline[(j + 1) % n]);
        let s = [d[0] - c[0], d[1] - c[1]];
        let q = [c[0] - a[0], c[1] - a[1]];
        let den = cross2(r, s);
        let scale = rr.sqrt() * s[0].hypot(s[1]);
        if den.abs() <= 1e-12 * scale {
            if cross2(q, r).abs() <= 1e-12 * rr.max(1e-300) {
                for p in [c, d] {
                    let t = ((p[0] - a[0]) * r[0] + (p[1] - a[1]) * r[1]) / rr;
                    if (0. ..=1.).contains(&t) {
                        cuts.push(t);
                    }
                }
            }
        } else {
            let (t, u) = (cross2(q, s) / den, cross2(q, r) / den);
            if (-1e-12..=1. + 1e-12).contains(&u) && (0. ..=1.).contains(&t) {
                cuts.push(t);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2).all(|w| {
        let t = (w[0] + w[1]) / 2.;
        w[1] - w[0] <= 1e-12 || inside_polygon(outline, [a[0] + r[0] * t, a[1] + r[1] * t])
    })
}

/// A convex footprint (counter-clockwise corners) lies in the closed outline when
/// its whole boundary does and no outline vertex pokes strictly into it.
fn contains(outline: &[[f64; 2]], corners: &[[f64; 2]]) -> bool {
    let m = corners.len();
    let strictly_inside = |p: [f64; 2]| {
        (0..m).all(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % m]);
            cross2([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]) > 1e-9
        })
    };
    (0..m).all(|i| edge_inside(outline, corners[i], corners[(i + 1) % m]))
        && !outline.iter().any(|&v| strictly_inside(v))
}

pub(crate) fn zones(bench: &Bench) -> Vec<ZoneReading> {
    bench.zones.iter().map(|z| {
        let outline = z.outline();
        let members: Vec<&Element> = if z.members.is_empty() {
            bench.elements.iter()
                .filter(|e| inside_polygon(&outline, [e.position[0], e.position[1]]))
                .collect()
        } else {
            z.members.iter().filter_map(|m| bench.elements.iter().find(|e| &e.id == m)).collect()
        };
        let fold = |f: fn(f64, f64) -> f64, i: usize, start: f64| outline.iter().map(|p| p[i]).fold(start, f);
        ZoneReading {
            id: z.id.clone(),
            label: z.label.clone(),
            area_mm2: polygon_area(&outline),
            bounds_mm: [
                [fold(f64::min, 0, f64::INFINITY), fold(f64::min, 1, f64::INFINITY)],
                [fold(f64::max, 0, f64::NEG_INFINITY), fold(f64::max, 1, f64::NEG_INFINITY)],
            ],
            out_of_bounds: members.iter()
                .filter(|e| !contains(&outline, &footprint(e)))
                .map(|e| e.id.clone())
                .collect(),
            members: members.iter().map(|e| e.id.clone()).collect(),
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slab_distance_matches_a_dense_sample_of_its_glass() {
        // A wedge steep enough that its back face cuts the front face inside the rim.
        let wedge = 40_f64.to_radians();
        let solid = Solid { radius: 18., front: 0., back: [-2., 0., 0.], back_normal: [wedge.cos(), wedge.sin(), 0.] };
        let mut glass = vec![];
        let n = 60;
        for i in 0..=n {
            for j in 0..=n {
                for k in 0..=n {
                    let q = [-20. + 20. * f64::from(i) / f64::from(n), -18. + 36. * f64::from(j) / f64::from(n), -18. + 36. * f64::from(k) / f64::from(n)];
                    if solid.contains(q, 0.) {
                        glass.push(q);
                    }
                }
            }
        }
        let spacing = 36. / f64::from(n);
        for q in [[5., 3., 1.], [-30., -25., 4.], [-3., 20., 2.], [1., -15., 19.], [-12., 0., 25.], [-1., 5., 0.]] {
            let exact = solid.distance(q);
            let sampled = glass.iter().map(|g| norm(sub(*g, q))).fold(f64::INFINITY, f64::min);
            assert!(exact <= sampled + 1e-9 && sampled - exact < 2. * spacing, "{q:?}: {exact} vs {sampled}");
        }
        assert_eq!(solid.distance([-1., 10., 0.]), 0.);
    }

    #[test]
    fn concave_notch_is_outside_the_zone() {
        let zone = [[0., 0.], [40., 0.], [40., 40.], [30., 40.], [30., 20.], [10., 20.], [10., 40.], [0., 40.]];
        let square = |lo: f64, hi: f64| [[lo, lo], [hi, lo], [hi, hi], [lo, hi]];
        // Corners on the notch walls, top edge across the notch.
        assert!(!contains(&zone, &square(10., 30.)));
        assert!(contains(&zone, &square(1., 19.)));
        // Touching the outline from inside still counts as inside.
        assert!(contains(&zone, &[[0., 0.], [10., 0.], [10., 40.], [0., 40.]]));
        // A reflex vertex poking into the footprint.
        assert!(!contains(&zone, &[[5., 15.], [15., 15.], [15., 25.], [5., 25.]]));
    }
}
