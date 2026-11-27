//! Lab planning for the geometric bench. All lengths are in millimetres.
use super::{
    System,
    bench::{self, Bench, Connection, Element, Kind, Mount, Polarization, Trace},
    math::{Matrix, V3, add, dot, norm, scale, sub, unit},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    f64::consts::{PI, TAU},
    fmt::Write,
};

const FORK_REACH: f64 = 22.;
const DIRECT_REACH: f64 = 1.5;

#[derive(Clone, Copy)]
struct Hardware {
    part: &'static str,
    name: &'static str,
    category: &'static str,
    kind: Kind,
    aperture: f64,
    body_radius: f64,
    focal_length: f64,
    reflectance: f64,
    transmittance: f64,
    retardance_waves: Option<f64>,
    fast_axis_deg: Option<f64>,
    extinction_ratio: Option<f64>,
    screw_tpi: Option<f64>,
    lever_arm_mm: Option<f64>,
    clamping_style: &'static str,
    mount: &'static str,
    base_size: [f64; 2],
    typical_price_usd: u32,
    notes: &'static str,
}
impl Hardware {
    fn mount(self) -> Mount {
        Mount {
            part: self.mount.into(),
            base_size: self.base_size,
            height_range: [50., 150.],
        }
    }
}

const LENS: Hardware = Hardware {
    part: "LA1509",
    name: "1 inch plano-convex lens, f = 100 mm",
    category: "lens",
    kind: Kind::Lens,
    aperture: 11.4,
    body_radius: 12.7,
    focal_length: 100.,
    reflectance: 0.,
    transmittance: 0.92,
    retardance_waves: None,
    fast_axis_deg: None,
    extinction_ratio: None,
    screw_tpi: None,
    lever_arm_mm: None,
    clamping_style: "clamping_fork",
    mount: "LMR1/M",
    base_size: [25., 25.],
    typical_price_usd: 55,
    notes: "Model uses nominal focal length and a separate post/base.",
};
const MIRROR: Hardware = Hardware {
    part: "KM100",
    name: "1 inch kinematic mirror mount",
    category: "mount",
    kind: Kind::Mirror,
    focal_length: 0.,
    reflectance: 0.99,
    transmittance: 0.,
    screw_tpi: Some(80.),
    lever_arm_mm: Some(25.4),
    mount: "KM100",
    typical_price_usd: 65,
    notes: "1/4-80 adjusters. Mirror sold separately; lever arm and footprint are planning estimates.",
    ..LENS
};
const SPLITTER: Hardware = Hardware {
    part: "BSW10",
    name: "1 inch nominal 50:50 plate beamsplitter",
    category: "beamsplitter",
    kind: Kind::Splitter,
    focal_length: 0.,
    reflectance: 0.5,
    transmittance: 0.5,
    mount: "KM100B",
    typical_price_usd: 160,
    notes: "Check coating band and polarization dependence at the working wavelength.",
    ..MIRROR
};
const PBS: Hardware = Hardware {
    part: "PBS251",
    name: "25.4 mm visible polarizing beamsplitter cube",
    category: "polarization",
    kind: Kind::Pbs,
    focal_length: 0.,
    reflectance: 0.5,
    transmittance: 0.5,
    extinction_ratio: Some(1000.),
    clamping_style: "slotted_base",
    mount: "KM100B",
    typical_price_usd: 265,
    notes: "420–680 nm. R/T entries are unpolarized shares in the lossless bench model.",
    ..LENS
};
const HWP: Hardware = Hardware {
    part: "WPH10M-532",
    name: "1 inch mounted half-wave plate, 532 nm",
    aperture: 11.3,
    body_radius: 15.25,
    category: "polarization",
    kind: Kind::Waveplate,
    focal_length: 0.,
    transmittance: 0.98,
    retardance_waves: Some(0.5),
    fast_axis_deg: Some(0.),
    mount: "CRM1PT/M",
    typical_price_usd: 500,
    notes: "Set the fast axis in bench coordinates. Rotation mount and post are separate.",
    ..LENS
};
const DETECTOR: Hardware = Hardware {
    part: "S120C",
    name: "Silicon photodiode power sensor",
    category: "detector",
    kind: Kind::Detector,
    aperture: 4.75,
    body_radius: 18.,
    focal_length: 0.,
    transmittance: 0.,
    clamping_style: "direct_hole",
    mount: "TR50/M + base",
    typical_price_usd: 390,
    notes: "9.5 mm entrance aperture; confirm wavelength and power limits with the meter.",
    ..LENS
};
const DUMP: Hardware = Hardware {
    part: "BT610/M",
    name: "Metric beam trap",
    category: "safety",
    kind: Kind::Blocker,
    aperture: 5.5,
    body_radius: 25.,
    clamping_style: "slotted_base",
    mount: "BT610/M",
    typical_price_usd: 290,
    notes: "Use only within the manufacturer's wavelength, power and beam size limits.",
    ..DETECTOR
};
const LENS_50: Hardware = Hardware {
    part: "LA1131",
    name: "1 inch plano-convex lens, f = 50 mm",
    focal_length: 50.,
    ..LENS
};
const LENS_150: Hardware = Hardware {
    part: "LA1433",
    name: "1 inch plano-convex lens, f = 150 mm",
    focal_length: 150.,
    ..LENS
};
const HARDWARE: &[Hardware] = &[
    MIRROR,
    Hardware {
        part: "KM200",
        name: "2 inch kinematic mirror mount",
        aperture: 22.8,
        body_radius: 25.4,
        lever_arm_mm: Some(40.),
        mount: "KM200",
        base_size: [40., 40.],
        typical_price_usd: 100,
        ..MIRROR
    },
    Hardware {
        part: "MPD129-F01",
        name: "1 inch 90 degree off-axis parabolic mirror",
        category: "mirror",
        kind: Kind::CurvedMirror,
        focal_length: 50.8,
        reflectance: 0.9,
        mount: "TR50/M + base",
        typical_price_usd: 250,
        notes: "Nominal reflected focal length; the bench uses a thin focusing reflection.",
        ..MIRROR
    },
    SPLITTER,
    PBS,
    HWP,
    Hardware {
        part: "WPQ10M-532",
        name: "1 inch mounted quarter-wave plate, 532 nm",
        retardance_waves: Some(0.25),
        typical_price_usd: 500,
        ..HWP
    },
    Hardware {
        part: "LPVISE100-A",
        name: "1 inch visible linear polarizer",
        kind: Kind::Polarizer,
        transmittance: 0.85,
        retardance_waves: None,
        extinction_ratio: Some(1000.),
        typical_price_usd: 160,
        notes: "Transmittance and extinction are model inputs; check the wavelength curves.",
        ..HWP
    },
    LENS,
    LENS_50,
    LENS_150,
    Hardware {
        part: "LA1484",
        name: "1 inch plano-convex lens, f = 300 mm",
        focal_length: 300.,
        ..LENS
    },
    Hardware {
        part: "ID25/M",
        name: "Metric iris diaphragm",
        category: "aperture",
        kind: Kind::Aperture,
        aperture: 12.5,
        body_radius: 19.,
        focal_length: 0.,
        transmittance: 1.,
        mount: "TR75/M + base",
        typical_price_usd: 75,
        notes: "Set aperture to the actual iris opening radius.",
        ..LENS
    },
    DETECTOR,
    DUMP,
];

