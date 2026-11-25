//! Free-space geometric bench. World XY is the table; Z is height (mm).
//! Elements use Rz(yaw) Ry(pitch) Rx(roll), with local +X as surface normal.
use super::math::*;
use super::{Field, Interaction, System, coating, trace::Launch};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bench {
    pub name: String,
    pub table_size: [f64; 2],
    pub hole_pitch: f64,
    pub origin: V3,
    pub elements: Vec<Element>,
    pub sources: Vec<Source>,
    pub connections: Vec<Connection>,
    pub max_interactions: usize,
    pub min_power: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    pub part: String,
    pub base_size: [f64; 2],
    pub height_range: [f64; 2],
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Mirror,
    CurvedMirror,
    Pbs,
    Waveplate,
    Polarizer,
    Lens,
    LensGroup,
    Splitter,
    Detector,
    Aperture,
    Blocker,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Element {
    pub id: String,
    pub label: String,
    pub kind: Kind,
    pub position: V3,
    /// Degrees: [roll about X, pitch about Y, yaw about Z].
    pub rotation: V3,
    pub aperture: f64,
    pub body_radius: f64,
    pub focal_length: f64,
    pub reflectance: f64,
    pub transmittance: f64,
    pub mount: Option<Mount>,
    /// Splitter substrate index; replaces scalar R/T with Fresnel splitting.
    #[serde(default)]
    pub index: Option<f64>,
    /// Thin-film stack on a mirror or splitter face, replacing scalar R/T.
    #[serde(default)]
    pub coating: Option<Coating>,
    /// Surface scatter model on mirrors, splitters and thin lenses.
    #[serde(default)]
    pub scatter: Option<Scatter>,
    /// Lens group: internal Fresnel ghost bounces to enumerate (0-2).
    #[serde(default)]
    pub ghosts: usize,
    /// Detector irradiance map resolution [columns, rows].
    #[serde(default)]
    pub pixels: Option<[usize; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retardance_waves: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_axis_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extinction_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screw_tpi: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lever_arm_mm: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coating {
    pub layers: Vec<Layer>,
    pub substrate_index: f64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub index: f64,
    /// Film thickness in nm.
    pub thickness: f64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum Scatter {
    Lambertian { fraction: f64, rays: usize },
    Abg { a: f64, b: f64, g: f64, rays: usize },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Polarization {
    Unpolarized {},
    Horizontal {},
    Vertical {},
    #[serde(rename = "diagonal45")]
    Diagonal45 {},
    RightCircular {},
    LeftCircular {},
    Linear {
        angle_deg: f64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub position: V3,
    pub direction: V3,
    pub wavelength: f64,
    pub power: f64,
    pub beam_radius: f64,
    pub rings: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waist: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waist_offset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m2: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polarization: Option<Polarization>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub from: String,
    pub port: String,
    pub to: String,
}
const GOLDEN: f64 = 2.39996322972865332;
fn hemisphere(axis: V3, cos: f64, phi: f64) -> V3 {
    let u = unit(cross(
        axis,
        if axis[2].abs() < 0.9 {
            [0., 0., 1.]
        } else {
            [0., 1., 0.]
        },
    ))
    .unwrap_or([1., 0., 0.]);
    let v = cross(axis, u);
    let sin = (1. - cos * cos).max(0.).sqrt();
    add(
        scale(axis, cos),
        add(scale(u, sin * phi.cos()), scale(v, sin * phi.sin())),
    )
}
fn uniform(k: usize, n: usize, axis: V3) -> V3 {
    hemisphere(axis, (k as f64 + 0.5) / n as f64, GOLDEN * k as f64)
}
fn cosine(k: usize, n: usize, axis: V3) -> V3 {
    hemisphere(
        axis,
        ((k as f64 + 0.5) / n as f64).sqrt(),
        GOLDEN * k as f64,
    )
}
impl Scatter {
    fn rays(&self) -> usize {
        match *self {
            Self::Lambertian { rays, .. } | Self::Abg { rays, .. } => rays,
        }
    }
    fn bsdf(&self, d: V3, specular: V3, axis: V3) -> f64 {
        match *self {
            Self::Lambertian { .. } => dot(d, axis).max(0.) / std::f64::consts::PI,
            // Harvey-Shack: shift between projected scatter and specular sines.
            Self::Abg { a, b, g, .. } => {
                let proj = |v: V3| sub(v, scale(axis, dot(v, axis)));
                a / (b + norm(sub(proj(d), proj(specular))).powf(g))
            }
        }
    }
    /// Total integrated scatter over the exit hemisphere, clamped to one.
    pub fn tis(&self, specular: V3, axis: V3) -> f64 {
        match *self {
            Self::Lambertian { fraction, .. } => fraction,
            Self::Abg { .. } => {
                const N: usize = 2048;
                let total: f64 = (0..N)
                    .map(|k| {
                        self.bsdf(uniform(k, N, axis), specular, axis) * std::f64::consts::TAU
                            / N as f64
                    })
                    .sum();
                total.min(1.)
            }
        }
    }
    /// Deterministic scattered directions and power weights summing to tis.
    pub fn sample(&self, specular: V3, axis: V3) -> Vec<(V3, f64)> {
        let n = self.rays();
        let tis = self.tis(specular, axis);
        if tis <= 0. {
            return vec![];
        }
        match *self {
            Self::Lambertian { .. } => (0..n)
                .map(|k| (cosine(k, n, axis), tis / n as f64))
                .collect(),
            Self::Abg { .. } => {
                let dirs: Vec<V3> = (0..n).map(|k| uniform(k, n, axis)).collect();
                let sum: f64 = dirs.iter().map(|d| self.bsdf(*d, specular, axis)).sum();
                if sum <= 0. {
                    return vec![];
                }
                dirs.into_iter()
                    .map(|d| (d, tis * self.bsdf(d, specular, axis) / sum))
                    .collect()
            }
        }
    }
}

pub fn rotate(v: V3, angles: V3) -> V3 {
    let [r, p, y] = angles.map(f64::to_radians);
    let (sr, cr) = r.sin_cos();
    let (sp, cp) = p.sin_cos();
    let (sy, cy) = y.sin_cos();
    let a = [v[0], cr * v[1] - sr * v[2], sr * v[1] + cr * v[2]];
    let b = [cp * a[0] + sp * a[2], a[1], -sp * a[0] + cp * a[2]];
    [cy * b[0] - sy * b[1], sy * b[0] + cy * b[1], b[2]]
}
pub(crate) fn local(v: V3, angles: V3) -> V3 {
    std::array::from_fn(|i| {
        dot(
            v,
            rotate(
                std::array::from_fn(|j| if i == j { 1. } else { 0. }),
                angles,
            ),
        )
    })
}
pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn ports(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Mirror | Kind::CurvedMirror => &["reflected"],
        Kind::Splitter | Kind::Pbs => &["reflected", "transmitted"],
        Kind::Lens | Kind::LensGroup | Kind::Aperture | Kind::Waveplate | Kind::Polarizer => {
            &["out"]
        }
        _ => &[],
    }
}
fn valid_vec(v: V3) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= 1e6)
}
impl Bench {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.len() > 256
            || !valid_vec(self.origin)
            || self
                .table_size
                .iter()
                .any(|x| !x.is_finite() || *x <= 0. || *x > 1e6)
            || !self.hole_pitch.is_finite()
            || self.hole_pitch <= 0.
            || self.hole_pitch > 1e6
            || self.elements.len() > 128
            || self.sources.is_empty()
            || self.sources.len() > 16
            || !(1..=128).contains(&self.max_interactions)
            || !self.min_power.is_finite()
            || !(0. ..=1.).contains(&self.min_power)
        {
            return Err("Invalid bench dimensions, sources or trace budget".into());
        }
        let mut ids = HashSet::new();
        for (id, p) in self
            .elements
            .iter()
            .map(|e| (&e.id, e.position))
            .chain(self.sources.iter().map(|s| (&s.id, s.position)))
        {
            if id.is_empty() || id.len() > 64 || !ids.insert(id) || !valid_vec(p) {
                return Err("Bench IDs must be unique, nonempty and positions finite".into());
            }
        }
        for e in &self.elements {
            if e.label.len() > 256
                || !valid_vec(e.rotation)
                || !e.aperture.is_finite()
                || e.aperture <= 0.
                || e.aperture > 1e5
                || !e.body_radius.is_finite()
                || e.body_radius < e.aperture
                || e.body_radius > 1e5
                || !e.focal_length.is_finite()
                || e.focal_length.abs() > 1e6
                || (matches!(e.kind, Kind::Lens | Kind::CurvedMirror)
                    && e.focal_length.abs() < 1e-6)
                || !e.reflectance.is_finite()
                || !e.transmittance.is_finite()
                || e.reflectance < 0.
                || e.transmittance < 0.
                || e.reflectance + e.transmittance > 1.
                || e.retardance_waves
                    .is_some_and(|x| !(0. ..=10.).contains(&x))
                || e.fast_axis_deg
                    .is_some_and(|x| !(-360. ..=360.).contains(&x))
                || e.extinction_ratio
                    .is_some_and(|x| !(1. ..=1e9).contains(&x))
                || e.screw_tpi.is_some_and(|x| !(10. ..=500.).contains(&x))
                || e.lever_arm_mm.is_some_and(|x| !(2. ..=200.).contains(&x))
            {
                return Err(format!("Invalid optical parameters: {}", e.id));
            }
            if let Some(m) = &e.mount {
                if m.part.len() > 256
                    || m.base_size
                        .iter()
                        .any(|x| !x.is_finite() || *x <= 0. || *x > 1e5)
                    || m.height_range
                        .iter()
                        .any(|x| !x.is_finite() || *x < 0. || *x > 1e6)
                    || m.height_range[0] > m.height_range[1]
                {
                    return Err(format!("Invalid mount: {}", e.id));
                }
            }
            if let Some(n) = e.index {
                if e.kind != Kind::Splitter || !(1. ..=4.).contains(&n) {
                    return Err(format!("Splitter substrate index outside 1..4: {}", e.id));
                }
            }
            if let Some(c) = &e.coating {
                if ![Kind::Mirror, Kind::CurvedMirror, Kind::Splitter].contains(&e.kind)
                    || c.layers.len() > 32
                    || !(1. ..=4.).contains(&c.substrate_index)
                    || c.layers.iter().any(|l| {
                        !(1. ..=4.).contains(&l.index) || !(0. ..=1e5).contains(&l.thickness)
                    })
                {
                    return Err(format!("Invalid coating stack: {}", e.id));
                }
            }
            if let Some(s) = &e.scatter {
                let valid = match s {
                    Scatter::Lambertian { fraction, rays } => {
                        (0. ..=1.).contains(fraction) && (1..=64).contains(rays)
                    }
                    Scatter::Abg { a, b, g, rays } => {
                        (0. ..=1e3).contains(a)
                            && *b > 0.
                            && *b <= 1e3
                            && *g > 0.
                            && *g <= 10.
                            && (1..=64).contains(rays)
                    }
                };
                if !valid
                    || ![Kind::Mirror, Kind::CurvedMirror, Kind::Splitter, Kind::Lens]
                        .contains(&e.kind)
                {
                    return Err(format!("Invalid scatter model: {}", e.id));
                }
            }
            if e.ghosts > 2 || (e.ghosts > 0 && e.kind != Kind::LensGroup) {
                return Err(format!(
                    "Ghost bounces require a lens group (0-2): {}",
                    e.id
                ));
            }
            if let Some(bins) = e.pixels {
                if e.kind != Kind::Detector || bins.iter().any(|b| !(2..=128).contains(b)) {
                    return Err(format!(
                        "Detector pixels require 2..=128 bins per axis: {}",
                        e.id
                    ));
                }
            }
        }
        let mut packets = 0;
        for s in &self.sources {
            if !valid_vec(s.direction)
                || unit(s.direction).is_err()
                || !s.wavelength.is_finite()
                || !(100. ..=20000.).contains(&s.wavelength)
                || !s.power.is_finite()
                || s.power <= 0.
                || s.power > 1e6
                || !s.beam_radius.is_finite()
                || !(0. ..=1e5).contains(&s.beam_radius)
                || s.rings > 8
                || s.waist.is_some_and(|x| !x.is_finite() || x <= 0.)
                || s.waist_offset.is_some_and(|x| !x.is_finite())
                || s.m2.is_some_and(|x| !x.is_finite() || x < 1.)
                || matches!(s.polarization, Some(Polarization::Linear { angle_deg }) if !angle_deg.is_finite())
            {
                return Err(format!("Invalid source: {}", s.id));
            }
            packets += 1 + 3 * s.rings * (s.rings + 1);
        }
        if packets > 1024 || self.connections.len() > 384 {
            return Err("Bench packet or connection budget exceeded".into());
        }
        let mut outputs = HashSet::new();
        for c in &self.connections {
            let allowed = if self.sources.iter().any(|s| s.id == c.from) {
                &["out"][..]
            } else {
                self.elements
                    .iter()
                    .find(|e| e.id == c.from)
                    .map(|e| ports(e.kind))
                    .ok_or("Unknown connection source")?
            };
            if !allowed.contains(&c.port.as_str())
                || !self.elements.iter().any(|e| e.id == c.to)
                || !outputs.insert((&c.from, &c.port))
            {
                return Err(format!(
                    "Invalid or repeated output connection: {} / {}",
                    c.from, c.port
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    pub source: String,
    pub branch: String,
    pub from: V3,
    pub to: V3,
    pub power: f64,
    pub element: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jones: Option<[[f64; 2]; 2]>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Termination {
    pub source: String,
    pub branch: String,
    pub element: Option<String>,
    pub position: V3,
    pub power: f64,
    pub reason: String,
}
#[derive(Default, Debug, Serialize)]
pub struct Ledger {
    pub launched: f64,
    pub detected: f64,
    pub absorbed: f64,
    pub blocked: f64,
    pub escaped: f64,
    pub truncated: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct DetectorMap {
    pub id: String,
    pub bins: [usize; 2],
    /// Clear half-width (mm); the map spans the inscribed square.
    pub extent: f64,
    /// Row-major bin power: rows are local height, columns local transverse.
    pub power: Vec<f64>,
    pub total: f64,
    /// Power-weighted centroid and RMS in local (transverse, height) mm.
    pub centroid: [f64; 2],
    pub rms: [f64; 2],
}
#[derive(Debug, Serialize)]
pub struct Trace {
    pub segments: Vec<Segment>,
    pub terminations: Vec<Termination>,
    pub power: Ledger,
    pub detectors: Vec<DetectorMap>,
    pub balance_error: f64,
    pub warnings: Vec<String>,
}
type Jones = [[f64; 2]; 2];

impl Polarization {
    fn jones(self) -> Option<Jones> {
        use std::f64::consts::FRAC_1_SQRT_2 as S;
        match self {
            Self::Unpolarized {} => None,
            Self::Horizontal {} => Some(linear(0.)),
            Self::Vertical {} => Some([[0., 0.], [1., 0.]]),
            Self::Diagonal45 {} => Some([[S, 0.], [S, 0.]]),
            Self::RightCircular {} => Some([[S, 0.], [0., -S]]),
            Self::LeftCircular {} => Some([[S, 0.], [0., S]]),
            Self::Linear { angle_deg } => Some(linear(angle_deg)),
        }
    }
}

fn linear(angle: f64) -> Jones {
    let (s, c) = angle.rem_euclid(360.).to_radians().sin_cos();
    [[c, 0.], [s, 0.]]
}

// Resolve the field in the optical axes, multiply by the complex amplitude
// gains, then rotate back into the packet's horizontal/vertical basis.
fn jones_axes(j: Jones, angle: f64, gains: [[f64; 2]; 2]) -> (Jones, f64) {
    let [[c, _], [s, _]] = linear(angle);
    let [h, v] = j;
    let multiply = |[re, im]: [f64; 2], [gr, gi]: [f64; 2]| [re * gr - im * gi, re * gi + im * gr];
    let a = multiply(std::array::from_fn(|i| c * h[i] + s * v[i]), gains[0]);
    let b = multiply(std::array::from_fn(|i| -s * h[i] + c * v[i]), gains[1]);
    let fraction = a.into_iter().chain(b).map(|x| x * x).sum::<f64>();
    if fraction == 0. {
        return ([[0.; 2]; 2], 0.);
    }
    let magnitude = fraction.sqrt();
    (
        [
            std::array::from_fn(|i| (c * a[i] - s * b[i]) / magnitude),
            std::array::from_fn(|i| (s * a[i] + c * b[i]) / magnitude),
        ],
        fraction.min(1.),
    )
}

fn focused(direction: V3, height: V3, e: &Element) -> Result<V3, String> {
    let [x, y, z] = local(direction, e.rotation);
    let local_direction = unit([
        x.signum(),
        y / x.abs() - height[1] / e.focal_length,
        z / x.abs() - height[2] / e.focal_length,
    ])?;
    Ok(rotate(local_direction, e.rotation))
}

struct Packet {
    source: String,
    branch: String,
    p: V3,
    d: V3,
    power: f64,
    depth: usize,
    expected: Option<String>,
    wavelength: f64,
    jones: Option<Jones>,
}
enum GhostEnd {
    Exit(V3),
    Truncated,
    Blocked(&'static str),
}
struct DetectorAccum {
    power: Vec<f64>,
    total: f64,
    sum: [f64; 2],
    sum2: [f64; 2],
}
struct GhostPath {
    points: Vec<V3>,
    factor: f64,
    bounces: usize,
    end: GhostEnd,
}
/// Enumerates Fresnel ghost paths through the prescription in prescription
/// coordinates. At each surface the refracted share continues and the
/// reflected share recurses until the bounce budget is spent; reflected
/// shares beyond the budget are recorded as truncated so the Fresnel
/// products telescope and power is conserved exactly.
struct GhostWalk<'a> {
    system: &'a System,
    entry: f64,
    after: Vec<f64>,
    vertices: Vec<f64>,
    max_bounces: usize,
    paths: Vec<GhostPath>,
}
impl GhostWalk<'_> {
    fn before(&self, i: usize) -> f64 {
        if i == 0 {
            self.entry
        } else {
            self.after[i - 1]
        }
    }
    fn run(
        &mut self,
        mut p: V3,
        mut v: V3,
        mut i: i64,
        step: i64,
        mut factor: f64,
        bounces_left: usize,
        mut points: Vec<V3>,
    ) {
        let len = self.after.len() as i64;
        loop {
            if i < 0 || i >= len {
                self.paths.push(GhostPath {
                    points,
                    factor,
                    bounces: self.max_bounces - bounces_left,
                    end: GhostEnd::Exit(v),
                });
                return;
            }
            let s = &self.system.surfaces[i as usize];
            let hit = match s.profile().intersect(p, v, self.vertices[i as usize]) {
                Ok(hit) => hit,
                Err(_) => {
                    self.paths.push(GhostPath {
                        points,
                        factor,
                        bounces: self.max_bounces - bounces_left,
                        end: GhostEnd::Blocked("intersection"),
                    });
                    return;
                }
            };
            points.push(hit.point);
            if s.clear_aperture && hit.point[0].hypot(hit.point[1]) > s.aperture + 1e-9 {
                self.paths.push(GhostPath {
                    points,
                    factor,
                    bounces: self.max_bounces - bounces_left,
                    end: GhostEnd::Blocked("aperture"),
                });
                return;
            }
            let (n1, n2) = if step > 0 {
                (self.before(i as usize), self.after[i as usize])
            } else {
                (self.after[i as usize], self.before(i as usize))
            };
            let ci = dot(v, hit.normal).clamp(-1., 1.);
            let reflectance = coating::fresnel(n1, n2, ci).unwrap_or(1.);
            if reflectance > 0. {
                let bounced = unit(sub(v, scale(hit.normal, 2. * ci))).unwrap_or(v);
                if bounces_left > 0 {
                    self.run(
                        hit.point,
                        bounced,
                        i - step,
                        -step,
                        factor * reflectance,
                        bounces_left - 1,
                        points.clone(),
                    );
                } else {
                    self.paths.push(GhostPath {
                        points: points.clone(),
                        factor: factor * reflectance,
                        bounces: self.max_bounces - bounces_left + 1,
                        end: GhostEnd::Truncated,
                    });
                }
            }
            let disc = n2 * n2 - n1 * n1 * (1. - ci * ci);
            if disc < 0. {
                return; // TIR: everything followed the reflected share.
            }
            let alpha = disc.sqrt() * if ci >= 0. { 1. } else { -1. } - n1 * ci;
            match unit(scale(add(scale(v, n1), scale(hit.normal, alpha)), 1. / n2)) {
                Ok(refracted) => v = refracted,
                Err(_) => {
                    self.paths.push(GhostPath {
                        points,
                        factor,
                        bounces: self.max_bounces - bounces_left,
                        end: GhostEnd::Blocked("refraction"),
                    });
                    return;
                }
            }
            factor *= 1. - reflectance;
            p = hit.point;
            i += step;
        }
    }
}
// A mount is an upright box at the component's XY position, rotated by yaw.
// Its top is below the optical body. No unprovided hardware geometry is inferred.
fn mount_hit(e: &Element, p: V3, d: V3, table_z: f64) -> Option<f64> {
    let m = e.mount.as_ref()?;
    let angles = [0., 0., e.rotation[2]];
    let q = local(sub(p, [e.position[0], e.position[1], table_z]), angles);
    let v = local(d, angles);
    let low = [-m.base_size[0] / 2., -m.base_size[1] / 2., 0.];
    let high = [
        m.base_size[0] / 2.,
        m.base_size[1] / 2.,
        e.position[2] - table_z - e.body_radius,
    ];
    if high[2] <= 0. {
        return None;
    }
    let (mut near, mut far) = (0f64, f64::INFINITY);
    for i in 0..3 {
        if v[i].abs() < 1e-12 {
            if q[i] < low[i] || q[i] > high[i] {
                return None;
            }
        } else {
            let a = (low[i] - q[i]) / v[i];
            let b = (high[i] - q[i]) / v[i];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
    }
    (near <= far && far > 1e-7).then_some(if near > 1e-7 { near } else { far })
}
impl Bench {
    /// A placed group references the project's current prescription directly.
    pub fn validate_groups(&self, system: &System) -> Result<(), String> {
        if !self.elements.iter().any(|e| e.kind == Kind::LensGroup) {
            return Ok(());
        }
        if system.surfaces.is_empty() {
            return Err("A lens group requires prescription surfaces".into());
        }
        if system
            .surfaces
            .iter()
            .any(|s| s.interaction == Interaction::Reflect)
            || system
                .surfaces
                .iter()
                .take(system.surfaces.len() - 1)
                .any(|s| s.thickness <= 0.)
        {
            return Err(
                "Placed lens groups require forward refractive surfaces and positive internal gaps"
                    .into(),
            );
        }
        let exit = system
            .surfaces
            .iter()
            .rev()
            .find(|s| s.interaction == Interaction::Refract)
            .map(|s| s.material.as_str())
            .unwrap_or(&system.object_material);
        for source in &self.sources {
            let air = system.material_index("AIR", source.wavelength)?;
            if (system.object_index(source.wavelength)? - air).abs() > 1e-12
                || (system.material_index(exit, source.wavelength)? - air).abs() > 1e-12
            {
                return Err("Placed lens groups require air at both physical boundaries".into());
            }
        }
        if self
            .elements
            .iter()
            .any(|e| e.kind == Kind::LensGroup && e.reflectance != 0.)
        {
            return Err("Lens group power uses transmittance; reflectance must be zero".into());
        }
        if system.surfaces.len() > 32
            && self
                .elements
                .iter()
                .any(|e| e.kind == Kind::LensGroup && e.ghosts > 0)
        {
            return Err("Ghost tracing requires at most 32 prescription surfaces".into());
        }
        Ok(())
    }

    fn nearest<'a>(
        &'a self,
        system: &System,
        position: V3,
        direction: V3,
        exclude: Option<&str>,
    ) -> Option<(f64, &'a Element, bool, Option<usize>)> {
        let mut closest: Option<(f64, &Element, bool, Option<usize>)> = None;
        for e in &self.elements {
            if exclude == Some(e.id.as_str()) {
                continue;
            }
            let p = local(sub(position, e.position), e.rotation);
            let d = local(direction, e.rotation);
            let mut take = |distance: f64, mount: bool, surface: Option<usize>| {
                if distance > 1e-7 && closest.is_none_or(|a| distance < a.0) {
                    closest = Some((distance, e, mount, surface));
                }
            };
            if e.kind == Kind::LensGroup {
                let mut vertex = 0.;
                for (i, surface) in system.surfaces.iter().enumerate() {
                    if let Ok(hit) =
                        surface
                            .profile()
                            .intersect([p[1], p[2], p[0]], [d[1], d[2], d[0]], vertex)
                    {
                        if hit.point[0].hypot(hit.point[1]) <= e.body_radius {
                            take(hit.distance, false, Some(i));
                        }
                    }
                    vertex += surface.thickness;
                }
            } else if d[0].abs() > 1e-12 {
                let distance = -p[0] / d[0];
                let hit = add(p, scale(d, distance));
                if hit[1].hypot(hit[2]) <= e.body_radius {
                    take(distance, false, None);
                }
            }
            if let Some(distance) = mount_hit(e, position, direction, self.origin[2]) {
                take(distance, true, None);
            }
        }
        closest
    }

    /// Lens group with Fresnel ghosts: every refractive surface splits the
    /// packet; bounce shares beyond the budget land in truncated so the
    /// polarization-averaged Fresnel products conserve power exactly.
    /// Ghost segments skip the group's internal obstruction check.
    fn trace_group_ghosts(
        &self,
        system: &System,
        out: &mut Trace,
        queue: &mut VecDeque<Packet>,
        ray: &Packet,
        e: &Element,
        hit: V3,
    ) -> Result<(), String> {
        let group_size = system.surfaces.len();
        if ray.depth + group_size > self.max_interactions {
            out.power.truncated += ray.power;
            out.terminations.push(Termination {
                source: ray.source.clone(),
                branch: ray.branch.clone(),
                element: Some(e.id.clone()),
                position: hit,
                power: ray.power,
                reason: "budget".into(),
            });
            return Ok(());
        }
        let mut n = system.material_index("AIR", ray.wavelength)?;
        let entry = n;
        let mut after = Vec::with_capacity(group_size);
        let mut vertices = Vec::with_capacity(group_size);
        let mut vertex = 0.;
        for s in &system.surfaces {
            if s.interaction == Interaction::Refract {
                n = system.material_index(&s.material, ray.wavelength)?;
            }
            after.push(n);
            vertices.push(vertex);
            vertex += s.thickness;
        }
        let p = local(sub(ray.p, e.position), e.rotation);
        let d = local(ray.d, e.rotation);
        let mut walk = GhostWalk {
            system,
            entry,
            after,
            vertices,
            max_bounces: e.ghosts,
            paths: vec![],
        };
        walk.run(
            [p[1], p[2], p[0]],
            [d[1], d[2], d[0]],
            0,
            1,
            1.,
            e.ghosts,
            vec![],
        );
        out.power.absorbed += ray.power * (1. - e.transmittance);
        let world = |p: V3| add(e.position, rotate([p[2], p[0], p[1]], e.rotation));
        for path in walk.paths {
            let power = ray.power * e.transmittance * path.factor;
            if power <= 0. {
                continue;
            }
            let mut last = hit;
            for point in path.points.iter().skip(1) {
                let end = world(*point);
                out.segments.push(Segment {
                    jones: ray.jones,
                    source: ray.source.clone(),
                    branch: ray.branch.clone(),
                    from: last,
                    to: end,
                    power,
                    element: Some(e.id.clone()),
                    status: if path.bounces == 0 {
                        "group_surface"
                    } else {
                        "group_ghost"
                    }
                    .into(),
                });
                last = end;
            }
            let termination = |power: f64, reason: String| Termination {
                source: ray.source.clone(),
                branch: ray.branch.clone(),
                element: Some(e.id.clone()),
                position: last,
                power,
                reason,
            };
            match path.end {
                GhostEnd::Exit(direction) => {
                    let main = path.bounces == 0;
                    queue.push_back(Packet {
                        source: ray.source.clone(),
                        branch: if main {
                            format!("{}/{}:out", ray.branch, e.id)
                        } else {
                            format!("{}/{}:ghost", ray.branch, e.id)
                        },
                        p: last,
                        d: rotate([direction[2], direction[0], direction[1]], e.rotation),
                        power,
                        depth: ray.depth + path.points.len().saturating_sub(1),
                        expected: if main {
                            self.connections
                                .iter()
                                .find(|c| c.from == e.id && c.port == "out")
                                .map(|c| c.to.clone())
                        } else {
                            None
                        },
                        wavelength: ray.wavelength,
                        jones: ray.jones,
                    });
                }
                GhostEnd::Truncated => {
                    out.power.truncated += power;
                    out.terminations
                        .push(termination(power, "ghost_bounce_limit".into()));
                }
                GhostEnd::Blocked(reason) => {
                    out.power.blocked += power;
                    out.terminations
                        .push(termination(power, format!("group_{reason}")));
                }
            }
        }
        Ok(())
    }

    pub fn trace(&self, system: &System) -> Result<Trace, String> {
        self.validate()?;
        self.validate_groups(system)?;
        let mut out = Trace {
            segments: vec![],
            terminations: vec![],
            power: Ledger::default(),
            detectors: vec![],
            balance_error: 0.,
            warnings: vec![],
        };
        let mut maps: HashMap<&str, DetectorAccum> = HashMap::new();
        let next = |id: &str, port: &str| {
            self.connections
                .iter()
                .find(|c| c.from == id && c.port == port)
                .map(|c| c.to.clone())
        };
        let mut queue = VecDeque::new();
        for s in &self.sources {
            let d = unit(s.direction)?;
            let u = unit(cross(
                d,
                if d[2].abs() < 0.9 {
                    [0., 0., 1.]
                } else {
                    [0., 1., 0.]
                },
            ))?;
            let v = cross(d, u);
            let mut offsets = vec![[0., 0., 0.]];
            for ring in 1..=s.rings {
                for j in 0..6 * ring {
                    let a = std::f64::consts::TAU * j as f64 / (6 * ring) as f64;
                    offsets.push(add(
                        scale(u, s.beam_radius * ring as f64 / s.rings as f64 * a.cos()),
                        scale(v, s.beam_radius * ring as f64 / s.rings as f64 * a.sin()),
                    ));
                }
            }
            let count = offsets.len();
            out.power.launched += s.power;
            for (i, offset) in offsets.into_iter().enumerate() {
                queue.push_back(Packet {
                    source: s.id.clone(),
                    branch: format!("{}:{i}", s.id),
                    p: add(s.position, offset),
                    d,
                    power: s.power / count as f64,
                    depth: 0,
                    expected: next(&s.id, "out"),
                    wavelength: s.wavelength,
                    jones: s.polarization.and_then(Polarization::jones),
                });
            }
        }
        while let Some(ray) = queue.pop_front() {
            let cutoff = ray.power < self.min_power
                || ray.depth >= self.max_interactions
                || out.segments.len() >= 50_000;
            if cutoff {
                out.power.truncated += ray.power;
                out.terminations.push(Termination {
                    source: ray.source,
                    branch: ray.branch,
                    element: None,
                    position: ray.p,
                    power: ray.power,
                    reason: "budget".into(),
                });
                continue;
            }
            let closest = self.nearest(system, ray.p, ray.d, None);
            let Some((distance, e, mount, face)) = closest else {
                let end = add(
                    ray.p,
                    scale(ray.d, self.table_size[0].hypot(self.table_size[1])),
                );
                out.segments.push(Segment {
                    jones: ray.jones,
                    source: ray.source.clone(),
                    branch: ray.branch.clone(),
                    from: ray.p,
                    to: end,
                    power: ray.power,
                    element: None,
                    status: if ray.expected.is_some() {
                        "missed_target"
                    } else {
                        "escaped"
                    }
                    .into(),
                });
                out.power.escaped += ray.power;
                out.terminations.push(Termination {
                    source: ray.source,
                    branch: ray.branch,
                    element: ray.expected,
                    position: end,
                    power: ray.power,
                    reason: "escaped".into(),
                });
                continue;
            };
            let hit = add(ray.p, scale(ray.d, distance));
            let h = local(sub(hit, e.position), e.rotation);
            let reason = if mount {
                Some("mount_obstruction")
            } else if ray.expected.as_ref().is_some_and(|id| id != &e.id) {
                Some("unexpected_hit")
            } else if h[1].hypot(h[2]) > e.aperture {
                Some("clipped")
            } else if e.kind == Kind::LensGroup
                && (face != Some(0) || local(ray.d, e.rotation)[0] <= 0.)
            {
                Some("group_entry")
            } else if e.kind == Kind::Blocker {
                Some("beam_stop")
            } else {
                None
            };
            out.segments.push(Segment {
                jones: ray.jones,
                source: ray.source.clone(),
                branch: ray.branch.clone(),
                from: ray.p,
                to: hit,
                power: ray.power,
                element: Some(e.id.clone()),
                status: reason.unwrap_or("hit").into(),
            });
            if let Some(reason) = reason {
                out.power.blocked += ray.power;
                out.terminations.push(Termination {
                    source: ray.source,
                    branch: ray.branch,
                    element: Some(e.id.clone()),
                    position: hit,
                    power: ray.power,
                    reason: reason.into(),
                });
                continue;
            }
            if e.kind == Kind::LensGroup {
                let group_size = system.surfaces.len();
                if e.ghosts > 0 {
                    self.trace_group_ghosts(system, &mut out, &mut queue, &ray, e, hit)?;
                    continue;
                }
                if ray.depth + group_size > self.max_interactions {
                    out.power.truncated += ray.power;
                    out.terminations.push(Termination {
                        source: ray.source,
                        branch: ray.branch,
                        element: Some(e.id.clone()),
                        position: hit,
                        power: ray.power,
                        reason: "budget".into(),
                    });
                    continue;
                }
                let p = local(sub(ray.p, e.position), e.rotation);
                let d = local(ray.d, e.rotation);
                let result = system.trace_path(
                    Launch {
                        point: [p[1], p[2], p[0]],
                        direction: [d[1], d[2], d[0]],
                        entrance_opl: 0.,
                        object_point: None,
                    },
                    ray.wavelength,
                    &Field::meridional(0.),
                    group_size - 1,
                    true,
                    false,
                );
                let (traced, failure) = match result {
                    Ok(path) => (Some(path), None),
                    Err(error) => (error.ray, Some((error.kind.as_str(), error.point))),
                };
                let world = |p: V3| add(e.position, rotate([p[2], p[0], p[1]], e.rotation));
                let mut last = hit;
                let mut blocked = None;
                if let Some(path) = &traced {
                    let mut points: Vec<_> = path.points.iter().skip(2).copied().collect();
                    if let Some((_, Some(point))) = failure {
                        points.push(point);
                    }
                    for point in points {
                        let end = world(point);
                        let delta = sub(end, last);
                        let length = norm(delta);
                        if length > 1e-7 {
                            let direction = scale(delta, 1. / length);
                            if dot(direction, rotate([1., 0., 0.], e.rotation)) <= 0. {
                                blocked = Some(("group_geometry".to_string(), e.id.clone()));
                                break;
                            }
                            if let Some((distance, obstacle, mount, _)) =
                                self.nearest(system, last, direction, Some(&e.id))
                            {
                                if distance < length - 1e-7 {
                                    let stop = add(last, scale(direction, distance));
                                    out.segments.push(Segment {
                                        jones: ray.jones,
                                        source: ray.source.clone(),
                                        branch: ray.branch.clone(),
                                        from: last,
                                        to: stop,
                                        power: ray.power,
                                        element: Some(obstacle.id.clone()),
                                        status: "group_obstruction".into(),
                                    });
                                    last = stop;
                                    blocked = Some((
                                        if mount {
                                            "mount_obstruction"
                                        } else {
                                            "group_obstruction"
                                        }
                                        .to_string(),
                                        obstacle.id.clone(),
                                    ));
                                    break;
                                }
                            }
                        }
                        out.segments.push(Segment {
                            jones: ray.jones,
                            source: ray.source.clone(),
                            branch: ray.branch.clone(),
                            from: last,
                            to: end,
                            power: ray.power,
                            element: Some(e.id.clone()),
                            status: "group_surface".into(),
                        });
                        last = end;
                    }
                }
                if blocked.is_none() {
                    if let Some((reason, _)) = failure {
                        blocked = Some((format!("group_{reason}"), e.id.clone()));
                    }
                }
                if let Some((reason, id)) = blocked {
                    out.power.blocked += ray.power;
                    out.terminations.push(Termination {
                        source: ray.source,
                        branch: ray.branch,
                        element: Some(id),
                        position: last,
                        power: ray.power,
                        reason,
                    });
                } else if let Some(path) = traced {
                    out.power.absorbed += ray.power * (1. - e.transmittance);
                    if e.transmittance > 0. {
                        let d = path.direction;
                        queue.push_back(Packet {
                            source: ray.source,
                            branch: format!("{}/{}:out", ray.branch, e.id),
                            p: last,
                            d: rotate([d[2], d[0], d[1]], e.rotation),
                            power: ray.power * e.transmittance,
                            depth: ray.depth + group_size,
                            expected: next(&e.id, "out"),
                            wavelength: ray.wavelength,
                            jones: ray.jones,
                        });
                    }
                }
                continue;
            }
            if e.kind == Kind::Detector {
                out.power.detected += ray.power;
                if let Some([nx, ny]) = e.pixels {
                    let acc = maps.entry(e.id.as_str()).or_insert_with(|| DetectorAccum {
                        power: vec![0.; nx * ny],
                        total: 0.,
                        sum: [0.; 2],
                        sum2: [0.; 2],
                    });
                    let bin = |x: f64, n: usize| {
                        (((x + e.aperture) / (2. * e.aperture)) * n as f64)
                            .floor()
                            .clamp(0., n as f64 - 1.) as usize
                    };
                    acc.power[bin(h[2], ny) * nx + bin(h[1], nx)] += ray.power;
                    acc.total += ray.power;
                    acc.sum[0] += ray.power * h[1];
                    acc.sum[1] += ray.power * h[2];
                    acc.sum2[0] += ray.power * h[1] * h[1];
                    acc.sum2[1] += ray.power * h[2] * h[2];
                }
                out.terminations.push(Termination {
                    source: ray.source,
                    branch: ray.branch,
                    element: Some(e.id.clone()),
                    position: hit,
                    power: ray.power,
                    reason: "detected".into(),
                });
                continue;
            }
            let n = rotate([1., 0., 0.], e.rotation);
            let reflected = sub(ray.d, scale(n, 2. * dot(ray.d, n)));
            let incidence = dot(ray.d, n).abs();
            let films = |c: &Coating| {
                c.layers
                    .iter()
                    .map(|l| (l.index, l.thickness))
                    .collect::<Vec<_>>()
            };
            let mut children = Vec::with_capacity(2);
            let mut scalar = |port, direction, fraction| {
                children.push((port, direction, fraction, ray.jones));
            };
            match e.kind {
                Kind::Mirror | Kind::CurvedMirror => {
                    let r = match &e.coating {
                        Some(c) => {
                            coating::stack(ray.wavelength, incidence, &films(c), c.substrate_index)
                                .map(|(r, _)| r)
                                .unwrap_or(1.)
                        }
                        None => e.reflectance,
                    };
                    let direction = if e.kind == Kind::CurvedMirror {
                        focused(reflected, h, e)?
                    } else {
                        reflected
                    };
                    scalar("reflected", direction, r);
                }
                Kind::Splitter => {
                    let (r, t) = if let Some(c) = &e.coating {
                        coating::stack(ray.wavelength, incidence, &films(c), c.substrate_index)
                            .unwrap_or((1., 0.))
                    } else if let Some(medium) = e.index {
                        let r = coating::fresnel(1., medium, incidence).unwrap_or(1.);
                        (r, 1. - r)
                    } else {
                        (e.reflectance, e.transmittance)
                    };
                    scalar("reflected", reflected, r);
                    scalar("transmitted", ray.d, t);
                }
                Kind::Lens => scalar("out", focused(ray.d, h, e)?, e.transmittance),
                Kind::Aperture => scalar("out", ray.d, 1.),
                Kind::Waveplate => {
                    let phase = std::f64::consts::TAU * e.retardance_waves.unwrap_or(0.5);
                    let angle = e.fast_axis_deg.unwrap_or(0.);
                    let jones = ray
                        .jones
                        .map(|j| jones_axes(j, angle, [[1., 0.], [phase.cos(), phase.sin()]]).0);
                    children.push(("out", ray.d, e.transmittance, jones));
                }
                Kind::Polarizer => {
                    let angle = e.fast_axis_deg.unwrap_or(0.);
                    let leak = 1. / e.extinction_ratio.unwrap_or(1000.);
                    let (jones, fraction) = ray.jones.map_or_else(
                        || (linear(angle), 0.5 * (1. + leak)),
                        |j| jones_axes(j, angle, [[1., 0.], [leak.sqrt(), 0.]]),
                    );
                    children.push(("out", ray.d, e.transmittance * fraction, Some(jones)));
                }
                Kind::Pbs => {
                    let tp = 1. / (1. + 1. / e.extinction_ratio.unwrap_or(1000.));
                    let ts = 1. - tp;
                    let (t, tf) = ray.jones.map_or_else(
                        || (linear(0.), 0.5),
                        |j| jones_axes(j, 0., [[tp.sqrt(), 0.], [ts.sqrt(), 0.]]),
                    );
                    let r = ray.jones.map_or_else(
                        || [[0., 0.], [1., 0.]],
                        |j| jones_axes(j, 0., [[ts.sqrt(), 0.], [tp.sqrt(), 0.]]).0,
                    );
                    children.push(("reflected", reflected, 1. - tf, Some(r)));
                    children.push(("transmitted", ray.d, tf, Some(t)));
                }
                _ => {}
            }
            let transmitted: f64 = children.iter().map(|c| c.2).sum();
            out.power.absorbed += ray.power * (1. - transmitted).max(0.);
            for (port, d, fraction, jones) in children {
                if fraction <= 0. {
                    continue;
                }
                let mut kept = fraction;
                if let Some(model) = &e.scatter {
                    let axis = scale(n, if dot(d, n) >= 0. { 1. } else { -1. });
                    let tis = model.tis(d, axis);
                    if tis > 0. {
                        kept = fraction * (1. - tis);
                        for (k, (direction, weight)) in
                            model.sample(d, axis).into_iter().enumerate()
                        {
                            queue.push_back(Packet {
                                source: ray.source.clone(),
                                branch: format!("{}/{}:scatter{k}", ray.branch, e.id),
                                p: hit,
                                d: direction,
                                power: ray.power * fraction * weight,
                                depth: ray.depth + 1,
                                expected: None,
                                wavelength: ray.wavelength,
                                jones,
                            });
                        }
                    }
                }
                if kept > 0. {
                    queue.push_back(Packet {
                        source: ray.source.clone(),
                        branch: format!("{}/{}:{}", ray.branch, e.id, port),
                        p: hit,
                        d,
                        power: ray.power * kept,
                        depth: ray.depth + 1,
                        expected: next(&e.id, port),
                        wavelength: ray.wavelength,
                        jones,
                    });
                }
            }
        }
        out.detectors = self
            .elements
            .iter()
            .filter(|e| e.kind == Kind::Detector && e.pixels.is_some())
            .map(|e| {
                let bins = e.pixels.unwrap();
                let (power, total, centroid, rms) = match maps.get(e.id.as_str()) {
                    Some(acc) if acc.total > 0. => {
                        let centroid = [acc.sum[0] / acc.total, acc.sum[1] / acc.total];
                        let rms = [0, 1].map(|i| {
                            (acc.sum2[i] / acc.total - centroid[i] * centroid[i])
                                .max(0.)
                                .sqrt()
                        });
                        (acc.power.clone(), acc.total, centroid, rms)
                    }
                    _ => (vec![0.; bins[0] * bins[1]], 0., [0.; 2], [0.; 2]),
                };
                DetectorMap {
                    id: e.id.clone(),
                    bins,
                    extent: e.aperture,
                    power,
                    total,
                    centroid,
                    rms,
                }
            })
            .collect();
        out.balance_error = out.power.launched
            - out.power.detected
            - out.power.absorbed
            - out.power.blocked
            - out.power.escaped
            - out.power.truncated;
        out.warnings = self.assembly()["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        Ok(out)
    }
    pub fn assembly(&self) -> Value {
        let mut warnings = vec![];
        let mut bom: HashMap<String, usize> = HashMap::new();
        let items:Vec<_>=self.elements.iter().map(|e| {
            let p=sub(e.position,self.origin);
            if p[2]<e.body_radius {warnings.push(format!("{}: optical body envelope intersects table",e.id));}
            let holes=[(p[0]/self.hole_pitch).round(),(p[1]/self.hole_pitch).round()];
            if e.mount.is_none(){warnings.push(format!("{}: mount dimensions unspecified",e.id));}
            if let Some(m)=&e.mount {
                if p[2]<m.height_range[0] || p[2]>m.height_range[1] {warnings.push(format!("{}: optical height outside mount adjustment range",e.id));}
                let a=e.rotation[2].to_radians();let w=(a.cos().abs()*m.base_size[0]+a.sin().abs()*m.base_size[1])/2.;let h=(a.sin().abs()*m.base_size[0]+a.cos().abs()*m.base_size[1])/2.;
                if p[0]-w<0. || p[1]-h<0. || p[0]+w>self.table_size[0] || p[1]+h>self.table_size[1] {warnings.push(format!("{}: mount envelope outside table",e.id));}
            }
            let part=e.mount.as_ref().map(|m|m.part.as_str()).unwrap_or("unspecified");
            *bom.entry(format!("{:?} / {}",e.kind,part)).or_default()+=1;
            json!({"id":e.id,"label":e.label,"kind":e.kind,"position":p,"rotation":e.rotation,"normal":rotate([1.,0.,0.],e.rotation),"hole":holes,"hole_offset":[p[0]-holes[0]*self.hole_pitch,p[1]-holes[1]*self.hole_pitch],"mount":e.mount,"lens_model":match e.kind { Kind::Lens => Some("ideal thin lens; slope approximation"), Kind::LensGroup => Some("current sequential prescription; real surfaces; scalar group transmission"), _ => None }})
        }).collect();
        for (i, a) in self.elements.iter().enumerate() {
            for b in &self.elements[i + 1..] {
                if let (Some(ma), Some(mb)) = (&a.mount, &b.mount) {
                    // Separating-axis test for upright, yaw-rotated rectangular bases.
                    let overlap = [
                        a.rotation[2],
                        a.rotation[2] + 90.,
                        b.rotation[2],
                        b.rotation[2] + 90.,
                    ]
                    .iter()
                    .all(|angle| {
                        let theta = angle.to_radians();
                        let axis = [theta.cos(), theta.sin(), 0.];
                        let extent = |e: &Element, m: &Mount| {
                            let da = (e.rotation[2] - angle).to_radians();
                            (da.cos().abs() * m.base_size[0] + da.sin().abs() * m.base_size[1]) / 2.
                        };
                        dot(sub(a.position, b.position), axis).abs() < extent(a, ma) + extent(b, mb)
                    });
                    if overlap {
                        warnings.push(format!("{} / {}: mount bases overlap", a.id, b.id));
                    }
                }
            }
        }
        let mut parts: Vec<_> = bom.into_iter().collect();
        parts.sort_by(|a, b| a.0.cmp(&b.0));
        let positions: HashMap<_, _> = self
            .elements
            .iter()
            .map(|e| (e.id.as_str(), e.position))
            .chain(self.sources.iter().map(|s| (s.id.as_str(), s.position)))
            .collect();
        let connections:Vec<_>=self.connections.iter().map(|c|json!({"from":c.from,"port":c.port,"to":c.to,"distance_mm":norm(sub(positions[c.from.as_str()],positions[c.to.as_str()]))})).collect();
        let sources:Vec<_>=self.sources.iter().map(|s|json!({"id":s.id,"position":sub(s.position,self.origin),"direction":unit(s.direction).unwrap_or([0.;3]),"wavelength":s.wavelength,"power":s.power,"mount":null})).collect();
        json!({"name":self.name,"units":{"length":"mm","angle":"degrees","wavelength":"nm","power":"relative source units"},"axes":"table XY; Z height; rotations [roll,pitch,yaw], Rz Ry Rx; local normal +X","origin":self.origin,"hole_pitch":self.hole_pitch,"items":items,"sources":sources,"bom":parts,"connections":connections,"warnings":warnings,"scope":"geometric rays; ideal thin lenses and current-prescription lens groups; scalar group/splitter power; upright mount boxes; connection distances join component centers"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half_wave_plate_switches_pbs_output_with_finite_extinction() {
        let mut scene = super::super::bench_lab::template("polarized_beam_combiner").unwrap();
        scene.elements[0].transmittance = 0.8;
        let system = System::default();
        let major = 1. / (1. + 1. / 1000.);
        for (axis, transmitted, reflected) in [(0., major, 1. - major), (45., 1. - major, major)] {
            scene.elements[0].fast_axis_deg = Some(axis);
            let trace = scene.trace(&system).unwrap();
            accounted(&trace);
            for (id, expected) in [("DT", transmitted), ("DR", reflected)] {
                let end = trace
                    .terminations
                    .iter()
                    .find(|t| t.element.as_deref() == Some(id))
                    .unwrap();
                assert_eq!(end.reason, "detected");
                close(end.power, 0.8 * expected);
            }
            close(trace.power.absorbed, 0.2);
        }

        scene.sources[0].polarization = Some(Polarization::Unpolarized {});
        let trace = scene.trace(&system).unwrap();
        accounted(&trace);
        for (id, expected) in [("DT", [[1., 0.], [0., 0.]]), ("DR", [[0., 0.], [1., 0.]])] {
            let segment = trace
                .segments
                .iter()
                .find(|s| s.element.as_deref() == Some(id))
                .unwrap();
            close(segment.power, 0.4);
            assert_eq!(segment.jones, Some(expected));
        }
    }

    #[test]
    fn quarter_wave_plate_and_linear_polarizer_transform_jones_fields() {
        let mut scene = super::super::bench_lab::template("polarized_beam_combiner").unwrap();
        let waveplate = &mut scene.elements[0];
        waveplate.fast_axis_deg = Some(0.);
        waveplate.retardance_waves = Some(0.25);
        waveplate.transmittance = 1.;
        scene.sources[0].polarization = Some(Polarization::Diagonal45 {});
        let polarizer = &mut scene.elements[1];
        polarizer.kind = Kind::Polarizer;
        polarizer.reflectance = 0.;
        polarizer.transmittance = 0.8;
        polarizer.fast_axis_deg = Some(90.);
        scene.connections.retain(|c| c.port != "reflected");
        scene.connections[2].port = "out".into();
        let trace = scene.trace(&System::default()).unwrap();
        accounted(&trace);
        let field = trace.segments[1].jones.unwrap();
        close(field[0][0], std::f64::consts::FRAC_1_SQRT_2);
        close(field[1][0], 0.);
        close(field[1][1], std::f64::consts::FRAC_1_SQRT_2);
        close(trace.power.detected, 0.8 * 0.5 * 1.001);
        scene.sources[0].polarization = None;
        let unpolarized = scene.trace(&System::default()).unwrap();
        close(unpolarized.power.detected, trace.power.detected);
        let field = unpolarized.segments[2].jones.unwrap();
        close(field[1][0], 1.);
        scene.elements[0].transmittance = 0.;
        let opaque = scene.trace(&System::default()).unwrap();
        close(opaque.power.absorbed, 1.);
        close(opaque.power.detected, 0.);
    }

    #[test]
    fn curved_mirror_brings_reflected_disk_to_its_focus() {
        let mut scene = super::super::bench_lab::template("periscope_routing").unwrap();
        scene.elements.remove(1);
        let mirror = &mut scene.elements[0];
        mirror.kind = Kind::CurvedMirror;
        mirror.position = [350., 200., 75.];
        mirror.rotation = [0.; 3];
        mirror.reflectance = 0.85;
        mirror.focal_length = 100.;
        let detector = &mut scene.elements[1];
        detector.position = [250., 200., 75.];
        detector.rotation = [0.; 3];
        detector.aperture = 0.01;
        scene.sources[0].position = [300., 200., 75.];
        scene.sources[0].beam_radius = 4.;
        scene.sources[0].rings = 1;
        scene.connections.pop();
        scene.connections[1].to = detector.id.clone();
        let trace = scene.trace(&System::default()).unwrap();
        accounted(&trace);
        close(trace.power.absorbed, 0.15);
        close(trace.power.detected, 0.85);
        for end in trace.terminations {
            assert_eq!(end.reason, "detected");
            close(norm(sub(end.position, scene.elements[1].position)), 0.);
        }
    }

    fn group_scene() -> (System, Bench) {
        let system = System::default();
        let mut bench = example();
        let mut group = bench.elements[0].clone();
        group.id = "G".into();
        group.kind = Kind::LensGroup;
        group.position = [100., 200., 75.];
        group.rotation = [0.; 3];
        group.reflectance = 0.;
        group.transmittance = 1.;
        group.mount = None;
        let mut detector = group.clone();
        detector.id = "D".into();
        detector.kind = Kind::Detector;
        detector.position[0] = 160.;
        detector.aperture = 100.;
        detector.body_radius = 100.;
        bench.elements = vec![group, detector];
        bench.sources.truncate(1);
        bench.sources[0].position = [50., 202., 75.];
        bench.sources[0].direction = [1., 0., 0.];
        bench.sources[0].rings = 0;
        bench.sources[0].beam_radius = 0.;
        bench.connections = vec![
            Connection {
                from: bench.sources[0].id.clone(),
                port: "out".into(),
                to: "G".into(),
            },
            Connection {
                from: "G".into(),
                port: "out".into(),
                to: "D".into(),
            },
        ];
        (system, bench)
    }
    #[test]
    fn lens_group_matches_real_prescription_and_tracks_edits() {
        let (mut system, bench) = group_scene();
        let trace = bench.trace(&system).unwrap();
        accounted(&trace);
        close(trace.power.detected, 1.);
        let ray = system
            .trace_path(
                Launch {
                    point: [2., 0., -50.],
                    direction: [0., 0., 1.],
                    entrance_opl: 0.,
                    object_point: None,
                },
                bench.sources[0].wavelength,
                &Field::meridional(0.),
                system.surfaces.len() - 1,
                true,
                false,
            )
            .unwrap();
        let hit = *ray.points.last().unwrap();
        let step = (60. - hit[2]) / ray.direction[2];
        let expected = add(hit, scale(ray.direction, step));
        let end = trace
            .terminations
            .iter()
            .find(|t| t.reason == "detected")
            .unwrap();
        close(end.position[0], 160.);
        close(end.position[1], 200. + expected[0]);
        close(end.position[2], 75. + expected[1]);
        assert!(trace.segments.iter().any(|s| s.status == "group_surface"));
        system.surfaces[0].radius = 80.;
        let changed = bench.trace(&system).unwrap();
        assert!((changed.terminations[0].position[1] - end.position[1]).abs() > 1e-4);
    }
    #[test]
    fn lens_group_spatial_pose_preserves_physical_path() {
        let (system, mut bench) = group_scene();
        let original = bench.trace(&system).unwrap();
        let angles = [17., 23., 31.];
        let offset = [100., 300., 200.];
        for e in &mut bench.elements {
            e.position = add(rotate(e.position, angles), offset);
            e.rotation = angles;
        }
        for s in &mut bench.sources {
            s.position = add(rotate(s.position, angles), offset);
            s.direction = rotate(s.direction, angles);
        }
        let posed = bench.trace(&system).unwrap();
        accounted(&posed);
        close(posed.power.detected, 1.);
        assert_eq!(posed.segments.len(), original.segments.len());
        for (a, b) in original.segments.iter().zip(&posed.segments) {
            let expected = add(rotate(a.to, angles), offset);
            for i in 0..3 {
                close(expected[i], b.to[i]);
            }
        }
    }
    #[test]
    fn lens_group_clipping_and_internal_obstruction_retain_evidence() {
        let (mut system, mut bench) = group_scene();
        system.surfaces[1].aperture = 0.5;
        system.surfaces[1].clear_aperture = true;
        let clipped = bench.trace(&system).unwrap();
        accounted(&clipped);
        close(clipped.power.blocked, 1.);
        assert_eq!(clipped.terminations[0].reason, "group_blocked");
        assert!(clipped.segments.len() >= 2);
        system.surfaces[1].aperture = 12.5;
        let mut obstacle = bench.elements[1].clone();
        obstacle.id = "inside".into();
        obstacle.kind = Kind::Blocker;
        obstacle.position[0] = 102.;
        bench.elements.push(obstacle);
        let blocked = bench.trace(&system).unwrap();
        accounted(&blocked);
        close(blocked.power.blocked, 1.);
        assert_eq!(blocked.terminations[0].element.as_deref(), Some("inside"));
        assert_eq!(blocked.terminations[0].reason, "group_obstruction");
        close(blocked.terminations[0].position[0], 102.);
    }
    #[test]
    fn lens_group_power_budget_and_dispersion() {
        let (system, mut bench) = group_scene();
        bench.elements[0].transmittance = 0.7;
        let trace = bench.trace(&system).unwrap();
        accounted(&trace);
        close(trace.power.detected, 0.7);
        close(trace.power.absorbed, 0.3);
        bench.sources[0].wavelength = 486.1327;
        let blue = bench.trace(&system).unwrap();
        bench.sources[0].wavelength = 656.2725;
        let red = bench.trace(&system).unwrap();
        assert!((blue.terminations[0].position[1] - red.terminations[0].position[1]).abs() > 1e-5);
        bench.max_interactions = 1;
        let limited = bench.trace(&system).unwrap();
        accounted(&limited);
        close(limited.power.truncated, 1.);
    }
    #[test]
    fn lens_group_boundary_and_entry_failures_are_explicit() {
        let (mut system, mut bench) = group_scene();
        system.surfaces[1].material = "N-BK7_SCHOTT".into();
        assert!(bench.trace(&system).unwrap_err().contains("air"));
        system = System::default();
        system.surfaces[0].interaction = Interaction::Reflect;
        assert!(bench.trace(&system).unwrap_err().contains("forward"));
        system = System::default();
        bench.elements.truncate(1);
        bench.connections.truncate(1);
        bench.sources[0].position = [120., 202., 75.];
        bench.sources[0].direction = [-1., 0., 0.];
        let rear = bench.trace(&system).unwrap();
        accounted(&rear);
        close(rear.power.blocked, 1.);
        assert_eq!(rear.terminations[0].reason, "group_entry");
    }
    #[test]
    fn fresnel_splitter_and_coatings_match_closed_form() {
        let mut b = example();
        b.elements[0].index = Some(1.5);
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        let r = coating::fresnel(1., 1.5, 0.5f64.sqrt()).unwrap();
        close(t.power.detected, 1.);
        let detected = |t: &Trace, id: &str| {
            t.terminations
                .iter()
                .filter(|t| t.element.as_deref() == Some(id))
                .map(|t| t.power)
                .sum()
        };
        close(detected(&t, "D2"), r);
        close(detected(&t, "D1"), 1. - r);
        let mut b = example();
        b.elements[0].coating = Some(Coating {
            layers: vec![Layer {
                index: 1.38,
                thickness: 532. / 4. / 1.38,
            }],
            substrate_index: 1.5,
        });
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        let (rc, tc) =
            coating::stack(532., 0.5f64.sqrt(), &[(1.38, 532. / 4. / 1.38)], 1.5).unwrap();
        close(detected(&t, "D2"), rc);
        close(detected(&t, "D1"), tc);
        close(t.power.absorbed, 1. - rc - tc);
        let mut b = example();
        b.elements[1].coating = Some(Coating {
            layers: vec![],
            substrate_index: 1.5,
        });
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        let bare = coating::fresnel(1., 1.5, 0.5f64.sqrt()).unwrap();
        close(detected(&t, "D2"), 0.5 * bare);
        close(t.power.absorbed, 0.5 * (1. - bare));
    }
    #[test]
    fn lambertian_scatter_conserves_power_and_zero_fraction_is_exact() {
        let mut b = example();
        let baseline = b.trace(&System::default()).unwrap();
        b.elements[0].scatter = Some(Scatter::Lambertian {
            fraction: 0.,
            rays: 8,
        });
        let zero = b.trace(&System::default()).unwrap();
        assert_eq!(zero.power.detected, baseline.power.detected);
        assert_eq!(zero.segments.len(), baseline.segments.len());
        b.elements[0].scatter = Some(Scatter::Lambertian {
            fraction: 0.25,
            rays: 8,
        });
        b.sources[0].rings = 0;
        b.sources[0].beam_radius = 0.;
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        assert!(t.power.detected >= 0.75);
        assert!(t.power.escaped > 0.);
        assert!(
            t.segments
                .iter()
                .filter(|s| s.branch.contains(":scatter"))
                .count()
                >= 16
        );
        let repeat = b.trace(&System::default()).unwrap();
        assert_eq!(repeat.power.detected, t.power.detected);
        assert_eq!(repeat.segments.len(), t.segments.len());
    }
    #[test]
    fn abg_weights_sum_to_tis_deterministically() {
        let model = Scatter::Abg {
            a: 1e-3,
            b: 1e-2,
            g: 2.,
            rays: 16,
        };
        let axis = [1., 0., 0.];
        let specular = unit([1., 0.2, 0.]).unwrap();
        let a = model.sample(specular, axis);
        assert_eq!(a, model.sample(specular, axis));
        assert_eq!(a.len(), 16);
        let sum: f64 = a.iter().map(|s| s.1).sum();
        close(sum, model.tis(specular, axis));
        assert!(sum > 0. && sum < 1.);
        for (d, _) in &a {
            assert!(dot(*d, axis) > 0.);
        }
        let lambert = Scatter::Lambertian {
            fraction: 0.3,
            rays: 5,
        };
        let s = lambert.sample(specular, axis);
        assert_eq!(s.len(), 5);
        for (d, w) in &s {
            close(*w, 0.06);
            assert!(dot(*d, axis) > 0.);
        }
    }
    #[test]
    fn detector_maps_record_irradiance_and_centroid() {
        let mut b = example();
        b.elements[3].pixels = Some([4, 4]);
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        assert_eq!(t.detectors.len(), 1);
        let map = &t.detectors[0];
        assert_eq!(map.id, "D1");
        assert_eq!(map.power.len(), 16);
        close(map.total, 0.5);
        close(map.total, map.power.iter().sum());
        close(map.power[2 * 4 + 2], 0.5);
        close(map.centroid[0], 0.);
        close(map.centroid[1], 0.);
        assert!(map.rms[0] < 1e-9 && map.rms[1] < 1e-9);
    }
    #[test]
    fn group_ghosts_split_fresnel_power_and_conserve() {
        let (system, mut bench) = group_scene();
        bench.sources[0].position = [50., 200., 75.];
        let n = system
            .material_index("N-BK7_SCHOTT", bench.sources[0].wavelength)
            .unwrap();
        let r = ((n - 1.) / (n + 1.)).powi(2);
        bench.elements[0].ghosts = 1;
        let one = bench.trace(&system).unwrap();
        accounted(&one);
        close(one.power.detected, (1. - r).powi(2));
        close(one.power.escaped, r + r * (1. - r).powi(2));
        close(one.power.truncated, (1. - r) * r.powi(2));
        assert!(one.segments.iter().any(|s| s.status == "group_ghost"));
        bench.elements[0].ghosts = 2;
        let two = bench.trace(&system).unwrap();
        accounted(&two);
        assert!(two.power.truncated < one.power.truncated);
        assert!(two.power.detected > one.power.detected);
        bench.elements[0].ghosts = 0;
        let zero = bench.trace(&system).unwrap();
        accounted(&zero);
        close(zero.power.detected, 1.);
    }
    #[test]
    fn stray_light_parameters_are_validated() {
        let mut b = example();
        b.elements[0].index = Some(1.5);
        b.validate().unwrap();
        b.elements[0].index = Some(5.);
        assert!(b.validate().is_err());
        b.elements[0].index = None;
        b.elements[2].index = Some(1.5);
        assert!(b.validate().is_err());
        b.elements[2].index = None;
        b.elements[1].scatter = Some(Scatter::Lambertian {
            fraction: 1.5,
            rays: 8,
        });
        assert!(b.validate().is_err());
        b.elements[1].scatter = Some(Scatter::Lambertian {
            fraction: 0.1,
            rays: 0,
        });
        assert!(b.validate().is_err());
        b.elements[1].scatter = Some(Scatter::Abg {
            a: 0.,
            b: 1.,
            g: 2.,
            rays: 8,
        });
        b.validate().unwrap();
        b.elements[3].scatter = b.elements[1].scatter;
        assert!(b.validate().is_err());
        b.elements[3].scatter = None;
        b.elements[2].coating = Some(Coating {
            layers: vec![],
            substrate_index: 1.5,
        });
        assert!(b.validate().is_err());
        b.elements[2].coating = None;
        b.elements[1].coating = Some(Coating {
            layers: vec![
                Layer {
                    index: 1.38,
                    thickness: 100.,
                };
                33
            ],
            substrate_index: 1.5,
        });
        assert!(b.validate().is_err());
        b.elements[1].coating = None;
        b.elements[0].ghosts = 1;
        assert!(b.validate().is_err());
        b.elements[0].ghosts = 3;
        assert!(b.validate().is_err());
        b.elements[0].ghosts = 0;
        b.elements[3].pixels = Some([4, 4]);
        b.validate().unwrap();
        b.elements[3].pixels = Some([1, 4]);
        assert!(b.validate().is_err());
        b.elements[3].pixels = None;
        b.elements[0].pixels = Some([4, 4]);
        assert!(b.validate().is_err());
    }
    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }
    fn accounted(t: &Trace) {
        close(t.balance_error, 0.);
        close(
            t.power.launched,
            t.power.detected
                + t.power.absorbed
                + t.power.blocked
                + t.power.escaped
                + t.power.truncated,
        );
    }
    #[test]
    fn split_fold_focus_and_absorption() {
        let mut b = example();
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        close(t.power.detected, 1.);
        let d1: Vec<_> = t
            .terminations
            .iter()
            .filter(|t| t.element.as_deref() == Some("D1"))
            .collect();
        assert_eq!(d1.len(), 7);
        for hit in d1 {
            close(hit.position[1], 200.);
            close(hit.position[2], 75.);
        }
        close(
            t.terminations
                .iter()
                .filter(|t| t.element.as_deref() == Some("D2"))
                .map(|t| t.power)
                .sum(),
            0.5,
        );
        b.elements[0].reflectance = 0.3;
        b.elements[0].transmittance = 0.6;
        b.elements[1].reflectance = 0.8;
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        close(t.power.detected, 0.84);
        close(t.power.absorbed, 0.16);
    }
    #[test]
    fn cascaded_splitters_keep_three_distinct_arms() {
        let mut b = example();
        let mut splitter = b.elements[0].clone();
        splitter.id = "BS2".into();
        splitter.position = [400., 200., 75.];
        splitter.reflectance = 0.25;
        splitter.transmittance = 0.75;
        let mut detector = b.elements[3].clone();
        detector.id = "D3".into();
        detector.position = [400., 400., 75.];
        detector.rotation = [0., 0., 90.];
        b.elements.extend([splitter, detector]);
        b.connections
            .iter_mut()
            .find(|c| c.from == "BS1" && c.port == "transmitted")
            .unwrap()
            .to = "BS2".into();
        b.connections.extend([
            Connection {
                from: "BS2".into(),
                port: "transmitted".into(),
                to: "L1".into(),
            },
            Connection {
                from: "BS2".into(),
                port: "reflected".into(),
                to: "D3".into(),
            },
        ]);
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        for (id, power) in [("D1", 0.375), ("D2", 0.5), ("D3", 0.125)] {
            close(
                t.terminations
                    .iter()
                    .filter(|t| t.element.as_deref() == Some(id) && t.reason == "detected")
                    .map(|t| t.power)
                    .sum(),
                power,
            );
        }
    }
    #[test]
    fn local_coordinates_and_spatial_mirror() {
        let v = [3., -2., 8.];
        let a = [23., -62., 111.];
        let back = local(rotate(v, a), a);
        for i in 0..3 {
            close(v[i], back[i]);
        }
        let mut b = example();
        b.elements.retain(|e| e.id == "BS1" || e.id == "D1");
        b.connections.clear();
        b.sources[0].rings = 0;
        for e in &mut b.elements {
            e.mount = None;
        }
        b.elements[0].kind = Kind::Mirror;
        b.elements[0].reflectance = 1.;
        b.elements[0].transmittance = 0.;
        b.elements[0].rotation = [0., 45., 0.];
        b.elements[1].position = [300., 200., 275.];
        b.elements[1].rotation = [0., 90., 0.];
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        close(t.power.detected, 1.);
        close(t.terminations[0].position[2], 275.);
    }
    #[test]
    fn rigid_yaw_and_translation_preserve_paths() {
        let mut b = example();
        let before = b.trace(&System::default()).unwrap();
        let offset = [231., -138., 24.];
        let angle = [0., 0., 37.];
        for e in &mut b.elements {
            e.mount = None;
            e.position = add(rotate(e.position, angle), offset);
            e.rotation[2] += 37.;
        }
        for s in &mut b.sources {
            s.position = add(rotate(s.position, angle), offset);
            s.direction = rotate(s.direction, angle);
        }
        let after = b.trace(&System::default()).unwrap();
        accounted(&after);
        close(before.power.detected, after.power.detected);
        assert_eq!(before.segments.len(), after.segments.len());
        for (a, b) in before.segments.iter().zip(&after.segments) {
            close(norm(sub(a.to, a.from)), norm(sub(b.to, b.from)));
            for i in 0..3 {
                close(add(rotate(a.to, angle), offset)[i], b.to[i]);
            }
        }
    }
    #[test]
    fn physical_clipping_and_intervening_mounts() {
        let mut b = example();
        b.sources[0].beam_radius = 13.;
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        close(t.power.blocked, 6. / 7.);
        let mut b = example();
        let mut obstacle = b.elements[0].clone();
        obstacle.id = "obstruction".into();
        obstacle.position = [200., 200., 110.];
        obstacle.rotation = [0.; 3];
        b.elements.push(obstacle);
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        close(t.power.blocked, 1.);
        assert!(
            t.terminations
                .iter()
                .all(|t| t.reason == "mount_obstruction")
        );
        b.elements.last_mut().unwrap().mount = None;
        b.elements.last_mut().unwrap().position = [200., 200., 75.];
        let t = b.trace(&System::default()).unwrap();
        assert!(t.terminations.iter().all(|t| t.reason == "unexpected_hit"));
    }
    #[test]
    fn loops_and_packet_cutoffs_keep_power() {
        let mut b = example();
        b.elements.retain(|e| e.id == "BS1" || e.id == "M1");
        b.sources[0].position = [200., 200., 75.];
        b.sources[0].rings = 0;
        for (i, e) in b.elements.iter_mut().enumerate() {
            e.kind = Kind::Mirror;
            e.mount = None;
            e.reflectance = 1.;
            e.transmittance = 0.;
            e.rotation = [0.; 3];
            e.position = [if i == 0 { 300. } else { 100. }, 200., 75.];
        }
        b.connections = vec![
            Connection {
                from: "S1".into(),
                port: "out".into(),
                to: "BS1".into(),
            },
            Connection {
                from: "BS1".into(),
                port: "reflected".into(),
                to: "M1".into(),
            },
            Connection {
                from: "M1".into(),
                port: "reflected".into(),
                to: "BS1".into(),
            },
        ];
        b.max_interactions = 5;
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        assert_eq!(t.segments.len(), 5);
        close(t.power.truncated, 1.);
        let mut b = example();
        b.min_power = 0.1;
        let t = b.trace(&System::default()).unwrap();
        accounted(&t);
        close(t.power.truncated, 1.);
    }
    #[test]
    fn forty_components_and_assembly_geometry() {
        let mut b = example();
        let prototype = b.elements[0].clone();
        b.elements.clear();
        b.connections.clear();
        b.max_interactions = 64;
        b.table_size = [1500., 600.];
        for i in 0..40 {
            let mut e = prototype.clone();
            e.id = format!("E{i}");
            e.position = [150. + 25. * i as f64, 200., 75.];
            e.rotation = [0.; 3];
            e.kind = if i == 39 {
                Kind::Detector
            } else {
                Kind::Aperture
            };
            e.mount = None;
            b.elements.push(e);
        }
        let start = std::time::Instant::now();
        let t = b.trace(&System::default()).unwrap();
        eprintln!(
            "40 components / {} segments: {:?}",
            t.segments.len(),
            start.elapsed()
        );
        accounted(&t);
        close(t.power.detected, 1.);
        assert_eq!(t.segments.len(), 280);
        let mut b = example();
        b.origin = [10., 20., 0.];
        let a = b.assembly();
        close(a["items"][0]["position"][0].as_f64().unwrap(), 290.);
        close(a["items"][0]["hole_offset"][0].as_f64().unwrap(), -10.);
        b.elements[1].position = b.elements[0].position;
        assert!(
            b.assembly()["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str().unwrap().contains("overlap"))
        );
    }
    #[test]
    fn invalid_graphs_and_parameters_are_rejected() {
        let b = example();
        let mut invalid = b.clone();
        invalid.elements[0].id = "S1".into();
        assert!(invalid.validate().is_err());
        let mut invalid = b.clone();
        invalid.connections[0].to = "missing".into();
        assert!(invalid.validate().is_err());
        let mut invalid = b.clone();
        invalid.connections.push(invalid.connections[0].clone());
        assert!(invalid.validate().is_err());
        let mut invalid = b.clone();
        invalid.elements[0].reflectance = 0.7;
        assert!(invalid.validate().is_err());
        let mut invalid = b.clone();
        invalid.sources[0].direction = [0.; 3];
        assert!(invalid.validate().is_err());
        let mut invalid = b;
        invalid.max_interactions = 0;
        assert!(invalid.validate().is_err());
    }
}
pub fn example() -> Bench {
    let element = |id: &str, kind, position, yaw| Element {
        id: id.into(),
        label: id.into(),
        kind,
        position,
        rotation: [0., 0., yaw],
        aperture: 12.5,
        body_radius: 15.,
        focal_length: 100.,
        reflectance: if kind == Kind::Mirror {
            1.
        } else if kind == Kind::Splitter {
            0.5
        } else {
            0.
        },
        transmittance: if kind == Kind::Mirror {
            0.
        } else if kind == Kind::Splitter {
            0.5
        } else {
            1.
        },
        mount: Some(Mount {
            part: "Generic 30 mm mount".into(),
            base_size: [30., 30.],
            height_range: [40., 150.],
        }),
        index: None,
        coating: None,
        scatter: None,
        ghosts: 0,
        pixels: None,
        retardance_waves: None,
        fast_axis_deg: None,
        extinction_ratio: None,
        screw_tpi: None,
        lever_arm_mm: None,
    };
    Bench {
        name: "Split laser experiment".into(),
        table_size: [900., 600.],
        hole_pitch: 25.,
        origin: [0.; 3],
        elements: vec![
            element("BS1", Kind::Splitter, [300., 200., 75.], -45.),
            element("M1", Kind::Mirror, [300., 400., 75.], 45.),
            element("L1", Kind::Lens, [500., 200., 75.], 0.),
            element("D1", Kind::Detector, [600., 200., 75.], 0.),
            element("D2", Kind::Detector, [100., 400., 75.], 0.),
        ],
        sources: vec![Source {
            id: "S1".into(),
            position: [100., 200., 75.],
            direction: [1., 0., 0.],
            wavelength: 532.,
            power: 1.,
            beam_radius: 3.,
            rings: 1,
            waist: None,
            waist_offset: None,
            m2: None,
            polarization: None,
        }],
        connections: vec![
            Connection {
                from: "S1".into(),
                port: "out".into(),
                to: "BS1".into(),
            },
            Connection {
                from: "BS1".into(),
                port: "reflected".into(),
                to: "M1".into(),
            },
            Connection {
                from: "BS1".into(),
                port: "transmitted".into(),
                to: "L1".into(),
            },
            Connection {
                from: "M1".into(),
                port: "reflected".into(),
                to: "D2".into(),
            },
            Connection {
                from: "L1".into(),
                port: "out".into(),
                to: "D1".into(),
            },
        ],
        max_interactions: 32,
        min_power: 1e-8,
    }
}
