//! Free-space geometric bench. World XY is the table; Z is height (mm).
//! Elements use Rz(yaw) Ry(pitch) Rx(roll), with local +X as surface normal.
use super::math::*;
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
    Lens,
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
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub from: String,
    pub port: String,
    pub to: String,
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
fn local(v: V3, angles: V3) -> V3 {
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
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn ports(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Mirror => &["reflected"],
        Kind::Splitter => &["reflected", "transmitted"],
        Kind::Lens | Kind::Aperture => &["out"],
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
                || (e.kind == Kind::Lens && e.focal_length.abs() < 1e-6)
                || !e.reflectance.is_finite()
                || !e.transmittance.is_finite()
                || e.reflectance < 0.
                || e.transmittance < 0.
                || e.reflectance + e.transmittance > 1.
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
#[derive(Debug, Serialize)]
pub struct Trace {
    pub segments: Vec<Segment>,
    pub terminations: Vec<Termination>,
    pub power: Ledger,
    pub balance_error: f64,
    pub warnings: Vec<String>,
}
struct Packet {
    source: String,
    branch: String,
    p: V3,
    d: V3,
    power: f64,
    depth: usize,
    expected: Option<String>,
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
    pub fn trace(&self) -> Result<Trace, String> {
        self.validate()?;
        let mut out = Trace {
            segments: vec![],
            terminations: vec![],
            power: Ledger::default(),
            balance_error: 0.,
            warnings: vec![],
        };
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
            let mut closest: Option<(f64, &Element, bool)> = None;
            for e in &self.elements {
                let p = local(sub(ray.p, e.position), e.rotation);
                let d = local(ray.d, e.rotation);
                if d[0].abs() > 1e-12 {
                    let t = -p[0] / d[0];
                    let h = add(p, scale(d, t));
                    if t > 1e-7
                        && h[1].hypot(h[2]) <= e.body_radius
                        && closest.is_none_or(|a| t < a.0)
                    {
                        closest = Some((t, e, false));
                    }
                }
                if let Some(t) = mount_hit(e, ray.p, ray.d, self.origin[2]) {
                    if closest.is_none_or(|a| t < a.0) {
                        closest = Some((t, e, true));
                    }
                }
            }
            let Some((distance, e, mount)) = closest else {
                let end = add(
                    ray.p,
                    scale(ray.d, self.table_size[0].hypot(self.table_size[1])),
                );
                out.segments.push(Segment {
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
            } else if e.kind == Kind::Blocker {
                Some("beam_stop")
            } else {
                None
            };
            out.segments.push(Segment {
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
            if e.kind == Kind::Detector {
                out.power.detected += ray.power;
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
            let mut children: Vec<(&str, V3, f64)> = vec![];
            match e.kind {
                Kind::Mirror => children.push(("reflected", reflected, e.reflectance)),
                Kind::Splitter => {
                    children.push(("reflected", reflected, e.reflectance));
                    children.push(("transmitted", ray.d, e.transmittance));
                }
                Kind::Lens => {
                    let d = local(ray.d, e.rotation);
                    let direction = unit([
                        d[0].signum(),
                        d[1] / d[0].abs() - h[1] / e.focal_length,
                        d[2] / d[0].abs() - h[2] / e.focal_length,
                    ])?;
                    children.push(("out", rotate(direction, e.rotation), e.transmittance));
                }
                Kind::Aperture => children.push(("out", ray.d, 1.)),
                _ => {}
            }
            let transmitted: f64 = children.iter().map(|c| c.2).sum();
            out.power.absorbed += ray.power * (1. - transmitted);
            for (port, d, fraction) in children {
                if fraction > 0. {
                    queue.push_back(Packet {
                        source: ray.source.clone(),
                        branch: format!("{}/{}:{}", ray.branch, e.id, port),
                        p: hit,
                        d,
                        power: ray.power * fraction,
                        depth: ray.depth + 1,
                        expected: next(&e.id, port),
                    });
                }
            }
        }
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
            json!({"id":e.id,"label":e.label,"kind":e.kind,"position":p,"rotation":e.rotation,"normal":rotate([1.,0.,0.],e.rotation),"hole":holes,"hole_offset":[p[0]-holes[0]*self.hole_pitch,p[1]-holes[1]*self.hole_pitch],"mount":e.mount,"lens_model":if e.kind==Kind::Lens {Some("ideal thin lens; slope approximation")} else {None}})
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
        json!({"name":self.name,"units":{"length":"mm","angle":"degrees","wavelength":"nm","power":"relative source units"},"axes":"table XY; Z height; rotations [roll,pitch,yaw], Rz Ry Rx; local normal +X","origin":self.origin,"hole_pitch":self.hole_pitch,"items":items,"sources":sources,"bom":parts,"connections":connections,"warnings":warnings,"scope":"geometric rays; ideal thin lenses; scalar splitter power; upright mount boxes; connection distances join component centers"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let t = b.trace().unwrap();
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
        let t = b.trace().unwrap();
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
        let t = b.trace().unwrap();
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
        let t = b.trace().unwrap();
        accounted(&t);
        close(t.power.detected, 1.);
        close(t.terminations[0].position[2], 275.);
    }
    #[test]
    fn rigid_yaw_and_translation_preserve_paths() {
        let mut b = example();
        let before = b.trace().unwrap();
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
        let after = b.trace().unwrap();
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
        let t = b.trace().unwrap();
        accounted(&t);
        close(t.power.blocked, 6. / 7.);
        let mut b = example();
        let mut obstacle = b.elements[0].clone();
        obstacle.id = "obstruction".into();
        obstacle.position = [200., 200., 110.];
        obstacle.rotation = [0.; 3];
        b.elements.push(obstacle);
        let t = b.trace().unwrap();
        accounted(&t);
        close(t.power.blocked, 1.);
        assert!(
            t.terminations
                .iter()
                .all(|t| t.reason == "mount_obstruction")
        );
        b.elements.last_mut().unwrap().mount = None;
        b.elements.last_mut().unwrap().position = [200., 200., 75.];
        let t = b.trace().unwrap();
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
        let t = b.trace().unwrap();
        accounted(&t);
        assert_eq!(t.segments.len(), 5);
        close(t.power.truncated, 1.);
        let mut b = example();
        b.min_power = 0.1;
        let t = b.trace().unwrap();
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
        let t = b.trace().unwrap();
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