fn kind_name(kind: Kind) -> String {
    serde_json::to_value(kind).unwrap().as_str().unwrap().into()
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct CatalogItem {
    part: &'static str,
    name: &'static str,
    category: &'static str,
    kind: String,
    aperture: f64,
    body_radius: f64,
    focal_length: f64,
    reflectance: f64,
    transmittance: f64,
    retardance_waves: Option<f64>,
    fast_axis_deg: Option<f64>,
    extinction_ratio: Option<f64>,
    screw_tpi: Option<f64>,
    lever_arm_mm: Option<f64>,
    clamping_style: &'static str,
    fork_reach_mm: f64,
    mount: Mount,
    typical_price_usd: u32,
    notes: &'static str,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct Catalog {
    items: Vec<CatalogItem>,
    units: &'static str,
    price_basis: &'static str,
}

pub fn catalog(query: Option<&str>, kind: Option<&str>) -> Value {
    json!(catalog_report(query, kind))
}
pub(crate) fn catalog_report(query: Option<&str>, kind: Option<&str>) -> Catalog {
    let query = query.unwrap_or_default().trim().to_ascii_lowercase();
    let items: Vec<_> = HARDWARE
        .iter()
        .filter_map(|item| {
            let name = kind_name(item.kind);
            if kind.is_some_and(|filter| !name.eq_ignore_ascii_case(filter.trim()))
                || ![
                    item.part, item.name, item.category, item.mount, item.notes, &name,
                ]
                .iter()
                .any(|text| text.to_ascii_lowercase().contains(&query))
            {
                return None;
            }
            Some(CatalogItem {
                part: item.part,
                name: item.name,
                category: item.category,
                kind: name,
                aperture: item.aperture,
                body_radius: item.body_radius,
                focal_length: item.focal_length,
                reflectance: item.reflectance,
                transmittance: item.transmittance,
                retardance_waves: item.retardance_waves,
                fast_axis_deg: item.fast_axis_deg,
                extinction_ratio: item.extinction_ratio,
                screw_tpi: item.screw_tpi,
                lever_arm_mm: item.lever_arm_mm,
                clamping_style: item.clamping_style,
                fork_reach_mm: if item.clamping_style == "direct_hole" {
                    0.
                } else {
                    FORK_REACH
                },
                mount: item.mount(),
                typical_price_usd: item.typical_price_usd,
                notes: item.notes,
            })
        })
        .collect();
    Catalog {
        items,
        units: "mm",
        price_basis: "Approximate USD planning estimates; mount footprints include assumed posts and bases.",
    }
}

pub(crate) const TEMPLATES: &[&str] = &[
    "mach_zehnder",
    "michelson",
    "polarized_beam_combiner",
    "beam_expander_focus",
    "periscope_routing",
];

fn normal_rotation([x, y, z]: V3) -> V3 {
    [
        0.,
        (-z).atan2(x.hypot(y)).to_degrees(),
        y.atan2(x).to_degrees(),
    ]
}

fn mount_element(id: &str, hardware: Hardware, position: V3, normal: V3) -> Element {
    Element {
        id: id.into(),
        label: format!("{id} ({})", hardware.part),
        kind: hardware.kind,
        position,
        rotation: normal_rotation(normal),
        aperture: hardware.aperture,
        body_radius: hardware.body_radius,
        focal_length: hardware.focal_length,
        reflectance: hardware.reflectance,
        transmittance: hardware.transmittance,
        mount: Some(hardware.mount()),
        index: None,
        coating: None,
        scatter: None,
        ghosts: 0,
        pixels: None,
        retardance_waves: hardware.retardance_waves,
        fast_axis_deg: hardware.fast_axis_deg,
        extinction_ratio: hardware.extinction_ratio,
        screw_tpi: hardware.screw_tpi,
        lever_arm_mm: hardware.lever_arm_mm,
    }
}

pub fn template(name: &str) -> Result<Bench, String> {
    const X: V3 = [1., 0., 0.];
    const Y: V3 = [0., 1., 0.];
    const FOLD: V3 = [-1., 1., 0.];
    let mut scene = bench::example();
    scene.sources[0].position = [100., 200., 75.];
    scene.sources[0].beam_radius = 0.7;
    scene.sources[0].waist = Some(0.7);
    scene.sources[0].rings = 0;
    scene.elements.clear();
    scene.connections.clear();
    let at = |x, y| [x, y, 75.];
    let mut place = |id, part, p, n| scene.elements.push(mount_element(id, part, p, n));
    let (label, edges): (&str, &[(&str, &str, &str)]) = match name {
        "mach_zehnder" => {
            place("BS1", SPLITTER, at(250., 200.), FOLD);
            place("M1", MIRROR, at(250., 400.), FOLD);
            place("M2", MIRROR, at(450., 200.), FOLD);
            place("BS2", SPLITTER, at(450., 400.), FOLD);
            place("D1", DETECTOR, at(650., 400.), X);
            place("D2", DETECTOR, at(450., 550.), Y);
            // Either BS2 output can be the reflected port, depending on the incident arm.
            (
                "Mach–Zehnder interferometer",
                &[
                    ("S1", "out", "BS1"),
                    ("BS1", "reflected", "M1"),
                    ("BS1", "transmitted", "M2"),
                    ("M1", "reflected", "BS2"),
                    ("M2", "reflected", "BS2"),
                ],
            )
        }
        "michelson" => {
            scene.sources[0].position = at(75., 400.);
            let return_from_y = unit([0.0625, -1., 0.])?;
            let return_from_x = unit([-1., 0.0625, 0.])?;
            place("BS", SPLITTER, at(450., 400.), FOLD);
            place("M1", MIRROR, at(450., 475.), sub(return_from_y, Y));
            place("M2", MIRROR, at(525., 400.), sub(return_from_x, X));
            place("D", DETECTOR, at(475., 75.), return_from_y);
            place("BD", DUMP, at(125., 425.), return_from_x);
            // Overlap the two returned rays at the splitter and route the
            // source-side output into the dump beside the launch axis.
            (
                "Michelson interferometer",
                &[
                    ("S1", "out", "BS"),
                    ("M1", "reflected", "BS"),
                    ("M2", "reflected", "BS"),
                ],
            )
        }
        "polarized_beam_combiner" => {
            let mut hwp = HWP;
            hwp.fast_axis_deg = Some(22.5);
            place("HWP", hwp, at(175., 200.), X);
            place("PBS", PBS, at(300., 200.), FOLD);
            place("DT", DETECTOR, at(550., 200.), X);
            place("DR", DETECTOR, at(300., 400.), Y);
            scene.sources[0].polarization = Some(Polarization::Horizontal {});
            (
                "Polarized beam combiner",
                &[
                    ("S1", "out", "HWP"),
                    ("HWP", "out", "PBS"),
                    ("PBS", "transmitted", "DT"),
                    ("PBS", "reflected", "DR"),
                ],
            )
        }
        "beam_expander_focus" => {
            place("L1", LENS_50, at(200., 200.), X);
            place("L2", LENS_150, at(400., 200.), X);
            place("M1", MIRROR, at(500., 200.), FOLD);
            place("L3", LENS, at(500., 350.), Y);
            place("D", DETECTOR, at(500., 450.), Y);
            (
                "Beam expander and focus",
                &[
                    ("S1", "out", "L1"),
                    ("L1", "out", "L2"),
                    ("L2", "out", "M1"),
                    ("M1", "reflected", "L3"),
                    ("L3", "out", "D"),
                ],
            )
        }
        "periscope_routing" => {
            let low = at(250., 200.);
            let high = [250., 300., 125.];
            let rise = unit(sub(high, low))?;
            place("M1", MIRROR, low, sub(rise, X));
            place("M2", MIRROR, high, sub(X, rise));
            place("D", DETECTOR, [500., 300., 125.], X);
            (
                "Height-raising periscope",
                &[
                    ("S1", "out", "M1"),
                    ("M1", "reflected", "M2"),
                    ("M2", "reflected", "D"),
                ],
            )
        }
        _ => {
            return Err(format!(
                "Unknown bench template {name:?}. Choose {}",
                TEMPLATES.join(", ")
            ));
        }
    };
    scene.name = label.into();
    scene.connections = edges
        .iter()
        .map(|&(from, port, to)| Connection {
            from: from.into(),
            port: port.into(),
            to: to.into(),
        })
        .collect();
    scene.validate()?;
    Ok(scene)
}

fn reflective(kind: Kind) -> bool {
    bench::ports(kind).contains(&"reflected")
}

pub fn auto_aim(bench: &Bench, snap_holes: bool, uniform_z: Option<f64>) -> Result<Value, String> {
    serde_json::to_value(aim(bench, snap_holes, uniform_z)?).map_err(|e| e.to_string())
}

#[derive(schemars::JsonSchema, Serialize)]
pub(crate) struct AimResult {
    pub bench: Bench,
    adjusted_elements: Vec<String>,
    warnings: Vec<String>,
}

pub(crate) fn aim(
    bench: &Bench,
    snap_holes: bool,
    uniform_z: Option<f64>,
) -> Result<AimResult, String> {
    bench.validate()?;
    if uniform_z.is_some_and(|z| !z.is_finite() || z <= 0.) {
        return Err("Uniform Z must be finite and positive".into());
    }
    let mut scene = bench.clone();
    for element in &mut scene.elements {
        if snap_holes {
            for i in 0..2 {
                let origin = scene.origin[i];
                element.position[i] = origin
                    + ((element.position[i] - origin) / scene.hole_pitch).round()
                        * scene.hole_pitch;
            }
        }
        if let Some(z) = uniform_z {
            element.position[2] = z;
        }
    }
    let indices: HashMap<_, _> = scene
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.clone(), i))
        .collect();
    let mut pending = VecDeque::new();
    let mut warnings = Vec::new();
    for source in &mut scene.sources {
        if let Some(z) = uniform_z {
            source.position[2] = z;
        }
        if let Some(link) = scene.connections.iter().find(|c| c.from == source.id) {
            let target = indices[&link.to];
            let direction = unit(sub(scene.elements[target].position, source.position))
                .map_err(|_| format!("{} and {} have coincident positions", source.id, link.to))?;
            source.direction = direction;
            pending.push_back((target, direction));
        }
    }
    let mut incoming = HashMap::new();
    while let Some((i, direction)) = pending.pop_front() {
        let element = &scene.elements[i];
        if let Some(&first) = incoming.get(&i) {
            if dot(first, direction) < 1. - 1e-9 {
                warnings.push(format!(
                    "{} receives multiple directions; aimed for the first path",
                    element.id
                ));
            }
            continue;
        }
        incoming.insert(i, direction);
        let mut normal = (!reflective(element.kind)).then_some(direction);
        for link in scene.connections.iter().filter(|c| c.from == element.id) {
            let j = indices[&link.to];
            let Ok(out) = unit(sub(scene.elements[j].position, element.position)) else {
                warnings.push(format!(
                    "{} / {}: coincident connection endpoints",
                    element.id, link.to
                ));
                continue;
            };
            if link.port == "reflected" {
                normal = unit(sub(out, direction)).ok();
                if normal.is_none() {
                    warnings.push(format!(
                        "{}: reflected target requires zero deflection",
                        element.id
                    ));
                }
            } else if element.kind != Kind::LensGroup && dot(out, direction) < 1. - 1e-9 {
                warnings.push(format!(
                    "{} / {}: straight-through target is off the incoming axis",
                    element.id, link.to
                ));
            }
            pending.push_back((j, out));
        }
        if let Some(normal) = normal {
            scene.elements[i].rotation = normal_rotation(normal);
        } else if !scene
            .connections
            .iter()
            .any(|c| c.from == element.id && c.port == "reflected")
        {
            warnings.push(format!("{}: no reflected target to aim at", element.id));
        }
    }
    for (i, e) in scene.elements.iter().enumerate() {
        if !incoming.contains_key(&i) {
            warnings.push(format!("{}: no incoming connection from a source", e.id));
        }
    }
    scene.validate()?;
    let adjusted: Vec<_> = scene
        .elements
        .iter()
        .zip(&bench.elements)
        .filter(|(new, old)| new.rotation != old.rotation || new.position != old.position)
        .map(|(e, _)| e.id.clone())
        .collect();
    Ok(AimResult {
        bench: scene,
        adjusted_elements: adjusted,
        warnings,
    })
}

#[derive(Clone, Copy)]
struct Q {
    re: f64,
    im: f64,
}
impl Q {
    fn transfer(self, distance: f64) -> Self {
        Self {
            re: self.re + distance,
            ..self
        }
    }

    fn transform(self, m: Matrix, n_in: f64, n_out: f64) -> Self {
        // Convert reduced coordinates [height, n*slope] to geometrical slopes.
        let (b, c, d) = (m.b * n_in, m.c / n_out, m.d * n_in / n_out);
        let nr = m.a * self.re + b;
        let ni = m.a * self.im;
        let dr = c * self.re + d;
        let di = c * self.im;
        let magnitude = dr.hypot(di);
        Self {
            re: (nr / magnitude) * (dr / magnitude) + (ni / magnitude) * (di / magnitude),
            // ABCD determinants are n_in / n_out. Avoid subtracting two nearly
            // equal complex products at a strongly converging focus.
            im: ((self.im / magnitude) / magnitude) * (n_in / n_out),
        }
    }

    fn radius(self, wavelength_mm: f64, index: f64) -> Result<f64, String> {
        let radius = self.re.hypot(self.im) * (wavelength_mm / PI / index / self.im).sqrt();
        if radius.is_finite() && radius > 0. {
            Ok(radius)
        } else {
            Err(
                "Gaussian radius outside the numeric range; check waist, M² and focal length"
                    .into(),
            )
        }
    }
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
struct EnvelopeSegment {
    from: V3,
    to: V3,
    w_from: f64,
    w_to: f64,
    branch: String,
    source: String,
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
struct Clearance {
    element_id: String,
    branch: String,
    source: String,
    beam_radius_mm: f64,
    beam_diameter_mm: f64,
    aperture_radius_mm: f64,
    clearance_ratio: f64,
    status: &'static str,
}

#[derive(Clone, Copy)]
struct BeamState<'a> {
    q: Q,
    from: &'a str,
    port: &'static str,
}

struct GroupState {
    q: Q,
    surface: usize,
    radius: f64,
}

struct Envelope<'a> {
    segments: Vec<EnvelopeSegment>,
    hits: Vec<Clearance>,
    links: Vec<Connection>,
    source_radii: BTreeMap<&'a str, f64>,
}
#[derive(Serialize, schemars::JsonSchema)]
struct GaussianEnvelope {
    segments: Vec<EnvelopeSegment>,
    hits: Vec<Clearance>,
    units: &'static str,
}

impl Envelope<'_> {
    fn report(&self) -> GaussianEnvelope {
        GaussianEnvelope {
            segments: self.segments.clone(),
            hits: self.hits.clone(),
            units: "mm",
        }
    }
}

fn center_trace(bench: &Bench, system: &System) -> Result<Trace, String> {
    bench.validate()?;
    let mut center = bench.clone();
    for source in &mut center.sources {
        source.rings = 0;
    }
    for element in &mut center.elements {
        element.scatter = None;
        element.ghosts = 0;
    }
    center.trace(system)
}

fn envelope<'a>(
    bench: &'a Bench,
    system: &System,
    trace: &'a Trace,
) -> Result<Envelope<'a>, String> {
    let mut out = Envelope {
        segments: Vec::new(),
        hits: Vec::new(),
        links: Vec::new(),
        source_radii: BTreeMap::new(),
    };
    let elements: HashMap<_, _> = bench.elements.iter().map(|e| (e.id.as_str(), e)).collect();
    let sources: HashMap<_, _> = bench.sources.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut states = HashMap::<(&str, String), BeamState>::new();
    let mut groups = HashMap::<(&str, &str), GroupState>::new();
    let mut matrices = HashMap::<u64, Vec<(Matrix, f64)>>::new();
    let mut links = HashSet::new();
    for source in &bench.sources {
        let wavelength = source.wavelength * 1e-6 * source.m2.unwrap_or(1.);
        let waist = source.waist.unwrap_or(source.beam_radius.max(0.5));
        let n = system.material_index("AIR", source.wavelength)?;
        let q = Q {
            re: -source.waist_offset.unwrap_or(0.),
            im: (PI * n * (waist / wavelength)) * waist,
        };
        out.source_radii
            .insert(source.id.as_str(), q.radius(wavelength, n)?);
        states.insert(
            (source.id.as_str(), format!("{}:0", source.id)),
            BeamState {
                q,
                from: &source.id,
                port: "out",
            },
        );
    }
    for segment in &trace.segments {
        let source = sources[segment.source.as_str()];
        let wavelength = source.wavelength * 1e-6 * source.m2.unwrap_or(1.);
        let n = system.material_index("AIR", source.wavelength)?;
        let key = (segment.source.as_str(), segment.branch.as_str());
        let element = segment.element.as_deref().map(|id| elements[id]);
        let (w_from, w_to) = match segment.status.as_str() {
            "group_surface" | "group_obstruction" => {
                let group = groups
                    .get_mut(&key)
                    .ok_or("Missing Gaussian lens group entry")?;
                let path = &matrices[&source.wavelength.to_bits()];
                let (m, index) = path[group.surface - 1];
                let (end, end_index) = if segment.status == "group_obstruction" {
                    (
                        group
                            .q
                            .transform(m, n, index)
                            .transfer(norm(sub(segment.to, segment.from))),
                        index,
                    )
                } else {
                    let (next, index) = path[group.surface];
                    group.surface += 1;
                    (group.q.transform(next, n, index), index)
                };
                let from = group.radius;
                let to = end.radius(wavelength, end_index)?;
                group.radius = to;
                (from, to)
            }
            _ => {
                let state = states
                    .remove(&(key.0, key.1.to_owned()))
                    .ok_or("Missing Gaussian center-ray branch")?;
                let q = state.q.transfer(norm(sub(segment.to, segment.from)));
                let to = q.radius(wavelength, n)?;
                if let Some(e) = element
                    .filter(|_| matches!(segment.status.as_str(), "hit" | "clipped" | "beam_stop"))
                {
                    let ratio = e.aperture / to;
                    out.hits.push(Clearance {
                        element_id: e.id.clone(),
                        branch: segment.branch.clone(),
                        source: segment.source.clone(),
                        beam_radius_mm: to,
                        beam_diameter_mm: 2. * to,
                        aperture_radius_mm: e.aperture,
                        clearance_ratio: ratio,
                        status: if ratio >= 1.5 {
                            "ok"
                        } else if ratio >= 1. {
                            "tight"
                        } else {
                            "clipped"
                        },
                    });
                    if links.insert((state.from, state.port, e.id.as_str())) {
                        out.links.push(Connection {
                            from: state.from.into(),
                            port: state.port.into(),
                            to: e.id.clone(),
                        });
                    }
                    if segment.status == "hit" {
                        let next = match e.kind {
                            Kind::Lens | Kind::CurvedMirror => {
                                let mut m = Matrix::identity();
                                m.refract(n / e.focal_length);
                                q.transform(m, n, n)
                            }
                            Kind::LensGroup => {
                                let path = match matrices.entry(source.wavelength.to_bits()) {
                                    std::collections::hash_map::Entry::Occupied(entry) => {
                                        entry.into_mut()
                                    }
                                    std::collections::hash_map::Entry::Vacant(entry) => entry
                                        .insert(
                                            (1..=system.surfaces.len())
                                                .map(|i| system.matrix(source.wavelength, 0, i))
                                                .collect::<Result<_, _>>()?,
                                        ),
                                };
                                groups.insert(
                                    key,
                                    GroupState {
                                        q,
                                        surface: 1,
                                        radius: to,
                                    },
                                );
                                let &(m, index) = path.last().unwrap();
                                q.transform(m, n, index)
                            }
                            _ => q,
                        };
                        for &port in bench::ports(e.kind) {
                            states.insert(
                                (key.0, format!("{}/{}:{port}", key.1, e.id)),
                                BeamState {
                                    q: next,
                                    from: &e.id,
                                    port,
                                },
                            );
                        }
                    }
                }
                (state.q.radius(wavelength, n)?, to)
            }
        };
        out.segments.push(EnvelopeSegment {
            from: segment.from,
            to: segment.to,
            w_from,
            w_to,
            branch: key.1.to_owned(),
            source: key.0.to_owned(),
        });
    }
    Ok(out)
}

pub fn gaussian_envelope(bench: &Bench, system: &System) -> Result<Value, String> {
    let trace = center_trace(bench, system)?;
    Ok(json!(envelope(bench, system, &trace)?.report()))
}

struct Grid {
    origin: V3,
    pitch: f64,
    last: [i64; 2],
}
impl Grid {
    fn new(bench: &Bench) -> Result<Self, String> {
        let count = bench
            .table_size
            .map(|width| (width / bench.hole_pitch + 1e-9).floor());
        if count.iter().any(|&n| n > (1u64 << 52) as f64) {
            return Err("Breadboard hole indices exceed numeric precision".into());
        }
        Ok(Self {
            origin: bench.origin,
            pitch: bench.hole_pitch,
            last: count.map(|n| n as i64),
        })
    }

    fn hole(&self, point: V3) -> [i64; 2] {
        std::array::from_fn(|i| {
            (((point[i] - self.origin[i]) / self.pitch).round() as i64).clamp(0, self.last[i])
        })
    }

    fn point(&self, hole: [i64; 2]) -> V3 {
        add(
            self.origin,
            [hole[0] as f64 * self.pitch, hole[1] as f64 * self.pitch, 0.],
        )
    }

    fn offset(&self, hole: [i64; 2], point: V3) -> f64 {
        let delta = sub(self.point(hole), point);
        delta[0].hypot(delta[1])
    }

    fn drawing_holes(&self) -> Result<impl Iterator<Item = [i64; 2]> + '_, String> {
        if (self.last[0] + 1).saturating_mul(self.last[1] + 1) > 25_000 {
            return Err("Blueprint contains more than 25,000 breadboard holes".into());
        }
        Ok((0..=self.last[0]).flat_map(|col| (0..=self.last[1]).map(move |row| [col, row])))
    }
}

fn base_contains(element: &Element, point: V3) -> bool {
    let Some(mount) = &element.mount else {
        return false;
    };
    let [x, y, _] = bench::local(sub(point, element.position), [0., 0., element.rotation[2]]);
    x.abs() <= mount.base_size[0] / 2. && y.abs() <= mount.base_size[1] / 2.
}

fn base_corners(element: &Element, mount: &Mount) -> [V3; 4] {
    let [w, h] = mount.base_size.map(|x| x / 2.);
    [[-w, -h, 0.], [w, -h, 0.], [w, h, 0.], [-w, h, 0.]].map(|p| {
        add(
            element.position,
            bench::rotate(p, [0., 0., element.rotation[2]]),
        )
    })
}

fn clamping_hole(grid: &Grid, element: &Element, others: &[&Element]) -> Option<([i64; 2], f64)> {
    let center = element.position;
    let nearest = grid.hole(center);
    let bound = |i: usize, delta: f64| (center[i] + delta - grid.origin[i]) / grid.pitch;
    let lo = std::array::from_fn(|i| (bound(i, -FORK_REACH).ceil() as i64).clamp(0, grid.last[i]));
    let hi = std::array::from_fn(|i| (bound(i, FORK_REACH).floor() as i64).clamp(0, grid.last[i]));
    let mut pending = vec![(lo, hi)];
    let mut best: Option<([i64; 2], f64)> = None;
    while let Some((lo, hi)) = pending.pop() {
        if lo[0] > hi[0] || lo[1] > hi[1] {
            continue;
        }
        let hole = std::array::from_fn(|i| nearest[i].clamp(lo[i], hi[i]));
        let distance = grid.offset(hole, center);
        if distance > FORK_REACH || best.is_some_and(|(_, d)| distance >= d) {
            continue;
        }
        let point = grid.point(hole);
        let covered = others.iter().any(|element| base_contains(element, point));
        if !covered {
            best = Some((hole, distance));
            continue;
        }
        let corners = [lo, [lo[0], hi[1]], hi, [hi[0], lo[1]]].map(|p| grid.point(p));
        if others
            .iter()
            .any(|element| corners.iter().all(|&p| base_contains(element, p)))
        {
            continue;
        }
        let axis = usize::from(hi[1] - lo[1] > hi[0] - lo[0]);
        let middle = lo[axis] + (hi[axis] - lo[axis]) / 2;
        let mut lower_max = hi;
        lower_max[axis] = middle;
        let mut upper_min = lo;
        upper_min[axis] = middle + 1;
        let halves = [(lo, lower_max), (upper_min, hi)];
        if hole[axis] <= middle {
            pending.extend(halves.into_iter().rev());
        } else {
            pending.extend(halves);
        }
    }
    best
}

#[derive(schemars::JsonSchema, Serialize)]
struct Issue {
    severity: &'static str,
    code: &'static str,
    element_id: Option<String>,
    branch: Option<String>,
    message: String,
}
fn issue(
    severity: &'static str,
    code: &'static str,
    element: Option<&str>,
    branch: Option<&str>,
    message: String,
) -> Issue {
    Issue {
        severity,
        code,
        element_id: element.map(str::to_owned),
        branch: branch.map(str::to_owned),
        message,
    }
}

fn dump_position(bench: &Bench, start: V3, direction: V3) -> Option<V3> {
    // Intersect the forward half-ray with the usable table footprint and the
    // half-space above the tabletop, then back off from the exit boundary.
    let mut enter: f64 = 0.;
    let mut exit = f64::INFINITY;
    let p = sub(start, bench.origin);
    for i in 0..3 {
        let (lo, hi) = if i == 2 {
            (1e-6, f64::INFINITY)
        } else {
            let margin = 20_f64.min(bench.table_size[i] / 4.);
            (margin, bench.table_size[i] - margin)
        };
        if direction[i] == 0. {
            if p[i] < lo || p[i] > hi {
                return None;
            }
        } else {
            let a = (lo - p[i]) / direction[i];
            let b = (hi - p[i]) / direction[i];
            enter = enter.max(a.min(b));
            exit = exit.min(a.max(b));
        }
    }
    if exit <= enter {
        return None;
    }
    let distance = if exit.is_finite() {
        exit - ((exit - enter) / 2.).min(25.)
    } else {
        enter + 25.
    };
    Some(add(start, scale(direction, distance)))
}

pub fn lab_check(bench: &Bench, system: &System) -> Result<Value, String> {
    serde_json::to_value(check(bench, system)?).map_err(|e| e.to_string())
}
pub(crate) fn check(bench: &Bench, system: &System) -> Result<LabCheck, String> {
    let trace = bench.trace(system)?;
    let center = center_trace(bench, system)?;
    let gaussian = envelope(bench, system, &center)?;
    lab_report(bench, &trace, &gaussian)
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct IssueCounts {
    errors: u32,
    warnings: u32,
    info: u32,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct BeamDump {
    branch: String,
    source: String,
    position: [f64; 3],
    direction: [f64; 3],
    power: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Clamping {
    element_id: String,
    hole: [i64; 2],
    offset_mm: f64,
    style: &'static str,
    clamping_hole: Option<[i64; 2]>,
    clamping_offset_mm: Option<f64>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct LabCheck {
    readiness_score: u32,
    passed: bool,
    counts: IssueCounts,
    issues: Vec<Issue>,
    gaussian: GaussianEnvelope,
    recommended_beam_dumps: Vec<BeamDump>,
    clamping: Vec<Clamping>,
}

fn lab_report(bench: &Bench, trace: &Trace, gaussian: &Envelope<'_>) -> Result<LabCheck, String> {
    let grid = Grid::new(bench)?;
    let mut issues = Vec::new();
    let mut dumps = Vec::new();
    let mut optical = HashSet::new();
    let mut branch_segments = HashMap::new();
    for segment in &trace.segments {
        branch_segments.insert((&segment.source, &segment.branch), segment);
    }
    for end in &trace.terminations {
        let last = branch_segments.get(&(&end.source, &end.branch));
        let code = if end.reason == "escaped" && last.is_some_and(|s| s.status == "missed_target") {
            "missed_target"
        } else {
            end.reason.as_str()
        };
        let (severity, diagnostic) = match code {
            "detected" => continue,
            "beam_stop" => ("info", "beam_stop"),
            "escaped" => ("warning", "escaped"),
            "missed_target" => ("error", "missed_target"),
            "clipped" | "group_aperture" => ("error", "clipped"),
            "mount_obstruction" | "group_obstruction" | "unexpected_hit" => {
                ("error", "obstruction")
            }
            "budget" | "ghost_bounce_limit" => ("warning", "truncated"),
            _ => ("error", "blocked"),
        };
        let target = end.element.as_deref();
        // Bundle the disk samples for one physical branch in readiness counts.
        let branch = end.branch.split_once('/').map_or("", |(_, path)| path);
        if optical.insert((&end.source, branch, target, diagnostic)) {
            let at = target.map_or_else(String::new, |id| format!(" at {id}"));
            issues.push(issue(
                severity,
                diagnostic,
                target,
                Some(&end.branch),
                format!("{}: {code}{at}", end.branch),
            ));
        }
        if end.reason == "escaped" {
            if let Some(segment) = last {
                if let Ok(direction) = unit(sub(segment.to, segment.from)) {
                    if let Some(position) = dump_position(bench, segment.from, direction) {
                        dumps.push(BeamDump {
                            branch: end.branch.clone(),
                            source: end.source.clone(),
                            position,
                            direction,
                            power: end.power,
                        });
                    }
                }
            }
        }
    }
    for hit in &gaussian.hits {
        if hit.status != "ok" {
            let severity = if hit.status == "tight" {
                "warning"
            } else {
                "error"
            };
            issues.push(issue(
                severity,
                "gaussian_clearance",
                Some(&hit.element_id),
                Some(&hit.branch),
                format!(
                    "{}: Gaussian aperture clearance is {:.2} beam radii ({})",
                    hit.element_id, hit.clearance_ratio, hit.status
                ),
            ));
        }
    }
    for warning in &trace.warnings {
        let (target, detail) = warning.rsplit_once(": ").unwrap_or(("", warning));
        let (severity, code) = match detail {
            "mount dimensions unspecified" => ("warning", "missing_mount"),
            "mount bases overlap" => ("error", "mount_overlap"),
            "optical body envelope intersects table" => ("error", "body_table_collision"),
            "mount envelope outside table" => ("error", "table_bounds"),
            "optical height outside mount adjustment range" => ("error", "mount_height"),
            _ => ("warning", "assembly"),
        };
        issues.push(issue(
            severity,
            code,
            (!target.is_empty()).then_some(target),
            None,
            warning.into(),
        ));
    }
    for source in &bench.sources {
        let p = sub(source.position, bench.origin);
        if p[2] <= 0. || (0..2).any(|i| !(0. ..=bench.table_size[i]).contains(&p[i])) {
            issues.push(issue(
                "error",
                "table_bounds",
                Some(&source.id),
                None,
                format!("{}: source lies outside the working table", source.id),
            ));
        }
    }
    for element in bench.elements.iter().filter(|e| e.mount.is_none()) {
        let p = sub(element.position, bench.origin);
        if (0..2).any(|i| !(0. ..=bench.table_size[i]).contains(&p[i])) {
            issues.push(issue(
                "error",
                "table_bounds",
                Some(&element.id),
                None,
                format!("{}: optic center lies outside the table", element.id),
            ));
        }
    }
    let mut clamping = Vec::new();
    let mounted: Vec<_> = bench
        .elements
        .iter()
        .filter(|e| e.mount.is_some())
        .collect();
    for &element in &mounted {
        let nearest = grid.hole(element.position);
        let offset = grid.offset(nearest, element.position);
        let others: Vec<_> = mounted
            .iter()
            .copied()
            .filter(|e| e.id != element.id)
            .collect();
        let free = clamping_hole(&grid, element, &others);
        let style = match free {
            Some((_, distance)) if distance <= DIRECT_REACH => "direct_hole",
            Some(_) => "clamping_fork",
            None => {
                issues.push(issue(
                    "error",
                    "unreachable_hole",
                    Some(&element.id),
                    None,
                    format!("{}: no free table hole within {FORK_REACH} mm", element.id),
                ));
                "unreachable"
            }
        };
        clamping.push(Clamping {
            element_id: element.id.clone(),
            hole: nearest,
            offset_mm: offset,
            style,
            clamping_hole: free.map(|(hole, _)| hole),
            clamping_offset_mm: free.map(|(_, d)| d),
        });
    }
    let count = |severity| {
        issues
            .iter()
            .filter(|item| item.severity == severity)
            .count() as u32
    };
    let (errors, warnings, info) = (count("error"), count("warning"), count("info"));
    let readiness_score = 100u32.saturating_sub(20 * errors + 7 * warnings);
    Ok(LabCheck {
        readiness_score,
        passed: errors == 0,
        counts: IssueCounts {
            errors,
            warnings,
            info,
        },
        issues,
        gaussian: gaussian.report(),
        recommended_beam_dumps: dumps,
        clamping,
    })
}

#[derive(schemars::JsonSchema, Serialize)]
struct Step {
    step: usize,
    id: String,
    kind: String,
    label: String,
    position: V3,
    hole: [i64; 2],
    hole_offset: f64,
    mount_part: Option<String>,
    beam_height_z: f64,
    expected_beam_radius_mm: Option<f64>,
    target_next: Option<String>,
    instruction: String,
}
#[derive(schemars::JsonSchema, Serialize)]
struct Sensitivity {
    element_id: String,
    screw_tpi: f64,
    lever_arm_mm: f64,
    mrad_per_quarter_turn: f64,
    targets: Vec<SpotShift>,
}
#[derive(schemars::JsonSchema, Serialize)]
struct SpotShift {
    target_id: String,
    distance_mm: f64,
    spot_shift_mm_per_quarter_turn: f64,
}
#[derive(schemars::JsonSchema, Serialize)]
pub(crate) struct Guide {
    steps: Vec<Step>,
    sensitivities: Vec<Sensitivity>,
    safety_notes: &'static [&'static str],
}

const SAFETY_NOTES: &[&str] = &[
    "Fit beam stops to unused outputs and secure mounts before enabling the source.",
    "Follow the lab's laser safety procedure and use the lowest authorized alignment power.",
    "Verify each path and detector reading before opening the next branch.",
];

fn guide<'a>(bench: &'a Bench, gaussian: &'a Envelope<'_>) -> Result<Guide, String> {
    let grid = Grid::new(bench)?;
    let mut outgoing: HashMap<&str, Vec<&Connection>> = HashMap::new();
    let mut links = HashSet::new();
    for c in bench.connections.iter().chain(&gaussian.links) {
        if links.insert((&c.from, &c.port, &c.to)) {
            outgoing.entry(c.from.as_str()).or_default().push(c);
        }
    }
    let edges = |id: &str| outgoing.get(id).map_or(&[][..], Vec::as_slice);
    let elements: HashMap<_, _> = bench.elements.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut queued: VecDeque<_> = bench.sources.iter().map(|s| s.id.as_str()).collect();
    let mut ids = Vec::new();
    let mut installed = HashSet::new();
    while let Some(id) = queued.pop_front() {
        if installed.insert(id) {
            ids.push(id);
            queued.extend(edges(id).iter().map(|c| c.to.as_str()));
        }
    }
    ids.extend(
        bench
            .elements
            .iter()
            .map(|e| e.id.as_str())
            .filter(|id| !installed.contains(id)),
    );
    let mut radii = HashMap::<&str, f64>::new();
    for hit in &gaussian.hits {
        let max = radii.entry(&hit.element_id).or_default();
        *max = max.max(hit.beam_radius_mm);
    }
    let mut steps = Vec::with_capacity(ids.len());
    let mut sensitivities = Vec::new();
    for id in ids {
        let targets = edges(id);
        let next = targets.first().map(|c| c.to.as_str());
        let target_note = if targets.is_empty() {
            String::new()
        } else {
            let ports: Vec<_> = targets
                .iter()
                .map(|c| format!("{} toward {}", c.port, c.to))
                .collect();
            format!(" Confirm {}.", ports.join(", "))
        };
        let (position, kind, label, mount_part, radius, instruction) = if let Some(e) =
            elements.get(id)
        {
            if reflective(e.kind) {
                let screw_tpi = e.screw_tpi.unwrap_or(100.);
                let lever_arm_mm = e.lever_arm_mm.unwrap_or(25.4);
                let deflection = 2. * (0.25 * 25.4 / screw_tpi) / lever_arm_mm;
                let mut pending: VecDeque<_> = targets
                    .iter()
                    .filter(|c| c.port == "reflected")
                    .map(|c| {
                        (
                            c.to.as_str(),
                            norm(sub(elements[c.to.as_str()].position, e.position)),
                            true,
                        )
                    })
                    .collect();
                let mut seen = HashSet::from([id]);
                let mut shifts = Vec::new();
                while let Some((target, distance, first)) = pending.pop_front() {
                    if !seen.insert(target) {
                        continue;
                    }
                    let item = elements[target];
                    if first || item.kind == Kind::Detector {
                        shifts.push(SpotShift {
                            target_id: target.to_owned(),
                            distance_mm: distance,
                            spot_shift_mm_per_quarter_turn: distance * deflection,
                        });
                    }
                    pending.extend(edges(target).iter().map(|c| {
                        let position = elements[c.to.as_str()].position;
                        (
                            c.to.as_str(),
                            distance + norm(sub(position, item.position)),
                            false,
                        )
                    }));
                }
                sensitivities.push(Sensitivity {
                    element_id: id.to_owned(),
                    screw_tpi,
                    lever_arm_mm,
                    mrad_per_quarter_turn: 1000. * deflection,
                    targets: shifts,
                });
            }
            let instruction = match e.kind {
                Kind::Mirror | Kind::CurvedMirror | Kind::Splitter | Kind::Pbs => {
                    "Center the incident spot, then adjust the mount to register the reflected spot."
                }
                Kind::Waveplate | Kind::Polarizer => {
                    "Center the beam and set the marked axis before checking downstream power."
                }
                Kind::Detector => {
                    "Center the beam on the active detector area and verify the power reading."
                }
                Kind::Blocker => "Secure the beam stop and confirm it captures the full beam.",
                _ => "Center the clear aperture on the incoming beam at the prescribed height.",
            };
            (
                e.position,
                kind_name(e.kind),
                e.label.as_str(),
                e.mount.as_ref().map(|m| m.part.as_str()),
                radii.get(id).copied(),
                instruction,
            )
        } else {
            let source = bench.sources.iter().find(|s| s.id == id).unwrap();
            (
                source.position,
                "source".into(),
                id,
                None,
                gaussian.source_radii.get(id).copied(),
                "Install the source, set the beam height and register the launch axis.",
            )
        };
        let hole = grid.hole(position);
        steps.push(Step {
            step: steps.len() + 1,
            id: id.to_owned(),
            kind,
            label: label.to_owned(),
            position,
            hole,
            hole_offset: grid.offset(hole, position),
            mount_part: mount_part.map(str::to_owned),
            beam_height_z: position[2] - bench.origin[2],
            expected_beam_radius_mm: radius,
            target_next: next.map(str::to_owned),
            instruction: format!("{instruction}{target_note}"),
        });
    }
    Ok(Guide {
        steps,
        sensitivities,
        safety_notes: SAFETY_NOTES,
    })
}

pub fn alignment_guide(bench: &Bench, system: &System) -> Result<Value, String> {
    serde_json::to_value(alignment(bench, system)?).map_err(|e| e.to_string())
}
pub(crate) fn alignment(bench: &Bench, system: &System) -> Result<Guide, String> {
    let trace = center_trace(bench, system)?;
    let gaussian = envelope(bench, system, &trace)?;
    guide(bench, &gaussian)
}

pub(crate) const FORMATS: &[&str] = &["svg", "dxf", "stl", "alignment_md"];

fn xml(text: &str) -> String {
    let mut safe = String::new();
    for c in text.chars() {
        match c {
            '&' => safe.push_str("&amp;"),
            '<' => safe.push_str("&lt;"),
            '>' => safe.push_str("&gt;"),
            '"' => safe.push_str("&quot;"),
            '\'' => safe.push_str("&apos;"),
            c if matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}') => {
                safe.push(c)
            }
            _ => {}
        }
    }
    safe
}

fn svg(bench: &Bench, grid: &Grid, gaussian: &Envelope<'_>) -> Result<String, String> {
    let [w, h] = bench.table_size;
    let page_width = w + 64.;
    let page_height = h + 108.;
    let pt = |[x, y, _]: V3| [x - grid.origin[0], h - (y - grid.origin[1])];
    let points = |p: [V3; 4]| {
        p.map(|p| {
            let [x, y] = pt(p);
            format!("{x:.4},{y:.4}")
        })
        .join(" ")
    };
    let mut out = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{page_width}mm" height="{page_height}mm" viewBox="-32 -32 {page_width} {page_height}">
<title>{}</title><defs><clipPath id="table"><rect width="{w}" height="{h}"/></clipPath></defs>
<rect x="-32" y="-32" width="{page_width}" height="{page_height}" fill="#f5f8fb"/>
<rect width="{w}" height="{h}" fill="#fff" stroke="#243b53"/>
<g id="holes" fill="#486581" font-family="sans-serif" font-size="3">
"##,
        xml(&bench.name)
    );
    let hole_radius = 1.5_f64.min(grid.pitch / 5.);
    for hole @ [col, row] in grid.drawing_holes()? {
        let [x, y] = pt(grid.point(hole));
        writeln!(
            out,
            r##"<circle cx="{x:.4}" cy="{y:.4}" r="{hole_radius}"/>"##
        )
        .unwrap();
        if row == 0 {
            writeln!(
                out,
                r##"<text x="{x:.4}" y="{}" text-anchor="middle">C{col}</text>"##,
                h + 9.
            )
            .unwrap();
        }
        if col == 0 {
            writeln!(
                out,
                r##"<text x="-6" y="{y:.4}" text-anchor="end">R{row}</text>"##
            )
            .unwrap();
        }
    }
    out.push_str(
        r##"</g><g id="mounts" fill="#d9e2ec" stroke="#829ab1" stroke-width="0.45">
"##,
    );
    for e in &bench.elements {
        if let Some(mount) = &e.mount {
            writeln!(
                out,
                r##"<polygon points="{}"/>"##,
                points(base_corners(e, mount))
            )
            .unwrap();
        }
    }
    out.push_str(r##"</g><g clip-path="url(#table)"><g id="gaussian-envelope" fill="#007c91" fill-opacity=".22">
"##);
    for segment in &gaussian.segments {
        let d = sub(segment.to, segment.from);
        if let Ok(side) = unit([-d[1], d[0], 0.]) {
            let a = scale(side, segment.w_from);
            let b = scale(side, segment.w_to);
            writeln!(
                out,
                r##"<polygon points="{}"/>"##,
                points([
                    add(segment.from, a),
                    add(segment.to, b),
                    sub(segment.to, b),
                    sub(segment.from, a)
                ])
            )
            .unwrap();
        }
    }
    out.push_str(
        r##"</g><g id="beams" stroke="#007c91" stroke-width=".55">
"##,
    );
    for segment in &gaussian.segments {
        let [x1, y1] = pt(segment.from);
        let [x2, y2] = pt(segment.to);
        writeln!(
            out,
            r##"<line x1="{x1:.4}" y1="{y1:.4}" x2="{x2:.4}" y2="{y2:.4}"/>"##
        )
        .unwrap();
    }
    out.push_str(
        r##"</g></g><g id="optics" fill="#102a43" font-family="sans-serif" font-size="4">
"##,
    );
    for (id, position, radius) in bench
        .elements
        .iter()
        .map(|e| (e.id.as_str(), e.position, e.aperture))
        .chain(
            bench
                .sources
                .iter()
                .map(|s| (s.id.as_str(), s.position, 3.)),
        )
    {
        let [x, y] = pt(position);
        let [px, py, pz] = sub(position, grid.origin);
        writeln!(out, r##"<circle cx="{x:.4}" cy="{y:.4}" r="{radius:.4}" fill="none" stroke="#102a43" stroke-width=".65"/>
<text x="{:.4}" y="{:.4}">{} ({px:.1}, {py:.1}, {pz:.1})</text>"##,
            x + 4., y - radius - 2., xml(id)).unwrap();
    }
    let title_y = h + 20.;
    writeln!(
        out,
        r##"</g><g id="title-block" font-family="sans-serif" fill="#102a43">
<path d="M 0 {title_y} H {w}" stroke="#829ab1"/>
<text x="0" y="{}" font-size="7">{}</text>
<text x="0" y="{}" font-size="4">Hissabeth · {w} × {h} mm · {} mm hole pitch · 1 unit = 1 mm</text>
</g></svg>"##,
        title_y + 12.,
        xml(&bench.name),
        title_y + 22.,
        grid.pitch
    )
    .unwrap();
    Ok(out)
}

fn dxf(bench: &Bench, grid: &Grid, gaussian: &Envelope<'_>) -> Result<String, String> {
    const LAYERS: &[(&str, i32)] = &[
        ("0", 7),
        ("TABLE", 7),
        ("HOLES", 8),
        ("MOUNTS", 2),
        ("OPTICS", 5),
        ("BEAMS", 3),
        ("TEXT", 7),
    ];
    let mut out = String::new();
    let mut pair = |group: i32, value: &dyn std::fmt::Display| {
        writeln!(out, "{group}\n{value}").unwrap();
    };
    pair(0, &"SECTION");
    pair(2, &"HEADER");
    pair(9, &"$ACADVER");
    pair(1, &"AC1009");
    pair(9, &"$LUNITS");
    pair(70, &2);
    pair(0, &"ENDSEC");
    pair(0, &"SECTION");
    pair(2, &"TABLES");
    pair(0, &"TABLE");
    pair(2, &"LTYPE");
    pair(70, &1);
    pair(0, &"LTYPE");
    pair(2, &"CONTINUOUS");
    pair(70, &0);
    pair(3, &"Solid line");
    pair(72, &65);
    pair(73, &0);
    pair(40, &0.);
    pair(0, &"ENDTAB");
    pair(0, &"TABLE");
    pair(2, &"LAYER");
    pair(70, &LAYERS.len());
    for (name, color) in LAYERS {
        pair(0, &"LAYER");
        pair(2, name);
        pair(70, &0);
        pair(62, color);
        pair(6, &"CONTINUOUS");
    }
    pair(0, &"ENDTAB");
    pair(0, &"ENDSEC");
    pair(0, &"SECTION");
    pair(2, &"ENTITIES");
    let mut entity = |kind: &str,
                      layer: &str,
                      a: V3,
                      b: Option<V3>,
                      radius: Option<f64>,
                      label: Option<&str>| {
        pair(0, &kind);
        pair(8, &layer);
        let a = sub(a, grid.origin);
        pair(10, &a[0]);
        pair(20, &a[1]);
        pair(30, &0.);
        if let Some(b) = b {
            let b = sub(b, grid.origin);
            pair(11, &b[0]);
            pair(21, &b[1]);
            pair(31, &0.);
        }
        if let Some(radius) = radius {
            pair(40, &radius);
        }
        if let Some(label) = label {
            let ascii: String = label
                .chars()
                .map(|c| {
                    if c.is_ascii_graphic() || c == ' ' {
                        c
                    } else {
                        '?'
                    }
                })
                .collect();
            pair(1, &ascii);
        }
    };
    let [w, h] = bench.table_size;
    let board = [[0., 0., 0.], [w, 0., 0.], [w, h, 0.], [0., h, 0.]].map(|p| add(p, grid.origin));
    for i in 0..4 {
        entity(
            "LINE",
            "TABLE",
            board[i],
            Some(board[(i + 1) % 4]),
            None,
            None,
        );
    }
    for hole in grid.drawing_holes()? {
        entity(
            "CIRCLE",
            "HOLES",
            grid.point(hole),
            None,
            Some(1.5_f64.min(grid.pitch / 5.)),
            None,
        );
    }
    for e in &bench.elements {
        if let Some(mount) = &e.mount {
            let base = base_corners(e, mount);
            for i in 0..4 {
                entity(
                    "LINE",
                    "MOUNTS",
                    base[i],
                    Some(base[(i + 1) % 4]),
                    None,
                    None,
                );
            }
        }
        entity("CIRCLE", "OPTICS", e.position, None, Some(e.aperture), None);
        let position = add(e.position, [4., e.aperture + 2., 0.]);
        entity("TEXT", "TEXT", position, None, Some(3.), Some(&e.id));
    }
    for s in &bench.sources {
        entity("CIRCLE", "OPTICS", s.position, None, Some(3.), None);
        entity(
            "TEXT",
            "TEXT",
            add(s.position, [4., 5., 0.]),
            None,
            Some(3.),
            Some(&s.id),
        );
    }
    for segment in &gaussian.segments {
        if (segment.from[0], segment.from[1]) != (segment.to[0], segment.to[1]) {
            entity("LINE", "BEAMS", segment.from, Some(segment.to), None, None);
        }
    }
    pair(0, &"ENDSEC");
    pair(0, &"EOF");
    Ok(out)
}

fn facet(out: &mut String, a: V3, b: V3, c: V3) -> Result<(), String> {
    let [nx, ny, nz] = unit(bench::cross(sub(b, a), sub(c, a)))?;
    writeln!(out, "  facet normal {nx} {ny} {nz}\n    outer loop").unwrap();
    for [x, y, z] in [a, b, c] {
        writeln!(out, "      vertex {x} {y} {z}").unwrap();
    }
    out.push_str("    endloop\n  endfacet\n");
    Ok(())
}

fn prism(out: &mut String, low: &[V3], high: &[V3]) -> Result<(), String> {
    for i in 0..low.len() {
        let j = (i + 1) % low.len();
        facet(out, low[i], low[j], high[j])?;
        facet(out, low[i], high[j], high[i])?;
    }
    for i in 1..low.len() - 1 {
        facet(out, low[0], low[i + 1], low[i])?;
        facet(out, high[0], high[i], high[i + 1])?;
    }
    Ok(())
}

fn box_solid(out: &mut String, xy: [V3; 4], bottom: f64, top: f64) -> Result<(), String> {
    prism(
        out,
        &xy.map(|[x, y, _]| [x, y, bottom]),
        &xy.map(|[x, y, _]| [x, y, top]),
    )
}

fn stl(bench: &Bench) -> Result<String, String> {
    let [w, h] = bench.table_size;
    let mut out = String::from("solid hissabeth_bench\n");
    box_solid(
        &mut out,
        [[0., 0., 0.], [w, 0., 0.], [w, h, 0.], [0., h, 0.]],
        -12.7,
        0.,
    )?;
    for e in &bench.elements {
        let center = sub(e.position, bench.origin);
        if let Some(mount) = &e.mount {
            let base = base_corners(e, mount).map(|p| sub(p, bench.origin));
            let post_top = (center[2] - e.body_radius).max(6.);
            box_solid(&mut out, base, 0., 6.)?;
            if post_top > 6. {
                let [x, y, _] = center;
                let post_radius = 6.35_f64.min(mount.base_size[0].min(mount.base_size[1]) / 2.);
                let post = std::array::from_fn::<_, 24, _>(|i| {
                    let (s, c) = (TAU * i as f64 / 24.).sin_cos();
                    [x + post_radius * c, y + post_radius * s, 6.]
                });
                prism(&mut out, &post, &post.map(|[x, y, _]| [x, y, post_top]))?;
            }
        }
        let rim = std::array::from_fn::<_, 32, _>(|i| {
            let (s, c) = (TAU * i as f64 / 32.).sin_cos();
            let p = [-2., e.body_radius * c, e.body_radius * s];
            add(center, bench::rotate(p, e.rotation))
        });
        let slab = bench::rotate([4., 0., 0.], e.rotation);
        prism(&mut out, &rim, &rim.map(|p| add(p, slab)))?;
    }
    out.push_str("endsolid hissabeth_bench\n");
    Ok(out)
}

fn markdown_cell(text: &str) -> String {
    xml(text)
        .replace('\\', "&#92;")
        .replace('|', "&#124;")
        .replace('\n', " ")
        .replace('\r', " ")
}

fn alignment_md(bench: &Bench, system: &System, gaussian: &Envelope<'_>) -> Result<String, String> {
    let guide = guide(bench, gaussian)?;
    let check = lab_report(bench, &bench.trace(system)?, gaussian)?;
    let mut out = format!("# {} — alignment SOP\n\n", markdown_cell(&bench.name));
    for note in guide.safety_notes {
        writeln!(out, "- {note}").unwrap();
    }
    out.push_str("\n## Bill of materials\n\n| Component | Mount | Qty |\n| --- | --- | ---: |\n");
    let mut bom = BTreeMap::new();
    for e in &bench.elements {
        *bom.entry((
            kind_name(e.kind),
            e.mount.as_ref().map_or("Unspecified", |m| m.part.as_str()),
        ))
        .or_insert(0) += 1;
    }
    bom.insert(("Laser source".into(), "Unspecified"), bench.sources.len());
    for ((kind, mount), count) in bom {
        writeln!(
            out,
            "| {} | {} | {count} |",
            markdown_cell(&kind),
            markdown_cell(mount)
        )
        .unwrap();
    }
    out.push_str("\n## Hole coordinates\n\n| Component | Column | Row | Offset (mm) | Beam height (mm) |\n| --- | ---: | ---: | ---: | ---: |\n");
    for step in &guide.steps {
        let [col, row] = step.hole;
        writeln!(
            out,
            "| {} | {col} | {row} | {:.2} | {:.2} |",
            markdown_cell(&step.id),
            step.hole_offset,
            step.beam_height_z
        )
        .unwrap();
    }
    out.push_str("\n## Alignment checklist\n\n");
    for step in &guide.steps {
        writeln!(
            out,
            "{}. [ ] **{}**. {}",
            step.step,
            markdown_cell(&step.label),
            markdown_cell(&step.instruction)
        )
        .unwrap();
    }
    out.push_str("\n## Kinematic screw sensitivity\n\n| Mount | Target | TPI | mrad / quarter turn | mm / quarter turn |\n| --- | --- | ---: | ---: | ---: |\n");
    for sensitivity in &guide.sensitivities {
        let id = markdown_cell(&sensitivity.element_id);
        let tpi = sensitivity.screw_tpi;
        let angle = sensitivity.mrad_per_quarter_turn;
        if sensitivity.targets.is_empty() {
            writeln!(out, "| {id} | — | {tpi:.1} | {angle:.3} | — |").unwrap();
        }
        for target in &sensitivity.targets {
            writeln!(
                out,
                "| {id} | {} | {tpi:.1} | {angle:.3} | {:.3} |",
                markdown_cell(&target.target_id),
                target.spot_shift_mm_per_quarter_turn
            )
            .unwrap();
        }
    }
    out.push_str("\n## Lab clearance warnings\n\n");
    let mut findings = check
        .issues
        .iter()
        .filter(|issue| issue.severity != "info")
        .peekable();
    if findings.peek().is_none() {
        out.push_str("No clearance warnings or errors were reported.\n");
    }
    for item in findings {
        writeln!(
            out,
            "- **{}** — {}",
            item.severity,
            markdown_cell(&item.message)
        )
        .unwrap();
    }
    let dumps = &check.recommended_beam_dumps;
    if !dumps.is_empty() {
        out.push_str("\n### Recommended beam stops\n\n");
        for dump in dumps {
            let p = dump.position;
            writeln!(
                out,
                "- {} at ({:.1}, {:.1}, {:.1}) mm in world coordinates.",
                markdown_cell(&dump.branch),
                p[0],
                p[1],
                p[2]
            )
            .unwrap();
        }
    }
    Ok(out)
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct Blueprint {
    format: String,
    filename: String,
    mime: &'static str,
    content: String,
}

pub fn export_blueprint(bench: &Bench, system: &System, format: &str) -> Result<Value, String> {
    serde_json::to_value(export_blueprint_report(bench, system, format)?).map_err(|e| e.to_string())
}
pub(crate) fn export_blueprint_report(
    bench: &Bench,
    system: &System,
    format: &str,
) -> Result<Blueprint, String> {
    if !FORMATS.contains(&format) {
        return Err(format!(
            "Unknown blueprint format {format:?}. Choose {}",
            FORMATS.join(", ")
        ));
    }
    let (extension, mime, content) = if format == "stl" {
        bench.validate()?;
        ("stl", "model/stl", stl(bench)?)
    } else {
        let trace = center_trace(bench, system)?;
        let gaussian = envelope(bench, system, &trace)?;
        match format {
            "svg" => (
                "svg",
                "image/svg+xml",
                svg(bench, &Grid::new(bench)?, &gaussian)?,
            ),
            "dxf" => (
                "dxf",
                "image/vnd.dxf",
                dxf(bench, &Grid::new(bench)?, &gaussian)?,
            ),
            _ => (
                "md",
                "text/markdown",
                alignment_md(bench, system, &gaussian)?,
            ),
        }
    };
    Ok(Blueprint {
        format: format.to_owned(),
        filename: format!("hissabeth-bench.{extension}"),
        mime,
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(got: f64, expected: f64) {
        assert!((got - expected).abs() < 1e-8, "{got} vs {expected}");
    }

    #[test]
    fn catalog_filters_parts_and_exposes_lab_metadata() {
        let all = catalog(None, None);
        assert!(all["items"].as_array().unwrap().len() >= 14);
        let result = catalog(Some("la1131"), Some("lens"));
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["mount"]["part"], "LMR1/M");
        assert_eq!(items[0]["focal_length"], 50.);
        assert!(
            catalog(Some("la1131"), Some("pbs"))["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn lab_templates_catch_every_ray_and_balance_the_ledger() {
        let system = System::default();
        for &name in TEMPLATES {
            let bench = template(name).unwrap();
            assert!(bench.elements.iter().all(|e| e.mount.is_some()));
            let trace = bench.trace(&system).unwrap();
            assert!(trace.power.detected > 0.4, "{name}: {:?}", trace.power);
            close(trace.balance_error, 0.);
            assert!(
                trace
                    .terminations
                    .iter()
                    .all(|t| matches!(t.reason.as_str(), "detected" | "beam_stop")),
                "{name}: {:?}",
                trace.terminations
            );
            let lab = lab_check(&bench, &system).unwrap();
            assert_eq!(lab["passed"], true, "{name}: {}", lab["issues"]);
        }
        assert!(template("other").unwrap_err().contains("mach_zehnder"));
    }

    #[test]
    fn auto_aim_snaps_relative_to_origin_and_aligns_fold() {
        let mut bench = template("periscope_routing").unwrap();
        bench.origin = [10., -15., 5.];
        bench.sources[0].position = [110., 185., 75.];
        bench.elements[0].position = [259., 183., 75.];
        bench.elements[1].position = [260., 284., 125.];
        bench.elements[2].position = [508., 285., 125.];
        bench.elements[0].rotation = [15., 30., 90.];
        let result = auto_aim(&bench, true, Some(85.)).unwrap();
        let adjusted: Bench = serde_json::from_value(result["bench"].clone()).unwrap();
        assert_eq!(adjusted.elements[0].position, [260., 185., 85.]);
        assert_eq!(adjusted.sources[0].position[2], 85.);
        assert_eq!(bench.elements[0].rotation, [15., 30., 90.]);
        assert_eq!(result["adjusted_elements"], json!(["M1", "M2", "D"]));
        let normal = bench::rotate([1., 0., 0.], adjusted.elements[0].rotation);
        let incoming = adjusted.sources[0].direction;
        let reflected = sub(incoming, scale(normal, 2. * dot(incoming, normal)));
        close(norm(sub(reflected, [0., 1., 0.])), 0.);
        assert!(dot(normal, incoming) < 0.);
        let trace = adjusted.trace(&System::default()).unwrap();
        close(trace.power.detected, 0.99 * 0.99);
    }

    #[test]
    fn q_envelope_distinguishes_three_clearance_bands() {
        let mut bench = template("polarized_beam_combiner").unwrap();
        let source = &mut bench.sources[0];
        source.waist = Some(1.);
        source.waist_offset = Some(20.);
        source.m2 = Some(1.5);
        source.rings = 2;
        bench.elements[0].aperture = 1.2;
        bench.elements[1].aperture = 0.8;
        let system = System::default();
        let envelope = gaussian_envelope(&bench, &system).unwrap();
        let hits = envelope["hits"].as_array().unwrap();
        assert_eq!(
            hits.iter()
                .map(|x| x["status"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["tight", "clipped", "ok", "ok"]
        );
        assert_eq!(envelope["segments"].as_array().unwrap().len(), 4);
        let n = system.material_index("AIR", 532.).unwrap();
        let z_r = PI * n / (532e-6 * 1.5);
        let expected = (1. + (55. / z_r).powi(2)).sqrt();
        close(hits[0]["beam_radius_mm"].as_f64().unwrap(), expected);
        let check = lab_check(&bench, &system).unwrap();
        assert!(
            check["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["code"] == "gaussian_clearance" && x["severity"] == "error")
        );
    }

    #[test]
    fn q_envelope_uses_prescription_matrix_inside_a_lens_group() {
        let mut bench = template("beam_expander_focus").unwrap();
        let mut group = bench.elements[0].clone();
        group.kind = Kind::LensGroup;
        let detector = mount_element("D", DETECTOR, [500., 200., 75.], [1., 0., 0.]);
        bench.elements = vec![group, detector];
        bench.connections.truncate(2);
        bench.connections[1].to = "D".into();
        let mut system = System::default();
        let last = system.surfaces.len() - 1;
        let internal: f64 = system.surfaces[..last].iter().map(|s| s.thickness).sum();
        system.surfaces[last].thickness = 300. - internal;
        let result = gaussian_envelope(&bench, &system).unwrap();
        let equivalent = super::super::gaussian::gaussian_beam(&system, 532., 0.7, -100.).unwrap();
        close(
            result["hits"][1]["beam_radius_mm"].as_f64().unwrap(),
            equivalent["image"]["radius"].as_f64().unwrap(),
        );
        assert_eq!(
            result["segments"].as_array().unwrap().len(),
            system.surfaces.len() + 1
        );
    }

    #[test]
    fn lab_report_finds_overlap_fork_holes_and_escaped_beam_dump() {
        let mut bench = template("polarized_beam_combiner").unwrap();
        bench.connections.retain(|c| c.to != "DR");
        bench.elements.retain(|e| e.id != "DR");
        bench.elements[0].position[0] += 8.;
        let overlapping = mount_element("C", LENS, [550., 200., 110.], [1., 0., 0.]);
        bench.elements.push(overlapping);
        let system = System::default();
        let result = lab_check(&bench, &system).unwrap();
        let find = |code, severity| {
            result["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["code"] == code && x["severity"] == severity)
        };
        assert!(find("mount_overlap", "error"));
        assert!(find("unreachable_hole", "error"));
        assert!(find("escaped", "warning"));
        assert_eq!(result["passed"], false);
        assert_eq!(result["clamping"][0]["style"], "clamping_fork");
        assert_eq!(result["clamping"][0]["clamping_hole"], json!([7, 8]));
        assert_eq!(result["clamping"][1]["style"], "direct_hole");
        assert_eq!(result["clamping"][2]["style"], "unreachable");
        let dump = &result["recommended_beam_dumps"][0];
        let p: V3 = serde_json::from_value(dump["position"].clone()).unwrap();
        close(p[0], 300.);
        close(p[2], 75.);
        assert!(p[1] > 200. && p[1] < bench.table_size[1]);
    }

    #[test]
    fn alignment_orders_installation_and_calculates_quarter_turn_shifts() {
        let mut bench = template("periscope_routing").unwrap();
        bench.elements[0].screw_tpi = Some(100.);
        let guide = alignment_guide(&bench, &System::default()).unwrap();
        let steps = guide["steps"].as_array().unwrap();
        assert_eq!(
            steps
                .iter()
                .map(|s| s["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["S1", "M1", "M2", "D"]
        );
        assert_eq!(steps[1]["target_next"], "M2");
        assert_eq!(steps[2]["hole"], json!([10, 12]));
        assert_eq!(steps[2]["beam_height_z"], 125.);
        let sensitivity = &guide["sensitivities"][0];
        close(sensitivity["mrad_per_quarter_turn"].as_f64().unwrap(), 5.);
        let targets = sensitivity["targets"].as_array().unwrap();
        let distance = 100_f64.hypot(50.);
        close(
            targets[0]["spot_shift_mm_per_quarter_turn"]
                .as_f64()
                .unwrap(),
            distance * 0.005,
        );
        assert_eq!(targets[1]["target_id"], "D");
        close(
            targets[1]["spot_shift_mm_per_quarter_turn"]
                .as_f64()
                .unwrap(),
            (distance + 250.) * 0.005,
        );
    }

    #[test]
    fn blueprint_exports_draw_geometry_and_alignment_content() {
        let mut bench = template("periscope_routing").unwrap();
        bench.name = "Alignment & beam <survey>".into();
        let system = System::default();
        let export = |format| export_blueprint(&bench, &system, format).unwrap();
        let svg = export("svg");
        assert_eq!(svg["mime"], "image/svg+xml");
        let image = svg["content"].as_str().unwrap();
        assert!(image.starts_with("<svg xmlns=") && image.trim_end().ends_with("</svg>"));
        for token in [
            "C0</text>",
            "R0</text>",
            "gaussian-envelope",
            "mounts",
            "Alignment &amp; beam &lt;survey&gt;",
        ] {
            assert!(image.contains(token), "SVG lacks {token}");
        }
        let dxf = export("dxf");
        let drawing = dxf["content"].as_str().unwrap();
        let rows: Vec<_> = drawing.lines().collect();
        assert_eq!(rows.len() % 2, 0);
        assert!(
            rows.chunks_exact(2)
                .all(|pair| pair[0].parse::<i32>().is_ok())
        );
        assert!(drawing.contains("1\nAC1009\n") && drawing.ends_with("0\nEOF\n"));
        for layer in ["TABLE", "HOLES", "MOUNTS", "OPTICS", "BEAMS", "TEXT"] {
            assert!(drawing.contains(&format!("8\n{layer}\n")));
        }
        let stl = export("stl");
        let solid = stl["content"].as_str().unwrap();
        assert!(
            solid.starts_with("solid hissabeth_bench\n")
                && solid.ends_with("endsolid hissabeth_bench\n")
        );
        let facets = solid.matches("facet normal ").count();
        assert_eq!(solid.matches("vertex ").count(), 3 * facets);
        assert_eq!(facets, 12 + 3 * (12 + 92 + 124));
        let md = export("alignment_md");
        assert_eq!(md["filename"], "hissabeth-bench.md");
        let sop = md["content"].as_str().unwrap();
        for section in [
            "Bill of materials",
            "Hole coordinates",
            "Alignment checklist",
            "Kinematic screw sensitivity",
            "Lab clearance warnings",
        ] {
            assert!(sop.contains(&format!("## {section}")));
        }
        assert!(sop.contains("1. [ ] **S1**."));
    }
}
