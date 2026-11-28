//! Lab planning for the geometric bench. All lengths are in millimetres.
use super::{
    System,
    bench::{
        self, Assertion, AssertionKind, AssertionUnit, BalancedPair, Bench, Branch, BranchPath,
        BranchReport, Connection, DispersionQuantity, Element, FieldProfile, FieldSampling,
        GlassPass, Kind, LensForm, ModeBasis, ModeFamily, ModeNormalization, Mount, PathBreak,
        PhasePlate, Polarization, Port, Quadrature, Readout, ReadoutPort, Region, RegionShape,
        RelayCheckKind, Stage, StagePose, StagePzt, Trace, next_beam,
    },
    blueprint_render,
    interval::{Dual, Iv, Real},
    math::{Matrix, V3, add, dot, norm, scale, sub, unit},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    f64::consts::{PI, TAU},
    fmt::Write,
};

#[path = "bench_alignment.rs"]
mod alignment_solver;
#[path = "bench_drawing.rs"]
pub(crate) mod drawing;
#[path = "bench_field.rs"]
pub(crate) mod field;
#[path = "bench_layout.rs"]
mod layout;
#[path = "bench_layout3d.rs"]
pub(crate) mod layout3d;
#[path = "bench_relay.rs"]
mod relay;
use alignment_solver::{AimResidual, Combiner, Separation, measure};

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
    /// Drawing cross-section of a lens.
    lens_form: Option<LensForm>,
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
    lens_form: Some(LensForm::Plano),
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
const MASK: Hardware = Hardware {
    part: "custom",
    name: "Three-hole aperture mask, 6 mm holes on 10 mm pitch",
    category: "aperture",
    kind: Kind::Aperture,
    aperture: 15.,
    body_radius: 25.4,
    focal_length: 0.,
    reflectance: 0.,
    transmittance: 1.,
    retardance_waves: None,
    fast_axis_deg: None,
    extinction_ratio: None,
    screw_tpi: None,
    lever_arm_mm: None,
    clamping_style: "retaining_ring",
    mount: "LMR2/M",
    base_size: [25., 25.],
    typical_price_usd: 40,
    notes: "Custom laser-cut mask; hole ports are named L, C and R.",
    lens_form: None,
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
    /// Lens cross-section to copy onto a placed lens.
    #[serde(skip_serializing_if = "Option::is_none")]
    lens_form: Option<LensForm>,
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
                    item.part,
                    item.name,
                    item.category,
                    item.mount,
                    item.notes,
                    &name,
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
                lens_form: item.lens_form.filter(|_| item.kind == Kind::Lens),
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
    "delay_line",
    "michelson",
    "polarized_beam_combiner",
    "beam_expander_focus",
    "periscope_routing",
    "three_arm",
    "modal_hybrid",
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
        rotation: Some(normal_rotation(normal)),
        aim_rotation: false,
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
        regions: vec![],
        substrate: None,
        lens_form: hardware.lens_form.filter(|_| hardware.kind == Kind::Lens),
        phase: None,
        layer: None,
        action: None,
        readout: None,
        role: None,
        metadata: Default::default(),
    }
}

/// Modal hybrid ports: id, LO phase on the port's combiner and the camera
/// offset (mm) its steering mirror aims at.
const HYBRID_PORTS: [(&str, f64, f64); 4] = [
    ("I+", 0., -37.5),
    ("I-", PI, -12.5),
    ("Q+", PI / 2., 12.5),
    ("Q-", 1.5 * PI, 37.5),
];

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
    // Connections of templates that build their element IDs at run time.
    let mut built: Vec<Connection> = Vec::new();
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
        "delay_line" => {
            place("BS1", SPLITTER, at(250., 200.), FOLD);
            place("M3", MIRROR, at(550., 200.), FOLD);
            place("M4", MIRROR, at(550., 325.), [1., 1., 0.]);
            place("BS2", SPLITTER, at(250., 325.), [1., 1., 0.]);
            place("D1", DETECTOR, at(100., 325.), X);
            place("D2", DETECTOR, at(250., 500.), Y);
            (
                "Two-arm retroreflecting delay line",
                &[
                    ("S1", "out", "BS1"),
                    ("BS1", "reflected", "BS2"),
                    ("BS1", "transmitted", "M3"),
                    ("M3", "reflected", "M4"),
                    ("M4", "reflected", "BS2"),
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
        "three_arm" => {
            // A three-hole mask splits one wide beam into parallel L / C / R arms
            // (L is on the +Y side, left when looking along the beam).
            scene.sources[0].beam_radius = 14.;
            scene.sources[0].waist = Some(14.);
            scene.sources[0].rings = 8;
            let mut mask = mount_element("AP", MASK, at(200., 200.), X);
            mask.regions = [("L", 10.), ("C", 0.), ("R", -10.)]
                .map(|(port, y)| Region {
                    port: port.into(),
                    shape: RegionShape::Circle {
                        center: [y, 0.],
                        radius: 3.,
                    },
                    transmittance: 1.,
                    normal: None,
                })
                .into();
            scene.elements.push(mask);
            // Staggered along the beam so the 25 mm bases clear each other
            // while the neighbouring arms pass 10 mm beside each small head.
            for (id, x, y) in [
                ("D_L", 400., 210.),
                ("D_C", 450., 200.),
                ("D_R", 500., 190.),
            ] {
                let mut detector = mount_element(id, DETECTOR, at(x, y), X);
                detector.aperture = 4.;
                detector.body_radius = 4.5;
                scene.elements.push(detector);
            }
            (
                "Three-arm spatial split",
                &[
                    ("S1", "out", "AP"),
                    ("AP", "L", "D_L"),
                    ("AP", "C", "D_C"),
                    ("AP", "R", "D_R"),
                ],
            )
        }
        "modal_hybrid" => {
            // BS0 splits signal (transmitted, +x) from local oscillator (reflected,
            // +y). Tap chains along the row and column feed four combiners on the
            // diagonal, so each combiner sees equal signal and LO path lengths and
            // reflection counts. A combiner's transmitted LO carries the port phase
            // (0, π, π/2, 3π/2); steering mirrors fold the four +x outputs straight
            // down (−y) onto separate ROIs of one camera at normal incidence, so
            // I+ − I− ∝ cos φ and Q+ − Q− ∝ sin φ for a signal phase φ on BS0.
            scene.sources[0].position = [50., 150., 75.];
            scene.sources[0].polarization = None;
            scene.sources[0].field = Some(FieldSampling {
                grid: [128, 128],
                pitch_um: 50.,
                profile: FieldProfile::Gaussian,
                waist_um: 700.,
                polarization: Some([[1., 0.], [0., 0.]]),
            });
            let wavelength = scene.sources[0].wavelength;
            let mut element = |id: String, part, p, normal, reflectance: f64, phase: f64| {
                let mut e = mount_element(&id, part, p, normal);
                // Ideal splitting ratios keep the balanced pairs free of a DC offset.
                e.reflectance = reflectance;
                e.transmittance = 1. - reflectance;
                e.phase = (phase != 0.).then_some(PhasePlate {
                    radians: Some(phase),
                    waves: None,
                    at_wavelength_nm: wavelength,
                });
                scene.elements.push(e);
            };
            element("BS0".into(), SPLITTER, at(150., 150.), FOLD, 0.5, 0.);
            let camera = at(575., 50.);
            let mut dumps = Vec::new();
            for (k, (port, phase, offset)) in HYBRID_PORTS.into_iter().enumerate() {
                let x = 225. + 75. * k as f64;
                let (part, tap) = if k == 3 {
                    (MIRROR, 1.)
                } else {
                    (SPLITTER, 1. / (4. - k as f64))
                };
                element(format!("TS{}", k + 1), part, at(x, 150.), FOLD, tap, 0.);
                element(format!("TL{}", k + 1), part, at(150., x), FOLD, tap, 0.);
                element(format!("C{port}"), SPLITTER, at(x, x), FOLD, 0.5, phase);
                // The combiner's +y output (signal transmitted, LO reflected)
                // carries the complementary fringe; dump it before the next LO row.
                dumps.push(mount_element(
                    &format!("D{port}"),
                    DUMP,
                    at(x, x + 45.),
                    [0., -1., 0.],
                ));
                // Each later row folds 25 mm further out, so a −y fold passes
                // beside the earlier rows' mirrors and lands on its own ROI.
                element(
                    format!("K{port}"),
                    MIRROR,
                    at(575. + offset, x),
                    [-1., -1., 0.],
                    1.,
                    0.,
                );
            }
            scene.elements.extend(dumps);
            let mut detector = mount_element("CAM", DETECTOR, camera, [0., -1., 0.]);
            detector.label = "CAM (4-port camera)".into();
            detector.aperture = 45.;
            detector.body_radius = 50.;
            detector.readout = Some(Readout {
                ports: HYBRID_PORTS
                    .map(|(id, _, offset)| ReadoutPort {
                        id: id.into(),
                        roi: [offset, 0., 5.6, 5.6],
                    })
                    .into(),
                modes: ModeBasis {
                    basis: ModeFamily::HermiteGauss,
                    indices: vec![[0, 0], [1, 0], [0, 1], [1, 1]],
                    custom: vec![],
                    waist_um: 700.,
                    normalization: ModeNormalization::Power,
                },
                balanced_pairs: [
                    ("I+", "I-", Quadrature::Real),
                    ("Q+", "Q-", Quadrature::Imag),
                ]
                .map(|(plus, minus, quadrature)| BalancedPair {
                    signal: "iq".into(),
                    plus: plus.into(),
                    minus: minus.into(),
                    quadrature,
                })
                .into(),
            });
            scene.elements.push(detector);
            let mut edge = |from: &str, port: &str, to: &str| {
                built.push(Connection {
                    from: from.into(),
                    port: port.into(),
                    to: to.into(),
                });
            };
            edge("S1", "out", "BS0");
            edge("BS0", "transmitted", "TS1");
            edge("BS0", "reflected", "TL1");
            for k in 1..=4 {
                let port = HYBRID_PORTS[k - 1].0;
                for chain in ["TS", "TL"] {
                    let tap = format!("{chain}{k}");
                    edge(&tap, "reflected", &format!("C{port}"));
                    if k < 4 {
                        edge(&tap, "transmitted", &format!("{chain}{}", k + 1));
                    }
                }
                edge(&format!("C{port}"), "reflected", &format!("K{port}"));
                edge(&format!("C{port}"), "transmitted", &format!("K{port}"));
                edge(&format!("K{port}"), "reflected", "CAM");
            }
            ("Four-port modal hybrid", &[])
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
    scene.connections.extend(built);
    if name == "mach_zehnder" {
        scene.branches = [
            ("arm_a", "reflected", "in_a"),
            ("arm_b", "transmitted", "in_b"),
        ]
        .into_iter()
        .map(|(id, output, input)| Branch {
            id: id.into(),
            from: Port::new("BS1", output),
            to: Port::new("BS2", input),
            via: vec![],
            style: None,
        })
        .collect();
        scene.assertions = vec![Assertion {
            id: "equal_arms".into(),
            kind: AssertionKind::EqualOpl,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit: AssertionUnit::Mm,
            tolerance: 1e-6,
            band_nm: None,
            quantity: None,
        }];
    }
    if name == "modal_hybrid" {
        // Signal arms leave BS0 transmitted and LO arms reflected; each walks its
        // tap chain to the port's combiner.
        let mut branches = Vec::new();
        for (k, (port, _, _)) in HYBRID_PORTS.into_iter().enumerate() {
            for (arm, first, chain) in [("signal", "transmitted", "TS"), ("lo", "reflected", "TL")]
            {
                let mut via = vec![Port::new("BS0", first)];
                via.extend((1..=k).map(|i| Port::new(&format!("{chain}{i}"), "transmitted")));
                if k < 3 {
                    via.push(Port::new(&format!("{chain}{}", k + 1), "reflected"));
                }
                via.push(Port::new(
                    &format!("C{port}"),
                    if arm == "signal" {
                        "reflected"
                    } else {
                        "transmitted"
                    },
                ));
                branches.push(Branch {
                    id: format!("{port} {arm}"),
                    from: Port::new("S1", "out"),
                    to: Port::new("CAM", "in"),
                    via,
                    style: None,
                });
            }
        }
        scene.branches = branches;
    }
    if name == "three_arm" {
        scene.branches = ["L", "C", "R"]
            .map(|arm| Branch {
                id: arm.into(),
                from: Port::new("AP", arm),
                to: Port::new(&format!("D_{arm}"), "in"),
                via: vec![],
                style: None,
            })
            .into();
    }
    if name == "delay_line" {
        let members = ["M3".into(), "M4".into()];
        let reference_pose = scene
            .elements
            .iter()
            .filter(|e| members.contains(&e.id))
            .map(|e| {
                (
                    e.id.clone(),
                    StagePose {
                        position: e.position,
                        rotation: e.rotation(),
                    },
                )
            })
            .collect();
        scene.stages.push(Stage {
            id: "delay".into(),
            members: members.into(),
            reference_pose,
            axis: X,
            travel: [-12.5, 12.5],
            position_mm: 0.,
            pzt: Some(StagePzt {
                range_um: [-5., 5.],
                value_um: 0.,
            }),
        });
        scene.branches = [
            ("arm_reference", "reflected", "in_a"),
            ("arm_delay", "transmitted", "in_b"),
        ]
        .into_iter()
        .map(|(id, output, input)| Branch {
            id: id.into(),
            from: Port::new("BS1", output),
            to: Port::new("BS2", input),
            via: vec![],
            style: None,
        })
        .collect();
        // At zero the delay arm has two 300 mm legs in addition to the reference length.
        scene.assertions = [
            (
                "delay_opl",
                AssertionKind::EqualOpl,
                600.,
                AssertionUnit::Mm,
                1e-3,
            ),
            (
                "combiner_position",
                AssertionKind::Colinear,
                0.,
                AssertionUnit::Mm,
                1e-3,
            ),
            (
                "combiner_angle",
                AssertionKind::Colinear,
                0.,
                AssertionUnit::Mrad,
                1e-3,
            ),
        ]
        .into_iter()
        .map(|(id, kind, target, unit, tolerance)| Assertion {
            id: id.into(),
            kind,
            branches: ["arm_delay".into(), "arm_reference".into()],
            target,
            unit,
            tolerance,
            band_nm: None,
            quantity: None,
        })
        .collect();
    }
    scene.validate()?;
    Ok(scene)
}

fn reflective(kind: Kind) -> bool {
    bench::ports(kind).contains(&"reflected")
}

pub fn auto_aim(bench: &Bench, snap_holes: bool, uniform_z: Option<f64>) -> Result<Value, String> {
    serde_json::to_value(aim(bench, snap_holes, uniform_z, &System::default())?)
        .map_err(|e| e.to_string())
}

#[derive(schemars::JsonSchema, Serialize)]
pub(crate) struct AimResult {
    pub bench: Bench,
    pub ok: bool,
    adjusted_elements: Vec<String>,
    warnings: Vec<String>,
    conflicts: Vec<Port>,
    residuals: Vec<AimResidual>,
    /// Beam clearance of each periscope on the aimed bench.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    periscopes: Vec<layout::PeriscopeReading>,
}

pub(crate) fn aim(
    bench: &Bench,
    snap_holes: bool,
    uniform_z: Option<f64>,
    system: &System,
) -> Result<AimResult, String> {
    let original = bench;
    let bench = original.resolved_stages()?;
    let paths = bench.draft_paths()?;
    if uniform_z.is_some_and(|z| !z.is_finite() || z <= 0.) {
        return Err("Uniform Z must be finite and positive".into());
    }
    let mut scene = bench.as_ref().clone();
    let stage_members: HashSet<_> = bench.stages.iter().flat_map(|s| &s.members).collect();
    for element in &mut scene.elements {
        if stage_members.contains(&element.id) {
            continue;
        }
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
    place_layers(&mut scene, &stage_members);
    let indices: HashMap<_, _> = scene
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.clone(), i))
        .collect();
    let combiners = Combiner::find(&bench, &paths);
    let adjustable: Vec<_> = scene
        .elements
        .iter()
        .map(|e| !stage_members.contains(&e.id) && (e.rotation.is_none() || e.aim_rotation))
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
            if source.aim_direction {
                source.direction = direction;
            } else if dot(unit(source.direction)?, direction) < 1. - 1e-9 {
                warnings.push(format!(
                    "{}: fixed launch direction differs from its nominal connection",
                    source.id
                ));
            }
            pending.push_back((target, unit(source.direction)?));
        }
    }
    let mut incoming = HashMap::new();
    while let Some((i, direction)) = pending.pop_front() {
        let element = &scene.elements[i];
        let combiner = combiners.iter().find(|c| c.index == i);
        if let Some(&first) = incoming.get(&i) {
            if combiner.is_none() && dot(first, direction) < 1. - 1e-9 {
                warnings.push(format!(
                    "{} receives multiple directions; aimed for the first path",
                    element.id
                ));
            }
            continue;
        }
        incoming.insert(i, direction);
        let mut normal = combiner
            .and_then(|c| c.nominal_normal(&scene))
            .or_else(|| (!reflective(element.kind)).then_some(direction));
        for link in scene.connections.iter().filter(|c| c.from == element.id) {
            let j = indices[&link.to];
            let Ok(out) = unit(sub(scene.elements[j].position, element.position)) else {
                warnings.push(format!(
                    "{} / {}: coincident connection endpoints",
                    element.id, link.to
                ));
                continue;
            };
            pending.push_back((j, out));
            if combiner.is_some() {
                continue;
            }
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
        }
        if adjustable[i] {
            if let Some(normal) = normal {
                let mut angles = normal_rotation(normal);
                if let Some(rotation) = element.rotation {
                    angles[0] = rotation[0];
                }
                scene.elements[i].rotation = Some(angles);
            } else if !scene
                .connections
                .iter()
                .any(|c| c.from == element.id && c.port == "reflected")
            {
                warnings.push(format!("{}: no reflected target to aim at", element.id));
            }
        }
    }
    for (i, e) in scene.elements.iter().enumerate() {
        if !incoming.contains_key(&i) {
            warnings.push(format!("{}: no incoming connection from a source", e.id));
        }
    }
    scene.validate_draft()?;
    let (mut conflicts, residuals) =
        alignment_solver::align(&mut scene, &bench, system, &combiners, &adjustable)?;
    // Stage members keep their stage pose, so one off its layer height cannot be
    // aimed onto the layer: refuse instead of returning a bench that fails to trace.
    // Periscope mirrors are exempt from their layer height, as in validation.
    let mirrors: HashSet<&String> = scene
        .periscopes
        .iter()
        .flat_map(|p| [&p.lower, &p.upper])
        .collect();
    for e in scene
        .elements
        .iter()
        .filter(|e| stage_members.contains(&e.id) && !mirrors.contains(&e.id))
    {
        if let Some(height) = scene.layer_height(e)
            && (e.position[2] - height).abs() > 1e-9
        {
            warnings.push(format!(
                "{}: stage member sits at Z = {} mm, off layer {} at {height} mm",
                e.id,
                e.position[2],
                e.layer.as_deref().unwrap_or_default()
            ));
            conflicts.push(Port::new(&e.id, "layer"));
        }
    }
    // A periscope must rise vertically and leave level onto its target.
    let mut periscopes = vec![];
    if conflicts.is_empty() && !scene.periscopes.is_empty() {
        match center_trace(&scene, system) {
            Ok(trace) => match scene.trace(system) {
                Ok(full) => periscopes = layout::periscopes(&scene, system, &trace, &full),
                Err(error) => {
                    warnings.push(format!("periscope geometry not traced: {error}"));
                    for p in &scene.periscopes {
                        conflicts.push(Port::new(&p.upper, "reflected"));
                    }
                }
            },
            Err(error) => {
                warnings.push(format!("periscope geometry not traced: {error}"));
                for p in &scene.periscopes {
                    conflicts.push(Port::new(&p.upper, "reflected"));
                }
            }
        }
        for reading in &periscopes {
            if let Some(stop) = &reading.interruption {
                warnings.push(format!(
                    "periscope {}: {} at {}",
                    reading.id, stop.reason, stop.element
                ));
                conflicts.push(Port::new(&stop.element, &stop.port));
            }
        }
    }
    let ok = conflicts.is_empty();
    let mut adjusted_elements = vec![];
    if ok {
        adjusted_elements.extend(
            scene
                .elements
                .iter()
                .zip(&bench.elements)
                .filter(|(new, old)| new.rotation != old.rotation || new.position != old.position)
                .map(|(element, _)| element.id.clone()),
        );
    } else {
        scene = original.clone();
    }
    Ok(AimResult {
        bench: scene,
        ok,
        adjusted_elements,
        warnings,
        conflicts,
        residuals,
        periscopes,
    })
}

/// Holds layered elements at their layer height and stands each periscope's upper
/// mirror directly above its lower mirror. A periscope mirror without a layer
/// takes the height of the beam it exchanges with the rest of the bench: the
/// lower mirror its upstream neighbour's, the upper mirror its downstream one's.
fn place_layers(scene: &mut Bench, fixed: &HashSet<&String>) {
    let mirrors: HashSet<String> = scene
        .periscopes
        .iter()
        .flat_map(|p| [p.lower.clone(), p.upper.clone()])
        .collect();
    let heights: Vec<_> = scene
        .elements
        .iter()
        .map(|e| scene.layer_height(e))
        .collect();
    for (e, height) in scene.elements.iter_mut().zip(heights) {
        if let Some(z) = height
            && !fixed.contains(&e.id)
            && !mirrors.contains(&e.id)
        {
            e.position[2] = z;
        }
    }
    let position = |scene: &Bench, id: &str| {
        scene
            .elements
            .iter()
            .map(|e| (&e.id, e.position))
            .chain(scene.sources.iter().map(|s| (&s.id, s.position)))
            .find(|(i, _)| *i == id)
            .map(|(_, p)| p)
    };
    for p in scene.periscopes.clone() {
        let (Some(lo), Some(up)) = (
            scene.elements.iter().position(|e| e.id == p.lower),
            scene.elements.iter().position(|e| e.id == p.upper),
        ) else {
            continue;
        };
        let upstream = scene
            .connections
            .iter()
            .find(|c| c.to == p.lower)
            .and_then(|c| position(scene, &c.from));
        let downstream = scene
            .connections
            .iter()
            .find(|c| c.from == p.upper && c.port == "reflected")
            .and_then(|c| position(scene, &c.to));
        if !fixed.contains(&p.lower)
            && let Some(z) = scene
                .layer_height(&scene.elements[lo])
                .or(upstream.map(|u| u[2]))
        {
            scene.elements[lo].position[2] = z;
        }
        if !fixed.contains(&p.upper) {
            let base = scene.elements[lo].position;
            let z = scene
                .layer_height(&scene.elements[up])
                .or(downstream.map(|d| d[2]))
                .unwrap_or(scene.elements[up].position[2]);
            scene.elements[up].position = [base[0], base[1], z];
        }
    }
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
    port: &'a str,
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

/// Periscope readings off the bench's centre rays (see [`layout::periscopes`]).
fn periscope_readings(
    bench: &Bench,
    system: &System,
    full: &Trace,
) -> Vec<layout::PeriscopeReading> {
    if bench.periscopes.is_empty() {
        return vec![];
    }
    center_trace(bench, system)
        .map(|trace| layout::periscopes(bench, system, &trace, full))
        .unwrap_or_default()
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
    center.trace_resolved(system)
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
    let mut states = HashMap::<Vec<Port>, BeamState>::new();
    let mut groups = HashMap::<Vec<Port>, GroupState>::new();
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
            vec![Port::new(&source.id, "out")],
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
                    .get_mut(&segment.beam)
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
            "substrate" => {
                // Inside glass the beam propagates over the reduced distance L·n_air/n.
                let state = states
                    .get_mut(&segment.beam)
                    .ok_or("Missing Gaussian substrate branch")?;
                let index = segment.flight.as_ref().map_or(n, |flight| flight.index);
                let from = state.q.radius(wavelength, n)?;
                state.q = state
                    .q
                    .transfer(norm(sub(segment.to, segment.from)) * n / index);
                (from, state.q.radius(wavelength, n)?)
            }
            _ => {
                let state = states
                    .remove(&segment.beam)
                    .ok_or("Missing Gaussian center-ray branch")?;
                let q = state.q.transfer(norm(sub(segment.to, segment.from)));
                let to = q.radius(wavelength, n)?;
                if let Some(e) = element.filter(|_| {
                    matches!(
                        segment.status.as_str(),
                        "hit" | "clipped" | "beam_stop" | "masked"
                    )
                }) {
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
                                    segment.beam.clone(),
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
                        for port in e.ports().into_iter().filter(|_| e.regions.is_empty()) {
                            states.insert(
                                next_beam(&segment.beam, &e.id, port),
                                BeamState {
                                    q: next,
                                    from: &e.id,
                                    port,
                                },
                            );
                        }
                    }
                    // A region passes the incoming beam unchanged when it is wider than
                    // the beam. A narrower region truncates it: keep the wavefront
                    // curvature (Re 1/q) and clamp the spot radius to the region
                    // (Im 1/q = -λ/(π n a²)). Each region arm, lit or probed, is keyed by
                    // its structured beam identity.
                    for region in &e.regions {
                        let a = region.shape.inscribed_radius();
                        let q_out = if to <= a {
                            q
                        } else {
                            let norm = q.re * q.re + q.im * q.im;
                            let (inv_re, inv_im) = (q.re / norm, -wavelength / (PI * n * a * a));
                            let norm = inv_re * inv_re + inv_im * inv_im;
                            Q {
                                re: inv_re / norm,
                                im: -inv_im / norm,
                            }
                        };
                        states.insert(
                            next_beam(&segment.beam, &e.id, &region.port),
                            BeamState {
                                q: q_out,
                                from: &e.id,
                                port: &region.port,
                            },
                        );
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
    let scene = bench.resolved_stages()?;
    let bench = scene.as_ref();
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
    let [x, y, _] = bench::local(
        sub(point, element.position),
        [0., 0., element.rotation()[2]],
    );
    x.abs() <= mount.base_size[0] / 2. && y.abs() <= mount.base_size[1] / 2.
}

fn base_corners(element: &Element, mount: &Mount) -> [V3; 4] {
    let [w, h] = mount.base_size.map(|x| x / 2.);
    [[-w, -h, 0.], [w, -h, 0.], [w, h, 0.], [-w, h, 0.]].map(|p| {
        add(
            element.position,
            bench::rotate(p, [0., 0., element.rotation()[2]]),
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
    let scene = bench.resolved_stages()?;
    let bench = scene.as_ref();
    let trace = bench.trace_resolved(system)?;
    let center = center_trace(bench, system)?;
    let gaussian = envelope(bench, system, &center)?;
    lab_report(bench, system, &trace, &gaussian)
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
    /// No errors and no pending inputs.
    passed: bool,
    /// Pending inputs on the bench, each also a pending_input warning.
    pending: u32,
    counts: IssueCounts,
    issues: Vec<Issue>,
    #[serde(flatten)]
    readings: LabReadings,
    gaussian: GaussianEnvelope,
    recommended_beam_dumps: Vec<BeamDump>,
    clamping: Vec<Clamping>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct LabReadings {
    stages: Vec<StageReading>,
    branches: Vec<BranchReading>,
    assertions: Vec<AssertionResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    periscopes: Vec<layout::PeriscopeReading>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    zones: Vec<layout::ZoneReading>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    folds: Vec<layout::FoldReading>,
    /// Reflection count, parity and handedness at the end of each complete branch.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    handedness: Vec<relay::HandednessReading>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    relays: Vec<relay::RelayReading>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct StageReading {
    id: String,
    position_mm: f64,
    pzt_um: Option<f64>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct BranchReading {
    id: String,
    stage_ids: Vec<String>,
    /// The branch's colour on the bench drawing and blueprint.
    color: String,
    paths: Vec<PathReading>,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    interruption: Option<PathBreak>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct PathReading {
    source: String,
    wavelength_nm: f64,
    /// Optical length accumulated up to the interruption if the path is incomplete.
    opl_mm: f64,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    interruption: Option<PathBreak>,
}

#[derive(schemars::JsonSchema, Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum AssertionStatus {
    Pass,
    Fail,
    /// The value's basis cannot prove the target either way: an estimate, or a
    /// bound on the side the tolerance leaves open.
    Indeterminate,
    Unreachable,
}

impl AssertionStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Indeterminate => "indeterminate",
            Self::Unreachable => "unreachable",
        }
    }

    /// The verdict on `residual` (value − target) for a value of this basis.
    /// A lower bound can only prove the value too large, an upper bound only
    /// too small; an estimate proves nothing.
    fn judge(residual: f64, tolerance: f64, kind: BasisKind) -> Self {
        match kind {
            BasisKind::Model if residual.abs() <= tolerance => Self::Pass,
            BasisKind::Model => Self::Fail,
            BasisKind::LowerBound if residual > tolerance => Self::Fail,
            BasisKind::UpperBound if residual < -tolerance => Self::Fail,
            _ => Self::Indeterminate,
        }
    }
}

/// What a reported value is: the exact value of the model (`model`), a bound
/// the real value cannot cross, or an `estimate`. A value reported without a
/// basis is a model value.
#[derive(schemars::JsonSchema, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BasisKind {
    #[default]
    Model,
    /// The real value is at least the reported one.
    LowerBound,
    /// The real value is at most the reported one.
    UpperBound,
    Estimate,
}

#[derive(schemars::JsonSchema, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssumptionCode {
    /// A splitter or PBS without a substrate is a zero-thickness surface
    /// (a pellicle): no glass, no lateral shift.
    ThinSplitter,
    /// A thin lens carries power with no thickness: the glass a real lens puts
    /// on the path is left out.
    ThinLens,
    /// A waveplate or polarizer without a substrate has no thickness: the
    /// crystal or film a real one puts on the path is left out.
    ThinPlate,
    /// Paraxial ray-transfer matrices, with each thin element's principal
    /// planes at the element plane.
    Paraxial,
}

#[derive(schemars::JsonSchema, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OmissionCode {
    /// The band search sampled the largest deviation it reports but could not
    /// close the enclosure of the maximum within its budget.
    BandEnclosure,
}

#[derive(schemars::JsonSchema, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct Assumption {
    pub code: AssumptionCode,
    /// Elements the assumption applies to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<String>,
}

#[derive(schemars::JsonSchema, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct Omission {
    pub code: OmissionCode,
    pub detail: String,
}

/// The basis of a reported value and the assumptions or omissions behind it.
#[derive(schemars::JsonSchema, Serialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct Basis {
    pub kind: BasisKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<Assumption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<Omission>,
}

impl Basis {
    pub(crate) fn is_plain(&self) -> bool {
        self.kind == BasisKind::Model && self.assumptions.is_empty() && self.omitted.is_empty()
    }

    fn assume(&mut self, code: AssumptionCode, elements: Vec<String>) {
        if !elements.is_empty() || code == AssumptionCode::Paraxial {
            self.assumptions.push(Assumption { code, elements });
        }
    }

    /// One line for drawings and issues, e.g. "lower bound · thin_lens L1".
    fn summary(&self) -> String {
        let kind = match self.kind {
            BasisKind::Model => "model",
            BasisKind::LowerBound => "lower bound",
            BasisKind::UpperBound => "upper bound",
            BasisKind::Estimate => "estimate",
        };
        let mut parts = vec![kind.to_string()];
        for a in &self.assumptions {
            let code = serde_json::to_value(a.code).unwrap();
            let code = code.as_str().unwrap_or_default();
            parts.push(if a.elements.is_empty() {
                code.to_string()
            } else {
                format!("{code} {}", a.elements.join(", "))
            });
        }
        for o in &self.omitted {
            let code = serde_json::to_value(o.code).unwrap();
            parts.push(format!("omits {}", code.as_str().unwrap_or_default()));
        }
        parts.join(" · ")
    }
}

/// Thin elements an arm crosses. Its ports list each element it meets as a
/// run of input and output ports; the final run, the endpoint input where the
/// comparison is made, is not crossed.
struct ThinCrossings {
    /// Splitters without a substrate, crossed by transmission.
    splitters: Vec<String>,
    /// Thin lenses and substrate-less waveplates and polarizers: glass a real
    /// element would add to this arm.
    lenses: Vec<String>,
    plates: Vec<String>,
}

fn thin_crossings(bench: &Bench, path: &BranchPath) -> ThinCrossings {
    let mut crossings = ThinCrossings {
        splitters: vec![],
        lenses: vec![],
        plates: vec![],
    };
    let mut runs: Vec<&[Port]> = path.ports.chunk_by(|a, b| a.element == b.element).collect();
    if path.interruption.is_none() {
        runs.pop();
    }
    for run in runs {
        let Some(e) = bench.elements.iter().find(|e| e.id == run[0].element) else {
            continue;
        };
        if e.substrate.is_some() {
            continue;
        }
        match e.kind {
            Kind::Splitter | Kind::Pbs if run.iter().any(|p| p.port == "transmitted") => {
                crossings.splitters.push(e.id.clone())
            }
            Kind::Lens => crossings.lenses.push(e.id.clone()),
            Kind::Waveplate | Kind::Polarizer => crossings.plates.push(e.id.clone()),
            _ => {}
        }
    }
    crossings
}

/// Removes the elements both lists hold (one crossing each), so what is left
/// is the glass one arm has and the other lacks.
fn unmatched(a: &[String], b: &[String]) -> [Vec<String>; 2] {
    let mut rest = b.to_vec();
    let mut only_a = Vec::new();
    for id in a {
        match rest.iter().position(|r| r == id) {
            Some(i) => {
                rest.remove(i);
            }
            None => only_a.push(id.clone()),
        }
    }
    [only_a, rest]
}

fn sorted_unique(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
}

/// The basis of a two-arm comparison, first arm minus second. Glass a thin
/// element leaves out only ever lengthens its arm's optical path, so omitted
/// glass on one arm bounds the difference from that side; on both arms it
/// leaves an estimate. `bounded` is false for quantities whose sign the
/// omitted glass does not fix (dispersion): any omission is an estimate.
fn comparison_basis(bench: &Bench, paths: [&BranchPath; 2], bounded: bool) -> Basis {
    let [a, b] = paths.map(|path| thin_crossings(bench, path));
    let thick = |c: &ThinCrossings| [c.lenses.clone(), c.plates.clone()].concat();
    let [only_a, only_b] = unmatched(&thick(&a), &thick(&b));
    let kind = match (only_a.is_empty(), only_b.is_empty()) {
        (true, true) => BasisKind::Model,
        _ if !bounded => BasisKind::Estimate,
        (false, true) => BasisKind::LowerBound,
        (true, false) => BasisKind::UpperBound,
        (false, false) => BasisKind::Estimate,
    };
    let unmatched_of = |ids: &[String]| -> Vec<String> {
        sorted_unique(
            ids.iter()
                .filter(|id| only_a.contains(id) || only_b.contains(id))
                .cloned()
                .collect(),
        )
    };
    let mut basis = Basis {
        kind,
        ..Basis::default()
    };
    basis.assume(
        AssumptionCode::ThinSplitter,
        sorted_unique([a.splitters.clone(), b.splitters.clone()].concat()),
    );
    basis.assume(
        AssumptionCode::ThinLens,
        unmatched_of(&[a.lenses, b.lenses].concat()),
    );
    basis.assume(
        AssumptionCode::ThinPlate,
        unmatched_of(&[a.plates, b.plates].concat()),
    );
    basis
}

#[derive(schemars::JsonSchema, Serialize)]
struct AssertionResult {
    assertion: String,
    kind: AssertionKind,
    branches: [String; 2],
    /// Input endpoint of the second arm, at the comparison element.
    at: Port,
    value: Option<AssertionValue>,
    unit: AssertionUnit,
    target: f64,
    tolerance: f64,
    residual: Option<f64>,
    status: AssertionStatus,
    /// Basis of `value`; absent for a model value with no stated assumption.
    #[serde(default, skip_serializing_if = "Basis::is_plain")]
    basis: Basis,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    interruption: Option<PathBreak>,
}

#[derive(Clone, schemars::JsonSchema, Serialize)]
#[serde(untagged)]
enum AssertionValue {
    Opl(f64),
    Colinear(Separation),
    Dispersion(DispersionValue),
    Parity(ParityValue),
    /// Image rotation, rad internally and in the assertion unit when reported.
    Rotation(f64),
}

#[derive(Clone, schemars::JsonSchema, Serialize)]
struct ParityValue {
    /// Reflections along the first and second branch.
    reflections: [u32; 2],
    /// Their difference modulo 2.
    parity: u32,
}

#[derive(Clone, schemars::JsonSchema, Serialize)]
struct DispersionValue {
    /// First branch minus second, in the assertion unit, at `wavelength_nm`.
    difference: f64,
    /// Band wavelength where the difference deviates most from the target.
    wavelength_nm: f64,
    /// Rigorous upper bound of max |Δ − target| over the band, in the assertion unit.
    deviation_bound: f64,
    /// Substrate glass on the first and second branch.
    glass: [Vec<GlassPass>; 2],
}

impl AssertionValue {
    fn in_unit(&self, unit: AssertionUnit) -> f64 {
        match self {
            Self::Opl(value) => unit.convert_mm(*value),
            Self::Dispersion(value) => value.difference,
            Self::Parity(value) => f64::from(value.parity),
            Self::Rotation(rad) => match unit {
                AssertionUnit::Mrad => rad * 1000.,
                _ => *rad,
            },
            Self::Colinear(separation) => match unit {
                AssertionUnit::Mrad => separation.angle_mrad,
                _ => unit.convert_mm(separation.position_mm),
            },
        }
    }
}

fn reached<'a>(report: &'a BranchReport, from: &Port) -> Result<&'a BranchPath, PathBreak> {
    if let Some(interruption) = &report.interruption {
        return Err(interruption.clone());
    }
    let [path] = report.paths.as_slice() else {
        return Err(PathBreak::new(&from.element, &from.port, "ambiguous_start"));
    };
    if let Some(interruption) = &path.interruption {
        return Err(interruption.clone());
    }
    Ok(path)
}

fn equal_opl(a: &BranchPath, b: &BranchPath, from: &Port) -> Result<f64, PathBreak> {
    // Both paths were built from the same split arrival, so they share a source and wavelength.
    if a.start_segment != b.start_segment {
        return Err(PathBreak::new(&from.element, &from.port, "different_start"));
    }
    Ok(a.opl_mm - b.opl_mm)
}

fn colinear(
    trace: &Trace,
    bench: &Bench,
    branches: [&Branch; 2],
    paths: [&BranchPath; 2],
) -> Result<Separation, PathBreak> {
    let [a, b] = branches.map(|branch| &branch.to);
    if a.element != b.element {
        return Err(PathBreak::new(&b.element, &b.port, "different_combiner"));
    }
    if !bench
        .elements
        .iter()
        .any(|e| e.id == a.element && matches!(e.kind, Kind::Splitter | Kind::Pbs))
    {
        return Err(PathBreak::new(&a.element, &a.port, "not_a_combiner"));
    }
    let [first, second] = paths.map(|path| path.end_segment.expect("reached branch endpoint"));
    if first == second {
        return Err(PathBreak::new(&b.element, &b.port, "same_arrival"));
    }
    measure(trace, [first, second], b).map(|overlap| overlap.separation)
}

/// Speed of light in mm/fs.
const C_MM_PER_FS: f64 = 2.997_924_58e-4;
/// Initial cells of the dispersion search, and its evaluation budget.
const DISPERSION_CELLS: usize = 64;
const DISPERSION_MAX_EVALUATIONS: usize = 200_000;

/// Net glass of one material over the two paths: path A counts +L, path B −L.
struct NetGlass<'a> {
    material: &'a str,
    element: &'a str,
    length_mm: f64,
}

/// Δ(w) and dΔ/dw for the scalar type `T`, from second-order duals of n(w).
/// The error names the failing material, its element, and the message.
fn dispersion_delta<T: Real>(
    system: &System,
    glass: &[NetGlass],
    group: bool,
    w: T,
) -> Result<(T, T), (String, String, String)> {
    let x = Dual {
        v: Dual::var(w),
        d: Dual {
            v: T::cst(1.),
            d: T::cst(0.),
        },
    };
    let (mut value, mut slope) = (T::cst(0.), T::cst(0.));
    for g in glass {
        let n = |material: &str| {
            system
                .material_index_real(material, x)
                .map_err(|err| (material.to_string(), g.element.to_string(), err))
        };
        let excess = n(g.material)? - n("AIR")?;
        let (e0, e1, e2) = (excess.v.v, excess.v.d, excess.d.d);
        if group {
            let k = g.length_mm / C_MM_PER_FS;
            value = value + (e0 - w * e1) * k;
            slope = slope - w * e2 * k;
        } else {
            let k = TAU * g.length_mm * 1e6;
            value = value + e0 / w * k;
            slope = slope + (e1 * w - e0) / (w * w) * k;
        }
    }
    Ok((value, slope))
}

/// Material dispersion of the substrate glass on two paths, Δ(λ) = Q₁(λ) − Q₂(λ)
/// with Q = Σ (n − n_air)·L·2π/λ for phase (rad) or Σ (n_g − n_g,air)·L/c for group
/// delay (fs), where n_g = n − λ·dn/dλ. Derivatives of n are exact (forward-mode
/// duals through the dispersion formula), not finite differences.
///
/// The band maximum of |Δ − target| is found by branch and bound over rigorous
/// enclosures. A cell [a, b] is enclosed by evaluating Δ in interval arithmetic
/// (outward rounded), intersected with the mean-value form Δ(m) + Δ'([a, b])·([a, b] − m),
/// so its upper bound holds for every wavelength in the cell, not only for samples.
/// The band starts as 64 cells; the cell with the highest bound is halved until that
/// bound is within 1e-10 (relative) of the best midpoint sample. Cells narrower than
/// 1e-9 nm stop splitting, the search also stops once no cell's bound exceeds the
/// largest bound of such an unsplittable cell, and evaluations are capped at 200 000. The reported Δ is the
/// best sample and `deviation_bound` the largest bound left, so the assertion passes
/// only if every wavelength in the band is within tolerance. If the model cannot be
/// enclosed somewhere (a pole or domain edge inside a cell) the assertion is
/// unresolved; if the search ends with the best sample within tolerance but the
/// bound outside it, the sample is a lower bound of the band maximum and the
/// assertion is indeterminate (a `band_enclosure` omission), not guessed.
fn dispersion(
    system: &System,
    assertion: &Assertion,
    paths: [&BranchPath; 2],
    at: &Port,
) -> Result<DispersionValue, PathBreak> {
    let [lo, hi] = assertion.band_nm.expect("validated dispersion band");
    let group = assertion.quantity == Some(DispersionQuantity::GroupDelay);
    let mut glass: Vec<NetGlass> = vec![];
    for (sign, path) in [(1., paths[0]), (-1., paths[1])] {
        for pass in &path.glass {
            match glass.iter_mut().find(|g| g.material == pass.material) {
                Some(g) => g.length_mm += sign * pass.length_mm,
                None => glass.push(NetGlass {
                    material: &pass.material,
                    element: &pass.element,
                    length_mm: sign * pass.length_mm,
                }),
            }
        }
    }
    let target = assertion.target;
    // Samples and enclosures evaluate every material, including those whose net
    // length cancels: the value drops out, but the model must still be defined
    // over the whole band (0·∞ makes the enclosure unbounded).
    let sample = |w: f64| {
        dispersion_delta(system, &glass, group, Iv::cst(w))
            .map(|(value, _)| (value.lo + value.hi) / 2.)
            .map_err(|(material, element, err)| {
                PathBreak::new(element.as_str(), "substrate", "material_error")
                    .with_detail(format!("{material} at {w} nm: {err}"))
            })
    };
    let upper = |a: f64, b: f64, m: f64| -> f64 {
        let (Ok((naive, slope)), Ok((center, _))) = (
            dispersion_delta(system, &glass, group, Iv::new(a, b)),
            dispersion_delta(system, &glass, group, Iv::cst(m)),
        ) else {
            return f64::INFINITY;
        };
        let mean = center + slope * Iv::new((a - m).next_down(), (b - m).next_up());
        let (l, h) = (naive.lo.max(mean.lo), naive.hi.min(mean.hi));
        if !(l.is_finite() && h.is_finite()) {
            return f64::INFINITY;
        }
        (target - l).max(h - target).next_up()
    };
    let deviation = |d: f64| (d - target).abs();
    struct Cell {
        a: f64,
        b: f64,
        upper: f64,
    }
    impl PartialEq for Cell {
        fn eq(&self, other: &Self) -> bool {
            self.upper == other.upper
        }
    }
    impl Eq for Cell {}
    impl PartialOrd for Cell {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for Cell {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.upper.total_cmp(&other.upper)
        }
    }
    let mut worst = (sample(lo)?, lo);
    let mut evaluations = 1;
    let mut heap = std::collections::BinaryHeap::new();
    let visit = |a: f64, b: f64, worst: &mut (f64, f64)| -> Result<Cell, PathBreak> {
        let m = (a + b) / 2.;
        let fm = sample(m)?;
        for (value, w) in [(fm, m), (sample(b)?, b)] {
            if deviation(value) > deviation(worst.0) {
                *worst = (value, w);
            }
        }
        Ok(Cell {
            a,
            b,
            upper: upper(a, b, m),
        })
    };
    let edge = |i: usize| lo + (hi - lo) * i as f64 / DISPERSION_CELLS as f64;
    for i in 0..DISPERSION_CELLS {
        heap.push(visit(edge(i), edge(i + 1), &mut worst)?);
        evaluations += 4;
    }
    let mut settled = 0f64;
    while let Some(top) = heap.peek() {
        // Cells at the minimum width can no longer be refined, so the bound cannot
        // drop below `settled`; refining further only matters above it.
        let best = deviation(worst.0).max(settled);
        if top.upper - best <= 1e-10 * best.max(1.) || evaluations + 8 > DISPERSION_MAX_EVALUATIONS
        {
            break;
        }
        let Cell { a, b, upper } = heap.pop().unwrap();
        if b - a < 1e-9 {
            settled = settled.max(upper);
            continue;
        }
        let m = (a + b) / 2.;
        heap.push(visit(a, m, &mut worst)?);
        heap.push(visit(m, b, &mut worst)?);
        evaluations += 8;
    }
    let deviation_bound = heap
        .peek()
        .map_or(0., |top| top.upper)
        .max(settled)
        .max(deviation(worst.0));
    let (difference, wavelength_nm) = worst;
    // A bound the enclosure could not close below the tolerance leaves the
    // assertion indeterminate (see check_assertions); only a non-finite one
    // means there is no value at all.
    if !deviation_bound.is_finite() {
        return Err(PathBreak::new(&at.element, &at.port, "dispersion_unresolved").with_detail(format!(
            "largest sampled deviation {} at {wavelength_nm} nm, but the enclosure is unbounded",
            deviation(difference)
        )));
    }
    Ok(DispersionValue {
        difference,
        wavelength_nm,
        deviation_bound,
        glass: paths.map(|path| path.glass.clone()),
    })
}

fn check_assertions(bench: &Bench, system: &System, trace: &Trace) -> Vec<AssertionResult> {
    let reports: HashMap<_, _> = trace
        .branches
        .iter()
        .map(|report| (report.id.as_str(), report))
        .collect();
    let branches: HashMap<_, _> = bench
        .branches
        .iter()
        .map(|branch| (branch.id.as_str(), branch))
        .collect();
    bench.assertions.iter().map(|a| {
        let [first, second] = a.branches.each_ref().map(|id| branches[id.as_str()]);
        let paths = reached(reports[first.id.as_str()], &first.from)
            .and_then(|pa| Ok([pa, reached(reports[second.id.as_str()], &second.from)?]));
        let mut basis = match (&paths, a.kind) {
            (Ok(paths), AssertionKind::EqualOpl) => comparison_basis(bench, *paths, true),
            (Ok(paths), AssertionKind::Dispersion) => comparison_basis(bench, *paths, false),
            (Ok(paths), AssertionKind::Colinear) => {
                let mut basis = comparison_basis(bench, *paths, true);
                // A thin element's missing glass shifts no axis: colinearity is
                // a model value under the thin-splitter assumption alone. Unlike
                // the OPL, it is measured on the combiner's outputs, so a thin
                // combiner is assumed too.
                basis.kind = BasisKind::Model;
                basis.assumptions.retain(|a| a.code == AssumptionCode::ThinSplitter);
                let combiner = bench.elements.iter().find(|e| e.id == second.to.element);
                if let Some(e) = combiner.filter(|e| matches!(e.kind, Kind::Splitter | Kind::Pbs) && e.substrate.is_none()) {
                    match basis.assumptions.first_mut() {
                        Some(thin) => thin.elements = sorted_unique([thin.elements.clone(), vec![e.id.clone()]].concat()),
                        None => basis.assume(AssumptionCode::ThinSplitter, vec![e.id.clone()]),
                    }
                }
                basis
            }
            _ => Basis::default(),
        };
        let compared = paths.clone().and_then(|[pa, pb]| {
            match a.kind {
                AssertionKind::EqualOpl => equal_opl(pa, pb, &first.from).map(AssertionValue::Opl),
                AssertionKind::Colinear => colinear(trace, bench, [first, second], [pa, pb])
                    .map(AssertionValue::Colinear),
                AssertionKind::Dispersion => dispersion(system, a, [pa, pb], &second.to).map(AssertionValue::Dispersion),
                AssertionKind::Parity => relay::parity(bench, trace, [pa, pb], &second.to).map(|reflections| {
                    let parity = if first.id == second.id { reflections[0] } else { reflections[0].abs_diff(reflections[1]) } % 2;
                    AssertionValue::Parity(ParityValue { reflections, parity })
                }),
                AssertionKind::ImageRotation => relay::rotation(bench, trace, [pa, pb], &second.to).map(AssertionValue::Rotation),
            }
        });
        let measured = compared.as_ref().ok();
        let residual = measured.map(|value| match value {
            // Angles compare modulo a full turn.
            AssertionValue::Rotation(_) => {
                let full = AssertionValue::Rotation(TAU).in_unit(a.unit);
                let r = (value.in_unit(a.unit) - a.target).rem_euclid(full);
                if r > full / 2. { r - full } else { r }
            }
            _ => value.in_unit(a.unit) - a.target,
        });
        let value = measured.map(|value| match value {
            AssertionValue::Opl(_) => AssertionValue::Opl(value.in_unit(a.unit)),
            AssertionValue::Rotation(_) => AssertionValue::Rotation(value.in_unit(a.unit)),
            _ => value.clone(),
        });
        let status = match (residual, measured) {
            (None, _) => AssertionStatus::Unreachable,
            // The band maximum of |Δ − target| lies between the largest sampled
            // deviation and the enclosure's bound.
            (Some(residual), Some(AssertionValue::Dispersion(value))) => {
                if basis.kind == BasisKind::Estimate {
                    AssertionStatus::Indeterminate
                } else if residual.abs() > a.tolerance {
                    AssertionStatus::Fail
                } else if value.deviation_bound <= a.tolerance {
                    AssertionStatus::Pass
                } else {
                    basis.kind = BasisKind::LowerBound;
                    basis.omitted.push(Omission {
                        code: OmissionCode::BandEnclosure,
                        detail: format!(
                            "largest sampled deviation {} at {} nm; the enclosure bounds the band maximum by {}",
                            residual.abs(), value.wavelength_nm, value.deviation_bound
                        ),
                    });
                    AssertionStatus::Indeterminate
                }
            }
            (Some(residual), _) => AssertionStatus::judge(residual, a.tolerance, basis.kind),
        };
        AssertionResult {
            assertion: a.id.clone(),
            kind: a.kind,
            branches: a.branches.clone(),
            at: second.to.clone(),
            value,
            unit: a.unit,
            target: a.target,
            tolerance: a.tolerance,
            residual,
            status,
            basis,
            interruption: compared.err(),
        }
    }).collect()
}

fn lab_readings(bench: &Bench, system: &System, trace: &Trace) -> LabReadings {
    LabReadings {
        stages: bench
            .stages
            .iter()
            .map(|stage| StageReading {
                id: stage.id.clone(),
                position_mm: stage.position_mm,
                pzt_um: stage.pzt.as_ref().map(|pzt| pzt.value_um),
            })
            .collect(),
        branches: trace
            .branches
            .iter()
            .map(|report| BranchReading {
                id: report.id.clone(),
                color: drawing::branch_style(bench, &report.id).0,
                stage_ids: report.stage_ids.clone(),
                paths: report
                    .paths
                    .iter()
                    .map(|path| PathReading {
                        source: path.source.clone(),
                        wavelength_nm: path.wavelength_nm,
                        opl_mm: path.opl_mm,
                        interruption: path.interruption.clone(),
                    })
                    .collect(),
                interruption: report.interruption.clone(),
            })
            .collect(),
        assertions: check_assertions(bench, system, trace),
        periscopes: periscope_readings(bench, system, trace),
        zones: layout::zones(bench),
        folds: layout::folds(bench, trace),
        handedness: relay::handedness(bench, trace),
        relays: relay::relays(bench, system, trace),
    }
}

impl LabCheck {
    pub(crate) fn require_pass(&self) -> Result<(), String> {
        if self.passed {
            return Ok(());
        }
        let undecided: Vec<String> = self
            .readings
            .assertions
            .iter()
            .filter(|a| a.status == AssertionStatus::Indeterminate)
            .map(|a| format!("{} ({})", a.assertion, a.basis.summary()))
            .collect();
        let mut message = format!(
            "Bench lab check failed with {} errors and {} pending inputs",
            self.counts.errors, self.pending
        );
        if !undecided.is_empty() {
            message += &format!(
                "; {} indeterminate assertions: {}",
                undecided.len(),
                undecided.join("; ")
            );
        }
        Err(message)
    }
}

fn lab_report(
    bench: &Bench,
    system: &System,
    trace: &Trace,
    gaussian: &Envelope<'_>,
) -> Result<LabCheck, String> {
    let grid = Grid::new(bench)?;
    let mut issues = Vec::new();
    // Placeholders awaiting the user's value keep the bench off lab-ready.
    for input in &bench.pending {
        let field = serde_json::to_value(input.field)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let value = input.field.value(bench, &input.target).map_or_else(
            || "unset".to_owned(),
            |v| format!("{v} {}", input.field.unit()),
        );
        let note = input
            .note
            .as_deref()
            .map_or_else(String::new, |n| format!(": {n}"));
        issues.push(issue(
            "warning",
            "pending_input",
            Some(&input.target),
            None,
            format!(
                "{} {field} is a placeholder ({}) awaiting input{note}",
                input.target,
                value.trim_end()
            ),
        ));
    }
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
            "masked" => ("info", "masked"),
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
        if end.reason == "escaped"
            && let Some(segment) = last
            && let Ok(direction) = unit(sub(segment.to, segment.from))
            && let Some(position) = dump_position(bench, segment.from, direction)
        {
            dumps.push(BeamDump {
                branch: end.branch.clone(),
                source: end.source.clone(),
                position,
                direction,
                power: end.power,
            });
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
    for zone in layout::zones(bench) {
        for id in &zone.out_of_bounds {
            issues.push(issue(
                "error",
                "zone_bounds",
                Some(id),
                None,
                format!("{id}: footprint extends outside zone {}", zone.id),
            ));
        }
    }
    for periscope in periscope_readings(bench, system, trace) {
        if let Some(stop) = &periscope.interruption {
            issues.push(issue(
                "error",
                "periscope_geometry",
                Some(&stop.element),
                None,
                format!(
                    "periscope {}: {} at {}",
                    periscope.id, stop.reason, stop.element
                ),
            ));
        }
        for id in &periscope.conflicts {
            issues.push(issue(
                "error",
                "periscope_clearance",
                Some(id),
                None,
                format!("{id}: within the beam radius of periscope {}", periscope.id),
            ));
        }
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
    let readings = lab_readings(bench, system, trace);
    for result in &readings.assertions {
        if result.status == AssertionStatus::Indeterminate {
            let residual = result
                .residual
                .map_or_else(String::new, |r| format!("residual {r} "));
            issues.push(issue(
                "warning",
                "assertion_indeterminate",
                Some(result.at.element.as_str()),
                None,
                format!(
                    "{}: {residual}cannot prove the target {} ±{} ({})",
                    result.assertion,
                    result.target,
                    result.tolerance,
                    result.basis.summary()
                ),
            ));
            continue;
        }
        let (code, target, message) = match result.status {
            AssertionStatus::Pass | AssertionStatus::Indeterminate => continue,
            AssertionStatus::Fail => (
                "assertion_failed",
                Some(result.at.element.as_str()),
                format!(
                    "{}: residual {} exceeds tolerance {}",
                    result.assertion,
                    result.residual.unwrap(),
                    result.tolerance
                ),
            ),
            AssertionStatus::Unreachable => {
                let location = result.interruption.as_ref().unwrap();
                (
                    "assertion_unreachable",
                    Some(location.element.as_str()),
                    format!(
                        "{}: {} / {} ({})",
                        result.assertion, location.element, location.port, location.reason
                    ),
                )
            }
        };
        issues.push(issue("error", code, target, None, message));
    }
    for reading in &readings.relays {
        for check in reading.checks.iter().filter(|check| !check.passed) {
            let message = match (&check.interruption, check.residual) {
                (Some(at), _) => format!(
                    "relay {} {:?}: {} at {}",
                    reading.id, check.kind, at.reason, at.element
                ),
                (None, Some(residual)) => format!(
                    "relay {} {:?}: residual {residual} exceeds tolerance {}",
                    reading.id, check.kind, check.tolerance
                ),
                (None, None) => format!("relay {} {:?}: no value", reading.id, check.kind),
            };
            issues.push(issue("error", "relay_check", None, None, message));
        }
        // An explicit pupil that cannot be resolved fails the relay even when
        // its matrix checks pass.
        if let Some(at) = &reading.interruption
            && reading
                .checks
                .iter()
                .all(|check| check.interruption.is_none())
        {
            let detail = at.detail.as_deref().unwrap_or_default();
            issues.push(issue(
                "error",
                "relay_pupil",
                None,
                None,
                format!("relay {}: {} {detail}", reading.id, at.reason),
            ));
        }
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
        // A placeholder awaiting the user's value, or an assertion its value's
        // basis cannot decide, keeps the bench from passing.
        passed: errors == 0
            && bench.pending.is_empty()
            && readings
                .assertions
                .iter()
                .all(|a| a.status != AssertionStatus::Indeterminate),
        pending: bench.pending.len() as u32,
        counts: IssueCounts {
            errors,
            warnings,
            info,
        },
        issues,
        readings,
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
    let scene = bench.resolved_stages()?;
    let bench = scene.as_ref();
    let trace = center_trace(bench, system)?;
    let gaussian = envelope(bench, system, &trace)?;
    guide(bench, &gaussian)
}

pub(crate) const FORMATS: &[&str] = &[
    "svg",
    "pdf",
    "png",
    "dxf",
    "stl",
    "alignment_md",
    "layout3d",
];

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

fn reading_number(value: f64) -> String {
    if value == 0. {
        "0".into()
    } else if value.abs() < 1e-6 || value.abs() >= 1e9 {
        format!("{value:.6e}")
    } else {
        format!("{value:.9}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .into()
    }
}

fn interruption_text(at: &PathBreak) -> String {
    format!("{} / {} ({})", at.element, at.port, at.reason)
}

fn reading_cards(readings: &LabReadings) -> Vec<drawing::Card> {
    let stages = readings.stages.iter().map(|stage| {
        let mut lines = vec![
            format!("Delay stage {}", stage.id),
            format!("Position {} mm", reading_number(stage.position_mm)),
        ];
        if let Some(value) = stage.pzt_um {
            lines.push(format!("PZT {} µm", reading_number(value)));
        }
        drawing::Card {
            kind: "stage",
            id: stage.id.clone(),
            color: "#243b53".into(),
            lines,
        }
    });
    let branches = readings.branches.iter().map(|branch| {
        let mut lines = vec![branch.id.clone()];
        lines.extend(
            branch
                .stage_ids
                .iter()
                .map(|id| format!("Delay stage {id}")),
        );
        if let Some(at) = &branch.interruption {
            lines.push(format!("OPL unavailable · {}", interruption_text(at)));
        }
        for path in &branch.paths {
            if branch.paths.len() > 1 {
                lines.push(format!(
                    "{} · {} nm",
                    path.source,
                    reading_number(path.wavelength_nm)
                ));
            }
            let value = reading_number(path.opl_mm);
            lines.push(match &path.interruption {
                Some(at) => format!("Partial OPL {value} mm · {}", interruption_text(at)),
                None => format!("OPL {value} mm"),
            });
        }
        drawing::Card {
            kind: "branch",
            id: branch.id.clone(),
            color: branch.color.clone(),
            lines,
        }
    });
    let assertions = readings.assertions.iter().map(|result| {
        let unit = result.unit.symbol();
        let title = match result.kind {
            AssertionKind::EqualOpl => "ΔOPL",
            AssertionKind::Colinear => "Colinearity",
            AssertionKind::Dispersion => "Dispersion",
            AssertionKind::Parity => "Parity",
            AssertionKind::ImageRotation => "Image rotation",
        };
        let mut lines = vec![
            format!(
                "{title} · {} · {}",
                result.assertion,
                result.status.as_str()
            ),
            result.branches.join(" / "),
        ];
        match &result.value {
            Some(AssertionValue::Opl(value)) => {
                lines.push(format!("ΔOPL {} {unit}", reading_number(*value)));
            }
            Some(AssertionValue::Colinear(value)) => lines.push(format!(
                "Δr {} mm · Δθ {} mrad",
                reading_number(value.position_mm),
                reading_number(value.angle_mrad),
            )),
            Some(AssertionValue::Dispersion(value)) => lines.push(format!(
                "Δ {} {unit} @ {} nm",
                reading_number(value.difference),
                reading_number(value.wavelength_nm),
            )),
            Some(AssertionValue::Parity(value)) => lines.push(format!(
                "Reflections {} / {} · parity {}",
                value.reflections[0], value.reflections[1], value.parity,
            )),
            Some(AssertionValue::Rotation(value)) => {
                lines.push(format!("Rotation {} {unit}", reading_number(*value)))
            }
            None => {}
        }
        lines.push(format!(
            "Target {} {unit} · tolerance ±{} {unit}",
            reading_number(result.target),
            reading_number(result.tolerance),
        ));
        if let Some(residual) = result.residual {
            lines.push(format!("Residual {} {unit}", reading_number(residual)));
        }
        if !result.basis.is_plain() {
            lines.push(format!("Basis {}", result.basis.summary()));
        }
        lines.push(match &result.interruption {
            Some(at) => interruption_text(at),
            None => format!("{} / {}", result.at.element, result.at.port),
        });
        drawing::Card {
            kind: "assertion",
            id: result.assertion.clone(),
            color: match result.status {
                AssertionStatus::Pass => PASS_COLOR,
                AssertionStatus::Indeterminate => INDETERMINATE_COLOR,
                _ => FAIL_COLOR,
            }
            .into(),
            lines,
        }
    });
    let zones = readings.zones.iter().map(|zone| {
        let [[x0, y0], [x1, y1]] = zone.bounds_mm;
        let mut lines = vec![
            if zone.label.is_empty() {
                format!("Zone {}", zone.id)
            } else {
                format!("Zone {} · {}", zone.id, zone.label)
            },
            format!(
                "Area {} mm² · {} × {} mm",
                reading_number(zone.area_mm2),
                reading_number(x1 - x0),
                reading_number(y1 - y0)
            ),
        ];
        if !zone.members.is_empty() {
            lines.push(format!("Members {}", zone.members.join(", ")));
        }
        if !zone.out_of_bounds.is_empty() {
            lines.push(format!("Out of zone {}", zone.out_of_bounds.join(", ")));
        }
        let color = if zone.out_of_bounds.is_empty() {
            ZONE_COLOR
        } else {
            FAIL_COLOR
        };
        drawing::Card {
            kind: "zone",
            id: zone.id.clone(),
            color: color.into(),
            lines,
        }
    });
    let folds = readings.folds.iter().map(|fold| {
        let mut lines = vec![format!("Fold {}", fold.id)];
        if let Some(at) = &fold.interruption {
            lines.push(format!("Length unavailable · {}", interruption_text(at)));
        }
        for path in &fold.paths {
            if fold.paths.len() > 1 {
                lines.push(format!(
                    "{} · {} nm",
                    path.source,
                    reading_number(path.wavelength_nm)
                ));
            }
            let (length, opl) = (reading_number(path.length_mm), reading_number(path.opl_mm));
            lines.push(match &path.interruption {
                Some(at) => format!(
                    "Partial length {length} mm · OPL {opl} mm · {}",
                    interruption_text(at)
                ),
                None => format!("Length {length} mm · OPL {opl} mm"),
            });
        }
        let color =
            if fold.interruption.is_some() || fold.paths.iter().any(|p| p.interruption.is_some()) {
                FAIL_COLOR
            } else {
                FOLD_COLOR
            };
        drawing::Card {
            kind: "fold",
            id: fold.id.clone(),
            color: color.into(),
            lines,
        }
    });
    let relays = readings.relays.iter().map(|relay| {
        let mut title = format!("Relay {} · {}", relay.id, relay.branch);
        if let Some(nm) = relay.wavelength_nm {
            title += &format!(" · {} nm", reading_number(nm));
        }
        let mut lines = vec![title];
        // With a matrix, only the conjugate pupil can be undefined.
        let (matrix_break, pupil_break) = match (&relay.interruption, relay.abcd) {
            (Some(at), None) => (Some(at), None),
            (at, _) => (None, at.as_ref()),
        };
        if let Some(at) = matrix_break {
            lines.push(format!("ABCD unavailable · {}", interruption_text(at)));
        }
        if let Some([a, b, c, d]) = relay.abcd {
            lines.push(format!(
                "ABCD [{} {} mm; {} /mm {}]",
                reading_number(a),
                reading_number(b),
                reading_number(c),
                reading_number(d),
            ));
        }
        if !relay.powered.is_empty() {
            let powered: Vec<String> = relay
                .powered
                .iter()
                .map(|p| {
                    format!(
                        "{} f {} @ {}",
                        p.element,
                        reading_number(p.focal_mm),
                        reading_number(p.distance_mm)
                    )
                })
                .collect();
            let gap = relay.image_gap_mm.map_or_else(String::new, |g| {
                format!(" · image +{} mm", reading_number(g))
            });
            lines.push(format!("{} mm{gap}", powered.join(" → ")));
        }
        if let Some(stop) = &relay.pupil_stop {
            let target = relay.pupil_target.as_deref().unwrap_or("—");
            lines.push(match relay.pupil_image_mm {
                Some(z) => format!("Pupil {stop} → {} mm from {target}", reading_number(z)),
                None => format!("Pupil {stop} → at infinity from {target}"),
            });
        }
        if let Some(at) = pupil_break {
            let why = at.detail.as_deref().unwrap_or(&at.reason);
            lines.push(format!("Pupil unavailable · {why}"));
        }
        for check in &relay.checks {
            let kind = match check.kind {
                RelayCheckKind::Conjugate => "Conjugate B",
                RelayCheckKind::Magnification => "Magnification A",
                RelayCheckKind::Afocal => "Afocal C",
                RelayCheckKind::Strict4f => "Strict 4f",
            };
            let value = check.value.map_or_else(|| "—".into(), reading_number);
            lines.push(format!(
                "{kind} {value} · target {} ±{} · {}",
                reading_number(check.target),
                reading_number(check.tolerance),
                if check.passed { "pass" } else { "fail" },
            ));
            if let Some(at) = &check.interruption {
                lines.push(interruption_text(at));
            }
        }
        if !relay.basis.is_plain() {
            lines.push(format!("Basis {}", relay.basis.summary()));
        }
        let failed = relay.interruption.is_some() || relay.checks.iter().any(|c| !c.passed);
        let color = if failed {
            FAIL_COLOR
        } else if relay.checks.is_empty() {
            FOLD_COLOR
        } else {
            PASS_COLOR
        };
        drawing::Card {
            kind: "relay",
            id: relay.id.clone(),
            color: color.into(),
            lines,
        }
    });
    stages
        .chain(branches)
        .chain(assertions)
        .chain(zones)
        .chain(folds)
        .chain(relays)
        .collect()
}

const PASS_COLOR: &str = "#26734d";
const FAIL_COLOR: &str = "#b42318";
/// Slate for a zone's footprint card and a fold's or check-free relay's readings.
const ZONE_COLOR: &str = "#52606d";
const FOLD_COLOR: &str = "#334e68";

/// Amber for an undecided assertion, apart from the pass green and fail red.
const INDETERMINATE_COLOR: &str = "#b54708";

fn assertion_marks(readings: &LabReadings) -> Vec<drawing::AssertionMark> {
    readings
        .assertions
        .iter()
        .map(|result| drawing::AssertionMark {
            id: result.assertion.clone(),
            passed: matches!(result.status, AssertionStatus::Pass),
            text: result.status.as_str().into(),
            element: Some(result.at.element.clone()),
        })
        .collect()
}

/// Legend entries laid out in rows across `width`: (x, row, entry).
fn legend_rows(
    legend: &[drawing::LegendEntry],
    width: f64,
) -> Vec<(f64, usize, &drawing::LegendEntry)> {
    let mut placed = Vec::new();
    let (mut x, mut row) = (0., 0);
    for entry in legend {
        let span = 12. + entry.text.chars().count() as f64 * 2.4 + 6.;
        if x > 0. && x + span > width {
            x = 0.;
            row += 1;
        }
        placed.push((x, row, entry));
        x += span;
    }
    placed
}

fn svg(
    bench: &Bench,
    system: &System,
    grid: &Grid,
    gaussian: &Envelope<'_>,
    trace: &Trace,
    center: &Trace,
    version: Option<BlueprintRevision>,
) -> Result<String, String> {
    let [w, h] = bench.table_size;
    let plan = plan(bench, system, grid, trace, center, 4.);
    let legend = legend_rows(&plan.legend, w.max(120.));
    let legend_area = legend
        .last()
        .map_or(0., |(_, row, _)| 7. * (row + 1) as f64 + 4.);
    let cards = &plan.cards;
    let card_width = cards
        .iter()
        .flat_map(|card| &card.lines)
        .map(|line| line.chars().count())
        .max()
        .map_or(0., |chars| chars as f64 * 4. + 12.);
    let columns = (((w + 6.) / (card_width + 6.)) as usize).max(1);
    let card_height = |row: &[drawing::Card]| {
        8. + 6. * row.iter().map(|card| card.lines.len()).max().unwrap_or(0) as f64
    };
    let card_area: f64 = cards.chunks(columns).map(|row| card_height(row) + 6.).sum();
    let modal: Vec<(&str, Result<field::readout::ReadoutReading, String>)> = bench
        .elements
        .iter()
        .filter(|e| e.readout.is_some())
        .map(|e| {
            (
                e.id.as_str(),
                field::readout::readout(bench, system, &e.id, None),
            )
        })
        .collect();
    let modal_area: f64 = modal.iter().map(|(_, r)| modal_size(r)[1] + 6.).sum();
    let modal_width = modal
        .iter()
        .map(|(_, r)| modal_size(r)[0])
        .fold(0., f64::max);
    let page_width = w.max(card_width).max(modal_width) + 64.;
    let page_height = h + 108. + legend_area + card_area + modal_area;
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
    let hole_radius = plan.hole_radius;
    for ([col, row], &[x, y]) in grid.drawing_holes()?.zip(&plan.holes) {
        let [x, y] = pt([x, y, 0.]);
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
    let flat = |[x, y]: [f64; 2]| [x - grid.origin[0], h - (y - grid.origin[1])];
    let polyline = |points: &[[f64; 2]]| {
        points
            .iter()
            .map(|&p| {
                let [x, y] = flat(p);
                format!("{x:.4},{y:.4}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    out.push_str(r##"</g><g id="rulers" font-family="sans-serif" font-size="3" fill="#486581" text-anchor="middle">
<path stroke="#829ab1" stroke-width=".3" d=""##);
    for tick in &plan.rulers {
        let length = if tick.major { 3.5 } else { 2. };
        match tick.axis {
            "x" => write!(out, "M{:.4} 0V{:.4}", tick.value_mm, -length),
            _ => write!(out, "M{w} {:.4}H{:.4}", h - tick.value_mm, w + length),
        }
        .unwrap();
    }
    out.push_str("\"/>\n");
    for tick in plan.rulers.iter().filter(|t| t.major) {
        let value = reading_number(tick.value_mm);
        match tick.axis {
            "x" => writeln!(
                out,
                r##"<text x="{:.4}" y="-5">{value}</text>"##,
                tick.value_mm
            ),
            _ => writeln!(
                out,
                r##"<text x="{}" y="{:.4}" text-anchor="start">{value}</text>"##,
                w + 5.,
                h - tick.value_mm + 1.
            ),
        }
        .unwrap();
    }
    writeln!(
        out,
        r##"<text x="{}" y="-11" text-anchor="end">mm</text>"##,
        w
    )
    .unwrap();
    out.push_str(
        r##"</g><g id="zones" fill="none" stroke="#829ab1" stroke-width=".4" stroke-dasharray="3 2">
"##,
    );
    for zone in &plan.zones {
        writeln!(
            out,
            r##"<polygon data-zone="{}" points="{}"/>"##,
            xml(&zone.id),
            polyline(&zone.outline)
        )
        .unwrap();
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
    out.push_str("</g><g id=\"beams\" stroke-width=\".9\">\n");
    let mut open: Option<Option<&str>> = None;
    for beam in &plan.beams {
        if open != Some(beam.branch.as_deref()) {
            if open.is_some() {
                out.push_str("</g>\n");
            }
            match &beam.branch {
                Some(id) => writeln!(
                    out,
                    r##"<g data-branch="{}" stroke="{}">"##,
                    xml(id),
                    beam.color
                ),
                None => writeln!(
                    out,
                    r##"<g data-beam="unbranched" stroke="{}">"##,
                    beam.color
                ),
            }
            .unwrap();
            open = Some(beam.branch.as_deref());
        }
        let [x1, y1] = flat(beam.from);
        let [x2, y2] = flat(beam.to);
        let dash = dasharray(&beam.dash);
        writeln!(
            out,
            r##"<line x1="{x1:.4}" y1="{y1:.4}" x2="{x2:.4}" y2="{y2:.4}"{dash}/>"##
        )
        .unwrap();
        if let Some(arrow) = &beam.arrow {
            writeln!(
                out,
                r##"<path d="M{}Z" fill="{}" stroke="none"/>"##,
                polyline(arrow).replace(' ', "L"),
                beam.color
            )
            .unwrap();
        }
    }
    if open.is_some() {
        out.push_str("</g>\n");
    }
    out.push_str(
        r##"</g></g><g id="stages" fill="none" stroke="#243b53" stroke-width=".4">
"##,
    );
    for stage in &plan.stages {
        let [a, b] = stage.travel.map(flat);
        writeln!(
            out,
            r##"<g data-stage="{}"><polygon points="{}" stroke-dasharray="1.5 1"/><path d="M{:.4} {:.4}L{:.4} {:.4}" stroke-width=".6"/><circle cx="{:.4}" cy="{:.4}" r=".8"/><circle cx="{:.4}" cy="{:.4}" r=".8"/></g>"##,
            xml(&stage.id), polyline(&stage.outline), a[0], a[1], b[0], b[1], a[0], a[1], b[0], b[1],
        ).unwrap();
    }
    out.push_str(
        r##"</g><g id="symbols" stroke="#102a43" stroke-width=".45" stroke-linejoin="round">
"##,
    );
    for symbol in &plan.symbols {
        writeln!(
            out,
            r##"<g data-element="{}" data-symbol="{}">"##,
            xml(&symbol.id),
            symbol.kind
        )
        .unwrap();
        for stroke in &symbol.strokes {
            let style = match stroke.role {
                drawing::Role::Body => r##"fill="#fff" stroke="#102a43""##,
                drawing::Role::Fill => r##"fill="#334e68" stroke="#102a43""##,
                drawing::Role::Face => r##"fill="none" stroke="#102a43" stroke-width="1""##,
                drawing::Role::Hatch => r##"fill="none" stroke="#102a43" stroke-width=".25""##,
            };
            let tag = if stroke.closed { "polygon" } else { "polyline" };
            writeln!(
                out,
                r##"<{tag} data-role="{}" points="{}" {style}/>"##,
                serde_json::to_value(stroke.role)
                    .unwrap()
                    .as_str()
                    .unwrap_or_default(),
                polyline(&stroke.points)
            )
            .unwrap();
        }
        out.push_str("</g>\n");
    }
    out.push_str(
        r##"</g><g id="periscopes" fill="none" stroke="#102a43" stroke-width=".45">
"##,
    );
    for mark in &plan.periscopes {
        let [x, y] = flat(mark.at);
        writeln!(
            out,
            r##"<g data-periscope="{}" data-rise="{}"><circle cx="{x:.4}" cy="{y:.4}" r="3"/><path d="M{:.4} {y:.4}H{:.4}M{x:.4} {:.4}V{:.4}"/></g>"##,
            xml(&mark.id), reading_number(mark.rise_mm), x - 3., x + 3., y - 3., y + 3.,
        ).unwrap();
    }
    writeln!(
        out,
        r##"</g><g id="labels" font-family="sans-serif" font-size="{}" fill="#102a43">"##,
        plan.text_mm
    )
    .unwrap();
    for label in &plan.labels {
        let [x, y] = flat(label.corner);
        if label.leader {
            let near = [
                label.anchor[0].clamp(label.corner[0], label.corner[0] + label.size[0]),
                label.anchor[1].clamp(label.corner[1], label.corner[1] + label.size[1]),
            ];
            let [ax, ay] = flat(label.anchor);
            let [bx, by] = flat(near);
            writeln!(out, r##"<line x1="{ax:.4}" y1="{ay:.4}" x2="{bx:.4}" y2="{by:.4}" stroke="#829ab1" stroke-width=".3"/>"##).unwrap();
        }
        let fill = match label.kind {
            "annotation" => r##" fill="#7a4b00" font-style="italic""##,
            "assertion"
                if plan
                    .assertions
                    .iter()
                    .any(|a| a.id == label.target && a.passed) =>
            {
                r##" fill="#26734d""##
            }
            "assertion"
                if plan
                    .assertions
                    .iter()
                    .any(|a| a.id == label.target && a.text == "indeterminate") =>
            {
                r##" fill="#b54708""##
            }
            "assertion" => r##" fill="#b42318""##,
            "zone" | "stage" => r##" fill="#486581""##,
            _ => "",
        };
        writeln!(
            out,
            r##"<text data-label="{}" data-target="{}" x="{:.4}" y="{:.4}"{fill}>{}</text>"##,
            label.kind,
            xml(&label.target),
            x + 0.3 * plan.text_mm,
            y - 0.35 * plan.text_mm,
            xml(&label.text),
        )
        .unwrap();
    }
    let title_y = h + 20.;
    writeln!(
        out,
        r##"</g><g id="title-block" font-family="sans-serif" fill="#102a43">
<path d="M 0 {title_y} H {w}" stroke="#829ab1"/>
<text x="0" y="{}" font-size="7">{}</text>
<text x="0" y="{}" font-size="4">Hissabeth · {w} × {h} mm · {} mm hole pitch · 1 unit = 1 mm</text>"##,
        title_y + 12.,
        xml(&bench.name),
        title_y + 22.,
        grid.pitch
    )
    .unwrap();
    if let Some(version) = &version {
        let input = if version.saved {
            "saved bench"
        } else {
            "bench draft"
        };
        writeln!(
            out,
            r##"<text x="0" y="{}" font-size="4">Revision {} · {input}</text>"##,
            title_y + 29.,
            version.revision
        )
        .unwrap();
    }
    let mut y = title_y + 36.;
    out.push_str(
        "</g><g id=\"legend\" font-family=\"sans-serif\" font-size=\"4\" fill=\"#102a43\">\n",
    );
    for (x, row, entry) in &legend {
        let line_y = y + 7. * *row as f64 + 3.;
        let swatch = if entry.kind == "symbol" {
            format!(
                r##"<rect x="{x}" y="{}" width="4" height="4" fill="#d9e2ec" stroke="{}" stroke-width=".4"/>"##,
                line_y - 3.5,
                entry.color
            )
        } else {
            let dash = dasharray(&entry.dash);
            format!(
                r##"<line x1="{x}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width=".9"{dash}/>"##,
                line_y - 1.5,
                x + 9.,
                line_y - 1.5,
                entry.color
            )
        };
        writeln!(
            out,
            r##"<g data-legend="{}" data-key="{}">{swatch}<text x="{}" y="{line_y}">{}</text></g>"##,
            entry.kind, xml(&entry.key), x + 12., xml(&entry.text),
        ).unwrap();
    }
    y += legend_area;
    out.push_str(r##"</g><g id="lab-readings" font-family="sans-serif" font-size="4">"##);
    for row in cards.chunks(columns) {
        let height = card_height(row);
        for (column, card) in row.iter().enumerate() {
            let x = column as f64 * (card_width + 6.);
            let color = &card.color;
            writeln!(
                out,
                r##"<g data-kind="{}" data-id="{}" transform="translate({x} {y})" fill="{color}">
<rect width="{card_width}" height="{height}" rx="2" fill="#fff" stroke="{color}" stroke-width=".5"/>"##,
                card.kind, xml(&card.id),
            ).unwrap();
            for (index, line) in card.lines.iter().enumerate() {
                let line_y = 7 + 6 * index;
                writeln!(out, r##"<text x="5" y="{line_y}">{}</text>"##, xml(line)).unwrap();
            }
            out.push_str("</g>\n");
        }
        y += height + 6.;
    }
    out.push_str("</g><g id=\"modal-readout\" font-family=\"sans-serif\" font-size=\"4\" fill=\"#102a43\">\n");
    let revision = version
        .as_ref()
        .map_or_else(String::new, |v| format!(" · revision {}", v.revision));
    for (id, reading) in &modal {
        modal_svg(&mut out, id, reading, &revision, y);
        y += modal_size(reading)[1] + 6.;
    }
    out.push_str("</g></svg>");
    Ok(out)
}

/// Heatmap cell size (mm) of the modal readout table: ports × modes.
const MODAL_CELL: [f64; 2] = [26., 7.];

fn modal_size(reading: &Result<field::readout::ReadoutReading, String>) -> [f64; 2] {
    match reading {
        Ok(r) => [
            (22. + MODAL_CELL[0] * r.ports.first().map_or(0, |p| p.modes.len()) as f64).max(140.),
            14. + MODAL_CELL[1] * (r.ports.len() + 1) as f64 + 6. * r.signals.len() as f64,
        ],
        Err(e) => [e.chars().count() as f64 * 2.2 + 40., 14.],
    }
}

/// One detector's modal readout: a ports × modes heatmap of mode intensity
/// (shaded relative to the table's largest intensity) and its balanced signals.
fn modal_svg(
    out: &mut String,
    id: &str,
    reading: &Result<field::readout::ReadoutReading, String>,
    revision: &str,
    y: f64,
) {
    // Readings below 1e-9 (of the 1-unit source scale) are numerical zeros.
    let number = |v: f64| match v.abs() {
        a if a < 1e-9 => "0".to_string(),
        a if a < 1e-3 => format!("{v:.1e}"),
        _ => format!("{v:.3}"),
    };
    writeln!(
        out,
        r##"<g data-detector="{}" transform="translate(0 {y})">"##,
        xml(id)
    )
    .unwrap();
    let r = match reading {
        Ok(r) => r,
        Err(e) => {
            writeln!(out, r##"<text y="6" font-size="5">Modal readout {}{revision}</text><text y="12" fill="#b42318">{}</text></g>"##, xml(id), xml(e)).unwrap();
            return;
        }
    };
    let modes = r.ports.first().map_or(&[][..], |p| &p.modes[..]);
    writeln!(
        out,
        r##"<text y="6" font-size="5">Modal readout {} · {} ports × {} modes · {} normalization{revision}</text>"##,
        xml(id),
        r.ports.len(),
        modes.len(),
        match r.normalization {
            bench::ModeNormalization::Power => "power",
            bench::ModeNormalization::Amplitude => "amplitude",
        }
    )
    .unwrap();
    let [cw, ch] = MODAL_CELL;
    let top = 10.;
    for (j, mode) in modes.iter().enumerate() {
        writeln!(
            out,
            r##"<text x="{}" y="{}" text-anchor="middle">{}</text>"##,
            22. + cw * (j as f64 + 0.5),
            top + 5.,
            xml(&mode.index)
        )
        .unwrap();
    }
    let peak = r
        .ports
        .iter()
        .flat_map(|p| &p.modes)
        .map(|m| m.intensity)
        .fold(0., f64::max);
    for (i, port) in r.ports.iter().enumerate() {
        let row = top + ch * (i + 1) as f64;
        writeln!(
            out,
            r##"<text x="0" y="{}">{}</text>"##,
            row + 5.,
            xml(&port.id)
        )
        .unwrap();
        for (j, mode) in port.modes.iter().enumerate() {
            let level = if peak > 0. {
                (mode.intensity / peak).clamp(0., 1.)
            } else {
                0.
            };
            let shade = |c: f64| (255. + (c - 255.) * level).round() as u8;
            let (x, ink) = (
                22. + cw * j as f64,
                if level > 0.5 { "#fff" } else { "#102a43" },
            );
            writeln!(
                out,
                r##"<rect x="{x}" y="{row}" width="{cw}" height="{ch}" fill="#{:02x}{:02x}{:02x}" stroke="#bcccdc" stroke-width=".3" data-port="{}" data-mode="{}"/><text x="{}" y="{}" text-anchor="middle" fill="{ink}">{}</text>"##,
                shade(0.),
                shade(124.),
                shade(145.),
                xml(&port.id),
                xml(&mode.index),
                x + cw / 2.,
                row + 5.,
                number(mode.intensity)
            )
            .unwrap();
        }
    }
    let base = top + ch * (r.ports.len() + 1) as f64;
    for (k, signal) in r.signals.iter().enumerate() {
        writeln!(
            out,
            r##"<text y="{}">{}: real {} · imag {}</text>"##,
            base + 6. * (k + 1) as f64,
            xml(&signal.pair),
            number(signal.real),
            number(signal.imag)
        )
        .unwrap();
    }
    out.push_str("</g>\n");
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
            add(center, bench::rotate(p, e.rotation()))
        });
        let slab = bench::rotate([4., 0., 0.], e.rotation());
        prism(&mut out, &rim, &rim.map(|p| add(p, slab)))?;
    }
    out.push_str("endsolid hissabeth_bench\n");
    Ok(out)
}

fn markdown_cell(text: &str) -> String {
    xml(text)
        .replace('\\', "&#92;")
        .replace('|', "&#124;")
        .replace(['\n', '\r'], " ")
}

fn alignment_md(bench: &Bench, system: &System, gaussian: &Envelope<'_>) -> Result<String, String> {
    let guide = guide(bench, gaussian)?;
    let check = lab_report(bench, system, &bench.trace_resolved(system)?, gaussian)?;
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
    /// The file: text for svg, dxf, stl and alignment_md; standard base64 of
    /// the bytes for pdf and png.
    content: String,
    /// "base64" when `content` encodes a binary file.
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<&'static str>,
    /// The bytes of a binary file, handed to artifacts without re-decoding.
    #[serde(skip)]
    #[schemars(skip)]
    binary: Option<Vec<u8>>,
}

impl Blueprint {
    /// The file name, MIME type and bytes of the drawing.
    pub(crate) fn into_file(self) -> (String, &'static str, Vec<u8>) {
        let bytes = self.binary.unwrap_or_else(|| self.content.into_bytes());
        (self.filename, self.mime, bytes)
    }
}

pub(crate) struct BlueprintRevision {
    pub revision: u64,
    pub saved: bool,
}

/// The plan-view drawing shared by the SVG blueprint and the GUI 2D view, with
/// text sized at `text_mm` table millimetres.
pub(crate) fn blueprint_drawing(
    bench: &Bench,
    system: &System,
    text_mm: f64,
) -> Result<drawing::Drawing, String> {
    let scene = bench.resolved_stages()?;
    let bench = scene.as_ref();
    let trace = center_trace(bench, system)?;
    let physical = physical_trace(bench, system)?;
    Ok(plan(
        bench,
        system,
        &Grid::new(bench)?,
        physical.as_ref().unwrap_or(&trace),
        &trace,
        text_mm,
    ))
}

/// The full sampled trace that branch, assertion, fold and relay readings
/// need, when the bench declares any of them; drawings fall back to the
/// centre-ray trace otherwise.
fn physical_trace(bench: &Bench, system: &System) -> Result<Option<Trace>, String> {
    let needed = !bench.branches.is_empty() || !bench.folds.is_empty();
    needed.then(|| bench.trace_resolved(system)).transpose()
}

/// The shared drawing with the breadboard holes and the result cards of `readings_trace`.
fn plan(
    bench: &Bench,
    system: &System,
    grid: &Grid,
    readings_trace: &Trace,
    center: &Trace,
    text_mm: f64,
) -> drawing::Drawing {
    let readings = lab_readings(bench, system, readings_trace);
    let mut plan = drawing::drawing(bench, center, assertion_marks(&readings), text_mm);
    plan.cards = reading_cards(&readings);
    if let Ok(holes) = grid.drawing_holes() {
        plan.holes = holes
            .map(|hole| {
                let [x, y, _] = grid.point(hole);
                [x, y]
            })
            .collect();
    }
    plan
}

fn dasharray(dash: &[f64]) -> String {
    if dash.is_empty() {
        return String::new();
    }
    let pattern: Vec<String> = dash.iter().map(|d| reading_number(*d)).collect();
    format!(r#" stroke-dasharray="{}""#, pattern.join(" "))
}

pub fn export_blueprint(bench: &Bench, system: &System, format: &str) -> Result<Value, String> {
    serde_json::to_value(export_blueprint_report(bench, system, format, None)?)
        .map_err(|e| e.to_string())
}
pub(crate) fn export_blueprint_report(
    bench: &Bench,
    system: &System,
    format: &str,
    version: Option<BlueprintRevision>,
) -> Result<Blueprint, String> {
    export_blueprint_file(bench, system, format, None, None, version)
}

/// A blueprint file. `px_per_mm` sets the PNG resolution (default
/// [`blueprint_render::DEFAULT_PX_PER_MM`]) and is refused for other formats.
/// `layout3d` options (when they ask for it) select the shaded 3-D layout for
/// the pdf and png formats, set up format layout3d (else the defaults) and
/// are refused for others.
pub(crate) fn export_blueprint_file(
    bench: &Bench,
    system: &System,
    format: &str,
    px_per_mm: Option<f64>,
    layout3d: Option<&layout3d::Layout3dOptions>,
    version: Option<BlueprintRevision>,
) -> Result<Blueprint, String> {
    if !FORMATS.contains(&format) {
        return Err(format!(
            "Unknown blueprint format {format:?}. Choose {}",
            FORMATS.join(", ")
        ));
    }
    if px_per_mm.is_some() && format != "png" {
        return Err(format!("px_per_mm applies only to png, not {format}"));
    }
    if layout3d.is_some() && !matches!(format, "layout3d" | "pdf" | "png") {
        return Err(format!(
            "layout3d options apply only to layout3d, pdf and png, not {format}"
        ));
    }
    let stem = if layout3d.is_some() || format == "layout3d" {
        "hissabeth-layout3d"
    } else {
        "hissabeth-bench"
    };
    if format == "layout3d" {
        let options = layout3d.cloned().unwrap_or_default();
        return Ok(Blueprint {
            format: format.to_owned(),
            filename: format!("{stem}.svg"),
            mime: "image/svg+xml",
            content: layout3d::render(bench, system, &options, version)?.svg(),
            encoding: None,
            binary: None,
        });
    }
    if matches!(format, "pdf" | "png") {
        let source = if layout3d.is_some() {
            "layout3d"
        } else {
            "svg"
        };
        let svg = export_blueprint_file(bench, system, source, None, layout3d, version)?.content;
        let (bytes, mime) = if format == "pdf" {
            (blueprint_render::pdf(&svg, true)?, "application/pdf")
        } else {
            (
                blueprint_render::png(
                    &svg,
                    px_per_mm.unwrap_or(blueprint_render::DEFAULT_PX_PER_MM),
                )?,
                "image/png",
            )
        };
        return Ok(Blueprint {
            format: format.to_owned(),
            filename: format!("{stem}.{format}"),
            mime,
            content: blueprint_render::base64(&bytes),
            encoding: Some("base64"),
            binary: Some(bytes),
        });
    }
    let scene = bench.resolved_stages()?;
    let bench = scene.as_ref();
    let (extension, mime, content) = if format == "stl" {
        bench.validate()?;
        ("stl", "model/stl", stl(bench)?)
    } else {
        let trace = center_trace(bench, system)?;
        let gaussian = envelope(bench, system, &trace)?;
        match format {
            "svg" => {
                let physical = physical_trace(bench, system)?;
                let content = svg(
                    bench,
                    system,
                    &Grid::new(bench)?,
                    &gaussian,
                    physical.as_ref().unwrap_or(&trace),
                    &trace,
                    version,
                )?;
                ("svg", "image/svg+xml", content)
            }
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
        encoding: None,
        binary: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optics::bench::{
        Annotation, AnnotationTarget, BranchStyle, HeightLayer, PendingField, PendingInput,
        Periscope, Region, RegionShape, Relay, RelayCheck, RelayCheckKind, Zone, ZoneRect, rotate,
    };
    #[test]
    fn unexpected_hit_on_a_region_plate_is_reported_not_an_internal_error() {
        let mut bench = template("three_arm").unwrap();
        for c in &mut bench.connections {
            if c.from == "S1" {
                c.to = "D_C".into();
            }
        }
        let system = System::default();
        gaussian_envelope(&bench, &system).unwrap();
        let check = lab_check(&bench, &system).unwrap();
        assert!(
            check["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["code"] == "obstruction" && x["severity"] == "error"),
            "{}",
            check["issues"]
        );
    }
    #[test]
    fn folds_alone_are_drawn_from_the_full_trace() {
        // Every three-arm hole is lit only by the sampled rings, not the centre ray.
        let system = System::default();
        let mut bench = template("three_arm").unwrap();
        bench.folds = std::mem::take(&mut bench.branches);
        bench.assertions.clear();
        bench.relays.clear();
        let check = lab_check(&bench, &system).unwrap();
        let folds = check["folds"].as_array().unwrap();
        assert_eq!(folds.len(), 3, "{check}");
        let svg = export_blueprint_report(&bench, &system, "svg", None)
            .unwrap()
            .content;
        for fold in folds {
            let id = fold["id"].as_str().unwrap();
            assert!(
                fold.get("break").is_none() && fold["paths"][0].get("break").is_none(),
                "{fold}"
            );
            let shown = card(&bench, &system, "fold", id);
            assert_eq!(shown.color, FOLD_COLOR, "{id}: {:?}", shown.lines);
            assert!(
                !shown.lines.iter().any(|l| l.starts_with("Partial")),
                "{:?}",
                shown.lines
            );
            let at = svg
                .find(&format!(r#"data-kind="fold" data-id="{id}""#))
                .unwrap();
            let tag = &svg[at..at + svg[at..].find('>').unwrap()];
            assert!(tag.contains(&format!(r#"fill="{FOLD_COLOR}""#)), "{tag}");
        }
    }
    #[test]
    fn sub_aperture_keeps_wavefront_curvature_and_truncates_only_wider_beams() {
        // Single C hole on the three-arm plate; the source focuses 300 mm ahead.
        let system = System::default();
        let n = system.material_index("AIR", 532.).unwrap();
        let lambda = 532e-6;
        let setup = |hole: f64| {
            let mut bench = template("three_arm").unwrap();
            bench.branches.clear();
            bench.assertions.clear();
            bench
                .connections
                .retain(|c| c.from != "AP" || c.port == "C");
            bench
                .elements
                .retain(|e| !matches!(e.id.as_str(), "D_L" | "D_R"));
            let source = &mut bench.sources[0];
            source.waist = Some(0.2);
            source.waist_offset = Some(300.);
            source.rings = 0;
            source.wavelength = 532.;
            let plate = bench.elements.iter_mut().find(|e| e.id == "AP").unwrap();
            plate.regions.retain(|r| r.port == "C");
            if let RegionShape::Circle { radius, .. } = &mut plate.regions[0].shape {
                *radius = hole;
            }
            bench
        };
        let after_plate = |bench: &Bench| {
            let envelope = gaussian_envelope(bench, &system).unwrap();
            envelope["segments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["branch"].as_str().unwrap().contains("AP:C"))
                .map(|s| (s["w_from"].as_f64().unwrap(), s["w_to"].as_f64().unwrap()))
                .unwrap()
        };
        let bench = setup(14.);
        let source = &bench.sources[0];
        let plate = bench.elements.iter().find(|e| e.id == "AP").unwrap();
        let detector = bench.elements.iter().find(|e| e.id == "D_C").unwrap();
        let (z_plate, z_det) = (
            plate.position[0] - source.position[0],
            detector.position[0] - source.position[0],
        );
        let z_r = PI * n * 0.04 / lambda;
        let w = |z: f64| 0.2 * (1. + ((z - 300.) / z_r).powi(2)).sqrt();
        // Wide hole: the beam continues as if the plate were absent.
        let (w_from, w_to) = after_plate(&bench);
        close(w_from, w(z_plate));
        close(w_to, w(z_det));
        // Narrow hole: radius clamps to a, curvature 1/R of the incoming wave is kept.
        let a = 0.5 * w(z_plate);
        let (w_from, w_to) = after_plate(&setup(a));
        close(w_from, a);
        let d = z_plate - 300.;
        let curvature = d / (d * d + z_r * z_r);
        let (re, im) = (curvature, -lambda / (PI * n * a * a));
        let norm = re * re + im * im;
        let q = (re / norm + (z_det - z_plate), -im / norm);
        let expected = (q.0.hypot(q.1)) * (lambda / PI / n / q.1).sqrt();
        close(w_to, expected);
    }

    fn close(got: f64, expected: f64) {
        assert!((got - expected).abs() < 1e-8, "{got} vs {expected}");
    }

    #[test]
    fn svg_prints_the_modal_readout_heatmap_with_the_revision() {
        let system = System::default();
        let bench = template("modal_hybrid").unwrap();
        bench.validate().unwrap();
        let drawing = export_blueprint_report(
            &bench,
            &system,
            "svg",
            Some(BlueprintRevision {
                revision: 17,
                saved: false,
            }),
        )
        .unwrap()
        .content;
        let (_, modal) = drawing.split_once(r#"<g id="modal-readout""#).unwrap();
        let (block, _) = modal
            .split_once(r#"<g data-detector="CAM""#)
            .unwrap()
            .1
            .split_once("</g>")
            .unwrap();
        assert!(
            block.contains(
                "Modal readout CAM · 4 ports × 4 modes · power normalization · revision 17"
            ),
            "{block}"
        );
        assert_eq!(block.matches("data-port=").count(), 16);
        let reading = field::readout::readout(&bench, &system, "CAM", None).unwrap();
        let cell = |port: &str, mode: &str| {
            let opening = format!(r#"data-port="{port}" data-mode="{mode}"/>"#);
            let (_, rest) = block.split_once(&opening).unwrap();
            rest.split_once('>')
                .unwrap()
                .1
                .split_once("</text>")
                .unwrap()
                .0
                .to_string()
        };
        assert_eq!(
            cell("Q+", "HG0,0"),
            format!("{:.3}", reading.ports[2].modes[0].intensity)
        );
        let signal = &reading.signals[0];
        // At zero signal phase the quadrature reading vanishes.
        assert!(signal.imag.abs() < 1e-9 && signal.real > 0.2);
        assert!(
            block.contains(&format!("iq: real {:.3} · imag 0</text>", signal.real)),
            "{block}"
        );
        assert!(block.contains(r##"data-port="Q+" data-mode="HG1,0"/><text x="61" y="36" text-anchor="middle" fill="#102a43">0</text>"##));
    }

    #[test]
    fn svg_prints_delay_readings_and_assertion_status_at_the_traced_locations() {
        fn group<'a>(svg: &'a str, attributes: &str) -> (&'a str, Vec<&'a str>) {
            let opening = format!("<g {attributes}");
            let (_, group) = svg.split_once(&opening).unwrap();
            let (group, _) = group.split_once("</g>").unwrap();
            let (attributes, body) = group.split_once('>').unwrap();
            let text = body
                .split("<text ")
                .skip(1)
                .map(|node| {
                    node.split_once('>')
                        .unwrap()
                        .1
                        .split_once("</text>")
                        .unwrap()
                        .0
                })
                .collect();
            (attributes, text)
        }

        let system = System::default();
        let mut bench = template("delay_line").unwrap();
        bench.stages[0].position_mm = 2.5;
        bench.stages[0].pzt.as_mut().unwrap().value_um = 1.;
        let drawing = export_blueprint_report(
            &bench,
            &system,
            "svg",
            Some(BlueprintRevision {
                revision: 17,
                saved: false,
            }),
        )
        .unwrap()
        .content;
        assert!(drawing.contains(">Revision 17 · bench draft</text>"));

        let (_, stage) = group(&drawing, r#"data-kind="stage" data-id="delay""#);
        assert_eq!(stage, ["Delay stage delay", "Position 2.5 mm", "PZT 1 µm"]);
        let (_, reference) = group(&drawing, r#"data-kind="branch" data-id="arm_reference""#);
        let (arm_style, delay) = group(&drawing, r#"data-kind="branch" data-id="arm_delay""#);
        let opl = |reading: &str| {
            reading
                .strip_prefix("OPL ")
                .unwrap()
                .strip_suffix(" mm")
                .unwrap()
                .parse::<f64>()
                .unwrap()
        };
        // Each of the two folded legs grows by the total displacement 2.501 mm.
        close(opl(reference[1]), 125.);
        close(opl(delay[2]), 730.002);
        let color = arm_style
            .split_once("fill=\"")
            .unwrap()
            .1
            .split('"')
            .next()
            .unwrap();
        let (highlight, _) = group(&drawing, r#"data-branch="arm_delay""#);
        assert_eq!(highlight, format!(" stroke=\"{color}\""));
        let arm_geometry = drawing
            .split_once(r#"data-branch="arm_delay""#)
            .unwrap()
            .1
            .split_once("</g>")
            .unwrap()
            .0;
        assert_eq!(arm_geometry.matches("<line ").count(), 3);
        assert!(arm_geometry.contains(r#"x2="552.5010""#));

        let (failed_style, failed) =
            group(&drawing, r#"data-kind="assertion" data-id="delay_opl""#);
        assert!(failed_style.contains(r##"fill="#b42318""##));
        assert!(failed.contains(&"ΔOPL · delay_opl · fail"));
        close(
            failed[2]
                .strip_prefix("ΔOPL ")
                .unwrap()
                .strip_suffix(" mm")
                .unwrap()
                .parse()
                .unwrap(),
            605.002,
        );
        assert!(failed.contains(&"Target 600 mm · tolerance ±0.001 mm"));
        assert!(failed.contains(&"BS2 / in_a"));
        let (_, colinear) = group(
            &drawing,
            r#"data-kind="assertion" data-id="combiner_angle""#,
        );
        assert!(colinear.contains(&"Colinearity · combiner_angle · pass"));
        let (position, angle) = colinear[2]
            .strip_prefix("Δr ")
            .unwrap()
            .split_once(" mm · Δθ ")
            .unwrap();
        close(position.parse().unwrap(), 0.);
        close(angle.strip_suffix(" mrad").unwrap().parse().unwrap(), 0.);

        bench
            .elements
            .push(mount_element("STOP", DUMP, [400., 200., 75.], [1., 0., 0.]));
        let blocked = export_blueprint_report(&bench, &system, "svg", None)
            .unwrap()
            .content;
        let (style, unavailable) = group(&blocked, r#"data-kind="assertion" data-id="delay_opl""#);
        assert!(style.contains(r##"fill="#b42318""##));
        assert!(unavailable.contains(&"ΔOPL · delay_opl · unreachable"));
        assert!(unavailable.contains(&"STOP / in (unexpected_hit)"));
    }

    #[test]
    fn delay_stage_opl_follows_parallel_tilted_and_pzt_motion() {
        let system = System::default();
        let mut bench = template("delay_line").unwrap();
        let trace = bench.trace(&system).unwrap();
        let baseline = &trace.branches[1].paths[0];
        assert_eq!(trace.branches[1].id, "arm_delay");
        assert!((baseline.opl_mm - 725.).abs() <= 1e-9);
        assert!((trace.branches[0].paths[0].opl_mm - 125.).abs() <= 1e-9);
        let theta = std::f64::consts::FRAC_PI_6;
        for (angle, position, pzt, expected_delay) in [
            (0., 2.5, 0., 5.),
            (theta, 2.5, 0., 5. * theta.cos()),
            (0., 0., 1., 0.002),
            (0., -2.5, 1., -4.998),
        ] {
            let stage = &mut bench.stages[0];
            // XZ tilt leaves the XY fold planes unchanged except for their X shift.
            stage.axis = [angle.cos(), 0., angle.sin()];
            stage.position_mm = position;
            stage.pzt.as_mut().unwrap().value_um = pzt;
            let trace = bench.trace(&system).unwrap();
            let delayed = &trace.branches[1].paths[0];
            let reference = &trace.branches[0].paths[0];
            assert!(delayed.nominal_match && reference.nominal_match);
            let delta = delayed.opl_mm - baseline.opl_mm;
            assert!(
                (delta - expected_delay).abs() <= 1e-9,
                "{delta} vs {expected_delay}"
            );
            assert!((reference.opl_mm - 125.).abs() <= 1e-9);
            let a = trace.stage_poses["M3"];
            let b = trace.stage_poses["M4"];
            assert!(norm(sub(sub(b.position, a.position), [0., 125., 0.])) <= 1e-9);
            assert_eq!(a.rotation, bench.stages[0].reference_pose["M3"].rotation);
            assert_eq!(b.rotation, bench.stages[0].reference_pose["M4"].rotation);
        }
        let aimed = aim(&bench, true, Some(85.), &system).unwrap();
        assert!(aimed.ok);
        assert_eq!(aimed.bench.elements[1].position[2], 75.);
        assert_eq!(aimed.bench.elements[1].position[0], 547.501);
        assert_eq!(aimed.bench.elements[0].position[2], 85.);
        assert!(
            !aimed
                .adjusted_elements
                .iter()
                .any(|id| bench.stages[0].members.contains(id))
        );
        let conflicted = aim(&bench, true, Some(115.), &system).unwrap();
        assert!(!conflicted.ok);
        assert_eq!(conflicted.bench.elements[0].position[2], 75.);
        assert_eq!(
            conflicted.bench.elements[1].position,
            bench.elements[1].position
        );
    }

    #[test]
    fn mach_zehnder_arms_measure_the_actual_fold_and_mirror_translation() {
        let system = System::default();
        let mut bench = template("mach_zehnder").unwrap();
        let expected_ports = [
            [
                ("BS1", "reflected"),
                ("M1", "in"),
                ("M1", "reflected"),
                ("BS2", "in_a"),
            ],
            [
                ("BS1", "transmitted"),
                ("M2", "in"),
                ("M2", "reflected"),
                ("BS2", "in_b"),
            ],
        ];
        let trace = bench.trace(&system).unwrap();
        for (report, expected) in trace.branches.iter().zip(expected_ports) {
            let [arm] = report.paths.as_slice() else {
                panic!("expected one axial path")
            };
            assert!(arm.nominal_match);
            assert_eq!(
                arm.ports,
                expected.map(|(element, port)| Port::new(element, port))
            );
            assert!((arm.length_mm - 400.).abs() <= 1e-9);
            assert!((arm.opl_mm - 400.).abs() <= 1e-9);
            assert_eq!(arm.wavelength_nm, 532.);
        }
        assert_eq!(trace.branches.len(), 2);
        assert_eq!(
            lab_check(&bench, &system).unwrap()["assertions"][0]["status"],
            "pass"
        );

        let shift = 2.5;
        bench.elements[1].position[1] += shift;
        // The mirror keeps its 45° fold. The first leg grows by the vertical shift;
        // the parallel outgoing ray crosses the diagonal combiner farther along X by the same amount.
        let moved = bench.trace(&system).unwrap();
        for (report, expected) in moved.branches.iter().zip([400. + 2. * shift, 400.]) {
            let arm = &report.paths[0];
            assert!(arm.nominal_match);
            assert!((arm.length_mm - expected).abs() <= 1e-9);
            assert!((arm.opl_mm - expected).abs() <= 1e-9);
        }
        bench.assertions[0].unit = AssertionUnit::Um;
        let check = lab_check(&bench, &system).unwrap();
        let result = &check["assertions"][0];
        assert_eq!(result["status"], "fail");
        assert_eq!(result["unit"], "um");
        assert!((result["value"].as_f64().unwrap() - 5000.).abs() <= 1e-6);
        assert_eq!(result["residual"], result["value"]);
        assert_eq!(check["passed"], false);
    }

    #[test]
    fn thin_lens_glass_bounds_the_opl_difference_and_bounds_decide_one_side_only() {
        let system = System::default();
        let base = template("mach_zehnder").unwrap();
        let check = |bench: &Bench, target: f64| {
            let mut b = bench.clone();
            b.assertions = vec![Assertion {
                target,
                ..base.assertions[0].clone()
            }];
            lab_check(&b, &system).unwrap()
        };
        let lens = |id: &str, at: V3, normal: V3| {
            let mut e = mount_element(id, LENS, at, normal);
            e.focal_length = 1e5;
            e
        };
        let in_a = || lens("LA", [250., 300., 75.], [0., 1., 0.]);
        let in_b = || lens("LB", [350., 200., 75.], [1., 0., 0.]);
        // Thin splitters are pellicles: a model value under that assumption.
        let plain = check(&base, 0.);
        assert_eq!(plain["assertions"][0]["status"], "pass");
        assert_eq!(
            plain["assertions"][0]["basis"],
            json!({"kind": "model", "assumptions": [{"code": "thin_splitter", "elements": ["BS1"]}]})
        );
        assert_eq!(plain["passed"], true);
        // Colinearity is read on the combiner's outputs, so the thin combiner
        // is assumed as well; a thin lens in one arm changes nothing.
        let colinear = Assertion {
            kind: AssertionKind::Colinear,
            unit: AssertionUnit::Mrad,
            ..base.assertions[0].clone()
        };
        for bench in [base.clone(), mach_zehnder_with(in_a(), None)] {
            let mut b = bench;
            b.assertions = vec![colinear.clone()];
            assert_eq!(
                lab_check(&b, &system).unwrap()["assertions"][0]["basis"],
                json!({"kind": "model", "assumptions": [{"code": "thin_splitter", "elements": ["BS1", "BS2"]}]})
            );
        }
        // A thin lens leaves out glass only arm A has, so A − B is a lower bound:
        // it can prove the difference too large, never within the tolerance.
        let a = mach_zehnder_with(in_a(), None);
        let undecided = check(&a, 0.);
        let result = &undecided["assertions"][0];
        assert_eq!(result["status"], "indeterminate");
        assert_eq!(result["basis"]["kind"], "lower_bound");
        assert_eq!(
            result["basis"]["assumptions"][1],
            json!({"code": "thin_lens", "elements": ["LA"]})
        );
        assert_eq!(undecided["passed"], false);
        let warning = undecided["issues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["code"] == "assertion_indeterminate")
            .unwrap();
        assert_eq!(
            (&warning["severity"], &warning["element_id"]),
            (&json!("warning"), &json!("BS2"))
        );
        assert!(
            warning["message"]
                .as_str()
                .unwrap()
                .contains("lower bound · thin_splitter BS1 · thin_lens LA")
        );
        let refused = super::check(
            &{
                let mut x = a.clone();
                x.assertions = base.assertions.clone();
                x
            },
            &system,
        )
        .unwrap()
        .require_pass()
        .unwrap_err();
        assert!(refused.ends_with("0 errors and 0 pending inputs; 1 indeterminate assertions: equal_arms (lower bound · thin_splitter BS1 · thin_lens LA)"), "{refused}");
        assert_eq!(check(&a, -1.)["assertions"][0]["status"], "fail");
        assert_eq!(check(&a, 1.)["assertions"][0]["status"], "indeterminate");
        // In arm B the same glass makes A − B an upper bound.
        let b = mach_zehnder_with(
            mount_element("PA", MASK, [250., 300., 75.], [0., 1., 0.]),
            Some(in_b()),
        );
        assert_eq!(
            check(&b, 0.)["assertions"][0]["basis"]["kind"],
            "upper_bound"
        );
        assert_eq!(check(&b, 1.)["assertions"][0]["status"], "fail");
        assert_eq!(check(&b, -1.)["assertions"][0]["status"], "indeterminate");
        // Glass left out of both arms bounds nothing.
        let both = mach_zehnder_with(in_a(), Some(in_b()));
        assert_eq!(
            check(&both, -1.)["assertions"][0]["basis"]["kind"],
            "estimate"
        );
        assert_eq!(
            check(&both, -1.)["assertions"][0]["status"],
            "indeterminate"
        );
        // Omitted glass of unknown dispersion leaves only an estimate: the zero
        // the model computes for the lens arm proves nothing.
        let mut dispersive = a.clone();
        dispersive.assertions = vec![Assertion {
            id: "phase".into(),
            kind: AssertionKind::Dispersion,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit: AssertionUnit::Rad,
            tolerance: 1.,
            band_nm: Some([520., 540.]),
            quantity: Some(DispersionQuantity::Phase),
        }];
        let estimate = &lab_check(&dispersive, &system).unwrap()["assertions"][0];
        assert_eq!(
            (&estimate["status"], &estimate["basis"]["kind"]),
            (&json!("indeterminate"), &json!("estimate")),
            "{estimate}"
        );
        // A thin waveplate leaves its crystal out the same way; given a
        // substrate it carries the glass and the value is a model value again.
        let plate = || mount_element("WA", HWP, [250., 300., 75.], [0., 1., 0.]);
        let thin = check(&mach_zehnder_with(plate(), None), 0.);
        assert_eq!(thin["assertions"][0]["basis"]["kind"], "lower_bound");
        assert_eq!(
            thin["assertions"][0]["basis"]["assumptions"][1],
            json!({"code": "thin_plate", "elements": ["WA"]})
        );
        let mut thick = plate();
        thick.substrate = Some(bench::Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: 3.,
            wedge_deg: None,
            coated_face: None,
        });
        let modelled = check(&mach_zehnder_with(thick, None), 0.);
        assert_eq!(modelled["assertions"][0]["basis"]["kind"], "model");
        assert_eq!(
            modelled["assertions"][0]["basis"]["assumptions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // The drawing shows the undecided assertion in amber with its basis.
        let mut drawn = a.clone();
        drawn.assertions = base.assertions.clone();
        let svg = export_blueprint_report(&drawn, &system, "svg", None)
            .unwrap()
            .content;
        let card = svg
            .split_once(r#"data-kind="assertion" data-id="equal_arms""#)
            .unwrap()
            .1
            .split_once("</g>")
            .unwrap()
            .0;
        assert!(card.contains(r##"fill="#b54708""##), "{card}");
        assert!(card.contains("ΔOPL · equal_arms · indeterminate"));
        assert!(card.contains("Basis lower bound · thin_splitter BS1 · thin_lens LA"));
        let plan = blueprint_drawing(&drawn, &system, 4.).unwrap();
        let mark = plan.labels.iter().find(|l| l.kind == "assertion").unwrap();
        assert_eq!(mark.text, "? equal_arms");
    }

    /// Independent N-BK7 Sellmeier index and dn/dλ (λ in µm, Schott datasheet).
    fn bk7(um: f64) -> (f64, f64) {
        const B: [f64; 3] = [1.039_612_12, 0.231_792_344, 1.010_469_45];
        const C: [f64; 3] = [0.006_000_698_67, 0.020_017_914_4, 103.560_653];
        let l2 = um * um;
        let n2 = 1. + (0..3).map(|i| B[i] * l2 / (l2 - C[i])).sum::<f64>();
        let dn2 = (0..3)
            .map(|i| -2. * B[i] * C[i] * um / (l2 - C[i]).powi(2))
            .sum::<f64>();
        let n = n2.sqrt();
        (n, dn2 / (2. * n))
    }

    fn plate(id: &str, at: V3, normal: V3, thickness_mm: f64) -> Element {
        let mut plate = mount_element(id, MASK, at, normal);
        plate.substrate = Some(bench::Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm,
            wedge_deg: None,
            coated_face: None,
        });
        plate
    }

    /// Mach–Zehnder with element `a` inserted in arm A (BS1→M1, along +Y) and,
    /// optionally, `b` in arm B (BS1→M2, along +X).
    fn mach_zehnder_with(a: Element, b: Option<Element>) -> Bench {
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        for (output, element) in [("reflected", Some(a)), ("transmitted", b)] {
            let Some(element) = element else { continue };
            let connection = bench
                .connections
                .iter_mut()
                .find(|c| c.from == "BS1" && c.port == output)
                .unwrap();
            let target = std::mem::replace(&mut connection.to, element.id.clone());
            bench.connections.push(Connection {
                from: element.id.clone(),
                port: "out".into(),
                to: target,
            });
            bench.elements.push(element);
        }
        bench
    }

    #[test]
    fn tilted_plate_shifts_the_beam_and_adds_glass_opl() {
        let system = System::default();
        let theta = 30_f64.to_radians();
        let t = 6.;
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.branches.clear();
        bench.elements = vec![
            plate("P", [250., 200., 75.], [theta.cos(), theta.sin(), 0.], t),
            mount_element("D", DETECTOR, [450., 200., 75.], [1., 0., 0.]),
        ];
        bench.connections = vec![
            Connection {
                from: "S1".into(),
                port: "out".into(),
                to: "P".into(),
            },
            Connection {
                from: "P".into(),
                port: "out".into(),
                to: "D".into(),
            },
        ];
        bench.branches = vec![Branch {
            id: "line".into(),
            from: Port::new("S1", "out"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        let wavelength = bench.sources[0].wavelength;
        let n = system.material_index("N-BK7_SCHOTT", wavelength).unwrap();
        assert!((n - bk7(wavelength * 1e-3).0).abs() < 1e-9);
        let cos_t = (1. - (theta.sin() / n).powi(2)).sqrt();
        let inside = t / cos_t;
        let shift = t * theta.sin() * (1. - theta.cos() / (n * n - theta.sin().powi(2)).sqrt());
        let trace = bench.trace(&system).unwrap();
        let glass: Vec<_> = trace
            .segments
            .iter()
            .filter(|s| s.status == "substrate")
            .collect();
        assert_eq!(glass.len(), 1);
        let flight = glass[0].flight.as_ref().unwrap();
        assert_eq!(flight.medium, "N-BK7_SCHOTT");
        assert!(
            (flight.length_mm - inside).abs() < 1e-9,
            "{} vs {inside}",
            flight.length_mm
        );
        assert!((flight.opl_mm - n * t / cos_t).abs() < 1e-9);
        let exit = trace
            .segments
            .iter()
            .find(|s| {
                s.flight
                    .as_ref()
                    .is_some_and(|f| f.from_element == "P" && f.from_port == "out")
            })
            .unwrap();
        // Leaves parallel to the incident beam, displaced sideways by the slab shift.
        assert!((exit.to[1] - exit.from[1]).abs() < 1e-9);
        assert!(
            ((exit.from[1] - 200.).abs() - shift).abs() < 1e-9,
            "{} vs {shift}",
            exit.from[1] - 200.
        );
        let path = &trace.branches[0].paths[0];
        assert!(path.interruption.is_none(), "{:?}", path.interruption);
        assert_eq!(path.glass.len(), 1);
        assert_eq!(path.glass[0].element, "P");
        assert_eq!(path.glass[0].thickness_mm, t);
        assert!((path.glass[0].length_mm - inside).abs() < 1e-9);
        // Air from the source to the entry face, glass, then air to the detector plane.
        let air: f64 = path
            .segment_indices
            .iter()
            .filter_map(|&i| trace.segments[i].flight.as_ref())
            .filter(|f| f.medium == "AIR")
            .map(|f| f.length_mm)
            .sum();
        assert!((path.opl_mm - (air + n * inside)).abs() < 1e-9);
        assert!((path.length_mm - (air + inside)).abs() < 1e-9);
        let check = lab_check(&bench, &system).unwrap();
        assert_eq!(check["issues"], json!([]));
        gaussian_envelope(&bench, &system).unwrap();
        export_blueprint(&bench, &system, "svg").unwrap();
    }

    #[test]
    fn compensator_plate_balances_group_delay_dispersion() {
        let system = System::default();
        let t = 5.;
        let band = [600., 900.];
        let assertion = |quantity, unit, tolerance| Assertion {
            id: "gd".into(),
            kind: AssertionKind::Dispersion,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit,
            tolerance,
            band_nm: Some(band),
            quantity: Some(quantity),
        };
        let mut balanced = mach_zehnder_with(
            plate("P", [250., 300., 75.], [0., 1., 0.], t),
            Some(plate("C", [350., 200., 75.], [1., 0., 0.], t)),
        );
        balanced.assertions = vec![assertion(
            DispersionQuantity::GroupDelay,
            AssertionUnit::Fs,
            1e-6,
        )];
        let trace = balanced.trace(&system).unwrap();
        let [result] = check_assertions(&balanced, &system, &trace)
            .try_into()
            .ok()
            .unwrap();
        assert!(
            matches!(result.status, AssertionStatus::Pass),
            "{:?}",
            result.interruption
        );
        let Some(AssertionValue::Dispersion(value)) = &result.value else {
            panic!()
        };
        assert_eq!(value.glass[0][0].element, "P");
        assert_eq!(value.glass[1][0].element, "C");
        assert_eq!(value.glass[0][0].material, "N-BK7_SCHOTT");

        let mut bare = mach_zehnder_with(plate("P", [250., 300., 75.], [0., 1., 0.], t), None);
        bare.assertions = vec![
            assertion(DispersionQuantity::GroupDelay, AssertionUnit::Fs, 1.),
            Assertion {
                id: "phase".into(),
                ..assertion(DispersionQuantity::Phase, AssertionUnit::Rad, 1.)
            },
        ];
        let trace = bare.trace(&system).unwrap();
        let results = check_assertions(&bare, &system, &trace);
        // Group delay excess t·(n − λ dn/dλ − 1)/c falls with λ, so it peaks at λmin.
        let (n, slope) = bk7(band[0] * 1e-3);
        let expected = t * (n - band[0] * 1e-3 * slope - 1.) / C_MM_PER_FS;
        let Some(AssertionValue::Dispersion(gd)) = &results[0].value else {
            panic!()
        };
        assert!(matches!(results[0].status, AssertionStatus::Fail));
        assert_eq!(gd.wavelength_nm, band[0]);
        assert!(
            (gd.difference - expected).abs() < 1e-4,
            "{} vs {expected}",
            gd.difference
        );
        assert!((results[0].residual.unwrap() - expected).abs() < 1e-4);
        assert!(gd.glass[1].is_empty());
        // Relative phase 2π·t·(n − 1)/λ also peaks at λmin.
        let phase = std::f64::consts::TAU * t * (n - 1.) / (band[0] * 1e-6);
        let Some(AssertionValue::Dispersion(ph)) = &results[1].value else {
            panic!()
        };
        assert!(matches!(results[1].status, AssertionStatus::Fail));
        assert!(
            (ph.difference - phase).abs() < 1e-6 * phase,
            "{} vs {phase}",
            ph.difference
        );
    }

    #[test]
    fn splitter_substrate_follows_the_declared_coated_face() {
        // BS1 is hit at 45° on its front face. A front coating reflects in air;
        // a back coating reflects inside, so arm A crosses the glass twice.
        let system = System::default();
        let t = 4.;
        let wavelength = template("mach_zehnder").unwrap().sources[0].wavelength;
        let n = system.material_index("N-BK7_SCHOTT", wavelength).unwrap();
        let sin = std::f64::consts::FRAC_1_SQRT_2;
        let inside = t / (1. - (sin / n).powi(2)).sqrt();
        for (face, reflected_glass) in [(bench::Face::Front, 0.), (bench::Face::Back, 2. * inside)]
        {
            let mut bench = template("mach_zehnder").unwrap();
            bench.assertions.clear();
            let splitter = bench.elements.iter_mut().find(|e| e.id == "BS1").unwrap();
            splitter.substrate = Some(bench::Substrate {
                material: "N-BK7_SCHOTT".into(),
                thickness_mm: t,
                wedge_deg: None,
                coated_face: Some(face),
            });
            let trace = bench.trace(&system).unwrap();
            let [a, b] = [0, 1].map(|i| &trace.branches[i].paths[0]);
            let glass = |path: &BranchPath| path.glass.iter().map(|g| g.length_mm).sum::<f64>();
            assert!(
                (glass(a) - reflected_glass).abs() < 1e-9,
                "{face:?}: {:?}",
                a.glass
            );
            assert!(a.glass.len() <= 1);
            assert!((glass(b) - inside).abs() < 1e-9, "{face:?}: {:?}", b.glass);
            assert_eq!(a.start_segment, b.start_segment);
            // The transmitted arm leaves parallel, so it is laterally shifted.
            let exit = trace
                .segments
                .iter()
                .find(|s| {
                    s.flight
                        .as_ref()
                        .is_some_and(|f| f.from_element == "BS1" && f.from_port == "transmitted")
                })
                .unwrap();
            assert!((exit.to[1] - exit.from[1]).abs() < 1e-9);
            // Refraction through the glass is not a reflection, and the internal
            // bounce off a back coating counts once, like a front one.
            let reference = template("mach_zehnder").unwrap();
            let bare = reference.trace(&system).unwrap();
            for (path, plain) in [a, b].into_iter().zip(&bare.branches) {
                let glass = relay::frame(&bench, &trace, path).unwrap();
                let air = relay::frame(&reference, &bare, &plain.paths[0]).unwrap();
                assert_eq!(glass.reflections, air.reflections, "{face:?}");
                assert_eq!(glass.handedness(), air.handedness());
                assert!(relay::image_rotation(&glass, &air).abs() < 1e-9);
            }
        }
    }

    /// Single plate P (centred on its back face at [250, 200, 75]) between S1 and D.
    fn back_entry_bench(t: f64, aperture: f64) -> Bench {
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        let normal = [
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.,
        ];
        let mut p = plate("P", add([250., 200., 75.], scale(normal, t)), normal, t);
        p.aperture = aperture;
        p.body_radius = 6.;
        bench.elements = vec![
            p,
            mount_element("D", DETECTOR, [450., 200., 75.], [1., 0., 0.]),
        ];
        bench.connections = vec![
            Connection {
                from: "S1".into(),
                port: "out".into(),
                to: "P".into(),
            },
            Connection {
                from: "P".into(),
                port: "out".into(),
                to: "D".into(),
            },
        ];
        bench.branches = vec![Branch {
            id: "line".into(),
            from: Port::new("S1", "out"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        bench
    }

    #[test]
    fn substrate_is_hit_on_its_entry_face_not_the_element_plane() {
        // Light along +x meets the 45° plate's back face at its centre. The
        // unrefracted ray would cross the element plane 10 mm off axis, outside
        // the 6 mm body, yet the refracted ray leaves 10·tanθt ≈ 5.3 mm off axis.
        let system = System::default();
        let t = 10.;
        let bench = back_entry_bench(t, 6.);
        let n = system
            .material_index("N-BK7_SCHOTT", bench.sources[0].wavelength)
            .unwrap();
        let sin_t = std::f64::consts::FRAC_1_SQRT_2 / n;
        let inside = t / (1. - sin_t * sin_t).sqrt();
        assert!(t * sin_t / (1. - sin_t * sin_t).sqrt() < 6.);
        let trace = bench.trace(&system).unwrap();
        let glass: Vec<_> = trace
            .segments
            .iter()
            .filter(|s| s.status == "substrate")
            .collect();
        assert_eq!(glass.len(), 1);
        assert!(
            norm(sub(glass[0].from, [250., 200., 75.])) < 1e-9,
            "{:?}",
            glass[0].from
        );
        assert!((glass[0].flight.as_ref().unwrap().length_mm - inside).abs() < 1e-9);
        let path = &trace.branches[0].paths[0];
        assert!(path.interruption.is_none(), "{:?}", path.interruption);
        assert!((path.glass[0].length_mm - inside).abs() < 1e-9);

        // A 5 mm clear aperture clips the exit point although the entry is central.
        let trace = back_entry_bench(t, 5.).trace(&system).unwrap();
        assert!(trace.segments.iter().all(|s| s.status != "substrate"));
        assert!(
            trace
                .terminations
                .iter()
                .any(|t| t.element.as_deref() == Some("P") && t.reason == "clipped")
        );
        let path = &trace.branches[0].paths[0];
        assert_eq!(
            path.interruption.as_ref().map(|b| b.reason.as_str()),
            Some("clipped")
        );
    }

    fn glass_plate(id: &str, at: V3, normal: V3, material: &str, thickness_mm: f64) -> Element {
        let mut element = plate(id, at, normal, thickness_mm);
        element.substrate.as_mut().unwrap().material = material.into();
        element
    }

    #[test]
    fn dispersion_reports_the_band_maximum_between_samples() {
        // N-BK7 against a thicker fused silica plate: the phase difference has an
        // interior maximum near 655 nm that a coarse fixed grid misses.
        let system = System::default();
        let band = [400., 900.];
        let silica = 1.128_785_627_349_848_7;
        let mut bench = mach_zehnder_with(
            glass_plate("P", [250., 300., 75.], [0., 1., 0.], "N-BK7_SCHOTT", 1.),
            Some(glass_plate(
                "C",
                [350., 200., 75.],
                [1., 0., 0.],
                "FUSED_SILICA",
                silica,
            )),
        );
        bench.assertions = vec![Assertion {
            id: "phase".into(),
            kind: AssertionKind::Dispersion,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit: AssertionUnit::Rad,
            tolerance: 7.8285,
            band_nm: Some(band),
            quantity: Some(DispersionQuantity::Phase),
        }];
        // Independent dense scan at 0.01 nm from the material library.
        let phase = |material: &str, t: f64, w: f64| {
            std::f64::consts::TAU * (system.material_index(material, w).unwrap() - 1.) * t
                / (w * 1e-6)
        };
        let (peak, at) = (0..=50_000)
            .map(|i| band[0] + i as f64 * 0.01)
            .map(|w| {
                (
                    (phase("N-BK7_SCHOTT", 1., w) - phase("FUSED_SILICA", silica, w)).abs(),
                    w,
                )
            })
            .fold((0., 0.), |a, b| if b.0 > a.0 { b } else { a });
        assert!(peak > 7.8285 && (at - 655.).abs() < 5., "{peak} at {at}");
        let trace = bench.trace(&system).unwrap();
        let [result] = check_assertions(&bench, &system, &trace)
            .try_into()
            .ok()
            .unwrap();
        let Some(AssertionValue::Dispersion(value)) = &result.value else {
            panic!("{:?}", result.interruption)
        };
        assert!(matches!(result.status, AssertionStatus::Fail));
        assert!(
            value.difference.abs() >= peak - 1e-9,
            "{} vs {peak}",
            value.difference
        );
        assert!(value.difference.abs() <= peak + 1e-6);
        assert!(
            (value.wavelength_nm - at).abs() < 0.1,
            "{} vs {at}",
            value.wavelength_nm
        );
    }

    #[test]
    fn dispersion_separates_close_stationary_points() {
        // Formula-3 glasses whose phase difference has stationary points near 401,
        // 402.2 and 403 nm, with the two maxima within 1e-6 rad of each other. The
        // target and tolerance put the band maximum (≈401 nm) just out of tolerance.
        let mut system = System::default();
        for (name, coefficients) in [
            (
                "WIGGLE_A",
                vec![
                    1.495_775_425_531_285,
                    0.,
                    0.,
                    -0.078_399_543_545_400_02,
                    0.161_658_280_000_000_02,
                    -0.2,
                ],
            ),
            ("WIGGLE_B", vec![1.5, 0., 0., 0., 0., 0.]),
        ] {
            system.custom_glasses.push(crate::optics::material::Glass {
                name: name.into(),
                catalog: "TEST".into(),
                formula: 3,
                coefficients,
                nd: 1.5,
                vd: 50.,
                wavelength_range: None,
                thermal: None,
                transmission: vec![],
                comment: String::new(),
            });
        }
        // Phase scales with thickness, so 10 mm plates take a tenth of the 100 mm
        // target and tolerance.
        let (t, target, tolerance) = (
            10.,
            -34_282.880_646_204_69 / 10.,
            13_156.848_988_582_788 / 10.,
        );
        let mut bench = mach_zehnder_with(
            glass_plate("P", [250., 300., 75.], [0., 1., 0.], "WIGGLE_A", t),
            Some(glass_plate(
                "C",
                [350., 200., 75.],
                [1., 0., 0.],
                "WIGGLE_B",
                t,
            )),
        );
        bench.assertions = vec![Assertion {
            id: "phase".into(),
            kind: AssertionKind::Dispersion,
            branches: ["arm_a".into(), "arm_b".into()],
            target,
            unit: AssertionUnit::Rad,
            tolerance,
            band_nm: Some([400., 900.]),
            quantity: Some(DispersionQuantity::Phase),
        }];
        let phase = |material: &str, w: f64| {
            std::f64::consts::TAU * (system.material_index(material, w).unwrap() - 1.) * t
                / (w * 1e-6)
        };
        let deviation = |w: f64| (phase("WIGGLE_A", w) - phase("WIGGLE_B", w) - target).abs();
        let (peak, at) = (0..=5_000)
            .map(|i| 400. + i as f64 * 1e-3)
            .map(|w| (deviation(w), w))
            .fold((0., 0.), |a, b| if b.0 > a.0 { b } else { a });
        assert!(
            peak > tolerance && (at - 401.).abs() < 0.5,
            "{peak} at {at}"
        );
        let trace = bench.trace(&system).unwrap();
        let [result] = check_assertions(&bench, &system, &trace)
            .try_into()
            .ok()
            .unwrap();
        let Some(AssertionValue::Dispersion(value)) = &result.value else {
            panic!("{:?}", result.interruption)
        };
        assert!(
            matches!(result.status, AssertionStatus::Fail),
            "{:?}",
            result.residual
        );
        assert!(result.residual.unwrap().abs() >= peak - 1e-9);
        assert!(
            (value.wavelength_nm - at).abs() < 0.01,
            "{} vs {at}",
            value.wavelength_nm
        );
    }

    #[test]
    fn wedged_substrate_is_entered_through_the_wedge_face() {
        // A 45° wedge with a back coating, met first on the wedge face: the front
        // plane lies behind the ray, so only the wedge face can be the entry.
        let system = System::default();
        let rotation = normal_rotation([1., 0., 0.]);
        let origin = [250., 200., 75.];
        let world = |l: V3| add(origin, bench::rotate(l, rotation));
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        let mut splitter = bench
            .elements
            .iter()
            .find(|e| e.id == "BS1")
            .unwrap()
            .clone();
        splitter.id = "W".into();
        splitter.position = origin;
        splitter.rotation = Some(rotation);
        (splitter.aperture, splitter.body_radius) = (6., 6.);
        (splitter.reflectance, splitter.transmittance) = (1., 0.);
        (splitter.index, splitter.coating) = (None, None);
        splitter.substrate = Some(bench::Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: 10.,
            wedge_deg: Some(45.),
            coated_face: Some(bench::Face::Back),
        });
        let d = [-0.1, 0.99_f64.sqrt(), 0.];
        let start = [-4., -10., 0.];
        // Wedge face x + y = −10.
        let s = (-10. - start[0] - start[1]) / (d[0] + d[1]);
        let entry = add(start, scale(d, s));
        let reflected = [-(0.99_f64.sqrt()), 0.1, 0.];
        bench.sources[0].position = world(start);
        bench.sources[0].direction = bench::rotate(d, rotation);
        bench.sources[0].beam_radius = 0.1;
        let out = bench::rotate(reflected, rotation);
        bench.elements = vec![
            splitter,
            mount_element(
                "D",
                DETECTOR,
                world(add(entry, scale(reflected, 100.))),
                out,
            ),
        ];
        bench.connections = vec![
            Connection {
                from: "S1".into(),
                port: "out".into(),
                to: "W".into(),
            },
            Connection {
                from: "W".into(),
                port: "reflected".into(),
                to: "D".into(),
            },
        ];
        bench.branches = vec![Branch {
            id: "line".into(),
            from: Port::new("S1", "out"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        let trace = bench.trace(&system).unwrap();
        let path = &trace.branches[0].paths[0];
        assert!(path.interruption.is_none(), "{:?}", path.interruption);
        assert!(path.glass.is_empty(), "{:?}", path.glass);
        let arrival = &trace.segments[path.segment_indices[0]];
        assert!(
            norm(sub(arrival.to, world(entry))) < 1e-9,
            "{:?} vs {:?}",
            arrival.to,
            world(entry)
        );
        let leaving = &trace.segments[path.segment_indices[1]];
        assert!(norm(sub(unit(sub(leaving.to, leaving.from)).unwrap(), out)) < 1e-9);
    }

    #[test]
    fn colinearity_is_measured_from_the_glass_exit() {
        // A back-coated substrate fed from both sides on one axis: A crosses the
        // glass, B reflects in air at the coated back face. Both leave from the
        // back face centre along −x, so the outputs coincide.
        let system = System::default();
        let rotation = normal_rotation([1., 0., 0.]);
        let origin = [250., 200., 75.];
        let world = |l: V3| add(origin, bench::rotate(l, rotation));
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        let mut splitter = bench
            .elements
            .iter()
            .find(|e| e.id == "BS1")
            .unwrap()
            .clone();
        splitter.id = "W".into();
        splitter.position = origin;
        splitter.rotation = Some(rotation);
        (splitter.reflectance, splitter.transmittance) = (0.5, 0.5);
        (splitter.index, splitter.coating) = (None, None);
        splitter.substrate = Some(bench::Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: 10.,
            wedge_deg: None,
            coated_face: Some(bench::Face::Back),
        });
        let mut b = bench.sources[0].clone();
        bench.sources[0].position = world([60., 0., 0.]);
        bench.sources[0].direction = bench::rotate([-1., 0., 0.], rotation);
        b.id = "S2".into();
        b.position = world([-30., 0., 0.]);
        b.direction = bench::rotate([1., 0., 0.], rotation);
        bench.sources.push(b);
        let out = bench::rotate([-1., 0., 0.], rotation);
        bench.elements = vec![
            splitter,
            mount_element("D", DETECTOR, world([-120., 0., 0.]), out),
        ];
        bench.connections = vec![
            Connection {
                from: "S1".into(),
                port: "out".into(),
                to: "W".into(),
            },
            Connection {
                from: "S2".into(),
                port: "out".into(),
                to: "W".into(),
            },
            Connection {
                from: "W".into(),
                port: "transmitted".into(),
                to: "D".into(),
            },
            Connection {
                from: "W".into(),
                port: "reflected".into(),
                to: "D".into(),
            },
        ];
        let arm = |id: &str, from: &str, input: &str| Branch {
            id: id.into(),
            from: Port::new(from, "out"),
            to: Port::new("W", input),
            via: vec![],
            style: None,
        };
        bench.branches = vec![arm("a", "S1", "in_a"), arm("b", "S2", "in_b")];
        bench.assertions = vec![Assertion {
            id: "overlap".into(),
            kind: AssertionKind::Colinear,
            branches: ["a".into(), "b".into()],
            target: 0.,
            unit: AssertionUnit::Mm,
            tolerance: 1e-6,
            band_nm: None,
            quantity: None,
        }];
        let trace = bench.trace(&system).unwrap();
        let [result] = check_assertions(&bench, &system, &trace)
            .try_into()
            .ok()
            .unwrap();
        assert!(
            matches!(result.status, AssertionStatus::Pass),
            "{:?} {:?}",
            result.residual,
            result.interruption
        );
        // The combiner's glass is modelled: nothing is assumed thin.
        assert!(result.basis.is_plain(), "{:?}", result.basis);
    }

    #[test]
    fn dispersion_finds_a_peak_narrower_than_the_initial_cells() {
        // A pole-like term puts a ~37 700 rad spike within 1 pm of the band start,
        // well inside the first 0.05 nm cell; the bend of the cell's midpoint
        // forces it to be refined.
        let mut system = System::default();
        for (name, coefficients) in [
            (
                "SPIKE_A",
                vec![
                    -40.320_755_060_657_13,
                    1.338_660_042_482_509e-7,
                    -4.480_026_773_348_182_7e-14,
                    1_493.580_543_268_169_8,
                    0.,
                    0.,
                ],
            ),
            ("SPIKE_B", vec![1.5, 0., 0., 0., 0., 0.]),
        ] {
            system.custom_glasses.push(crate::optics::material::Glass {
                name: name.into(),
                catalog: "TEST".into(),
                formula: 3,
                coefficients,
                nd: 1.5,
                vd: 50.,
                wavelength_range: None,
                thermal: None,
                transmission: vec![],
                comment: String::new(),
            });
        }
        let t = 10.;
        let band = [167.333_005_306_815_12, 167.433_005_306_815_12];
        let phase = |w: f64| {
            let n = |m: &str| {
                system.material_index(m, w).unwrap() - system.material_index("AIR", w).unwrap()
            };
            TAU * (n("SPIKE_A") - n("SPIKE_B")) * t / (w * 1e-6)
        };
        let (at, peak) = (0..=100_000)
            .map(|i| band[0] + (band[1] - band[0]) * f64::from(i) / 100_000.)
            .map(|w| (w, phase(w).abs()))
            .fold((0., 0.), |best, x| if x.1 > best.1 { x } else { best });
        assert!(
            peak > 37_000. && (at - 167.334).abs() < 1e-3,
            "{peak} at {at}"
        );
        let mut bench = mach_zehnder_with(
            glass_plate("P", [250., 300., 75.], [0., 1., 0.], "SPIKE_A", t),
            Some(glass_plate(
                "C",
                [350., 200., 75.],
                [1., 0., 0.],
                "SPIKE_B",
                t,
            )),
        );
        bench.assertions = vec![Assertion {
            id: "spike".into(),
            kind: AssertionKind::Dispersion,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit: AssertionUnit::Rad,
            tolerance: 25_000.,
            band_nm: Some(band),
            quantity: Some(DispersionQuantity::Phase),
        }];
        let trace = bench.trace(&system).unwrap();
        let [result] = check_assertions(&bench, &system, &trace)
            .try_into()
            .ok()
            .unwrap();
        assert!(
            matches!(result.status, AssertionStatus::Fail),
            "{:?}",
            result.residual
        );
        let Some(AssertionValue::Dispersion(value)) = &result.value else {
            panic!()
        };
        assert!(
            value.difference.abs() >= peak - 1e-6 * peak,
            "{} vs {peak}",
            value.difference
        );
        assert!(value.deviation_bound >= value.difference.abs());
        assert!(
            (value.wavelength_nm - at).abs() < 1e-4,
            "{} vs {at}",
            value.wavelength_nm
        );
    }

    #[test]
    fn dispersion_bound_encloses_a_peak_the_samples_miss() {
        // Review counterexample: with x = (λ/µm)² − 0.028 and ε = x(λmin),
        // nA − nB = 0.4ε/x − 0.4ε²/x² + K·(x − ε), whose rational part peaks at
        // x = 2ε, between the samples of the first cell. Sampling alone reported
        // ~22 500 rad as the maximum; the true peak is ~37 500 rad.
        let mut system = System::default();
        let k = 1_792.307_349_035_117_8;
        for (name, coefficients) in [
            (
                "KINK_A",
                vec![
                    -48.684_606_372_804_06,
                    1.338_656_033_245_122_6e-10,
                    -4.479_999_938_358_917e-20,
                    k,
                    0.,
                    0.,
                ],
            ),
            ("KINK_B", vec![1.5, 0., 0., 0., 0., 0.]),
        ] {
            system.custom_glasses.push(crate::optics::material::Glass {
                name: name.into(),
                catalog: "TEST".into(),
                formula: 3,
                coefficients,
                nd: 1.5,
                vd: 50.,
                wavelength_range: None,
                thermal: None,
                transmission: vec![],
                comment: String::new(),
            });
        }
        let t = 10.;
        let band = [167.332_006_306_815_12, 167.432_006_306_815_1];
        let epsilon = (band[0] / 1000.).powi(2) - 0.028;
        let at = 1000. * (0.028 + 2. * epsilon).sqrt();
        let analytic = TAU * (0.1 + k * epsilon) * t / (at * 1e-6);
        let n = |m: &str| {
            system.material_index(m, at).unwrap() - system.material_index("AIR", at).unwrap()
        };
        let peak = TAU * (n("KINK_A") - n("KINK_B")) * t / (at * 1e-6);
        assert!(
            peak > 37_000. && ((peak - analytic) / analytic).abs() < 1e-3,
            "{peak} vs {analytic}"
        );
        let mut bench = mach_zehnder_with(
            glass_plate("P", [250., 300., 75.], [0., 1., 0.], "KINK_A", t),
            Some(glass_plate(
                "C",
                [350., 200., 75.],
                [1., 0., 0.],
                "KINK_B",
                t,
            )),
        );
        bench.assertions = vec![Assertion {
            id: "kink".into(),
            kind: AssertionKind::Dispersion,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit: AssertionUnit::Rad,
            tolerance: 25_000.,
            band_nm: Some(band),
            quantity: Some(DispersionQuantity::Phase),
        }];
        let trace = bench.trace(&system).unwrap();
        let [result] = check_assertions(&bench, &system, &trace)
            .try_into()
            .ok()
            .unwrap();
        assert!(
            matches!(result.status, AssertionStatus::Fail),
            "{} {:?}",
            result.status.as_str(),
            result.residual
        );
        let Some(AssertionValue::Dispersion(value)) = &result.value else {
            panic!()
        };
        assert!(
            value.deviation_bound >= peak,
            "{} vs {peak}",
            value.deviation_bound
        );
        // A tolerance between the largest sampled deviation and the enclosure's
        // bound: the samples cannot fail it, the bound cannot pass it.
        let tolerance = (value.difference.abs() + value.deviation_bound) / 2.;
        assert!(tolerance > value.difference.abs() && tolerance < value.deviation_bound);
        bench.assertions[0].tolerance = tolerance;
        let check = lab_check(&bench, &system).unwrap();
        let undecided = &check["assertions"][0];
        assert_eq!(undecided["status"], "indeterminate");
        assert_eq!(undecided["basis"]["kind"], "lower_bound");
        assert_eq!(undecided["basis"]["omitted"][0]["code"], "band_enclosure");
        assert!(undecided.get("break").is_none());
        assert_eq!(check["passed"], false);
        assert!(
            check["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == "assertion_indeterminate" && i["severity"] == "warning")
        );
        assert!(
            value.difference.abs() >= peak * (1. - 1e-6),
            "{} vs {peak}",
            value.difference
        );
        assert!(
            (value.wavelength_nm - at).abs() < 1e-4,
            "{} vs {at}",
            value.wavelength_nm
        );
    }

    #[test]
    fn dispersion_is_unresolved_when_the_enclosure_diverges() {
        // Both arms carry differently named glasses with the same formula-3 pole at
        // √0.028 µm ≈ 167.33 nm. Δ cancels the pole, so every sample is ~0, but each
        // material is enclosed separately and the enclosure of the cell around the
        // pole is unbounded: the assertion must be unresolved, not pass on samples.
        let mut system = System::default();
        for name in ["POLE_A", "POLE_B"] {
            system.custom_glasses.push(crate::optics::material::Glass {
                name: name.into(),
                catalog: "TEST".into(),
                formula: 3,
                coefficients: vec![1.5, 0., 1e-30, 0., 0., 0.],
                nd: 1.5,
                vd: 50.,
                wavelength_range: None,
                thermal: None,
                transmission: vec![],
                comment: String::new(),
            });
        }
        // Differently named glasses, and the same glass whose net length cancels.
        for compensator in ["POLE_B", "POLE_A"] {
            let mut bench = mach_zehnder_with(
                glass_plate("P", [250., 300., 75.], [0., 1., 0.], "POLE_A", 1.),
                Some(glass_plate(
                    "C",
                    [350., 200., 75.],
                    [1., 0., 0.],
                    compensator,
                    1.,
                )),
            );
            bench.assertions = vec![Assertion {
                id: "pole".into(),
                kind: AssertionKind::Dispersion,
                branches: ["arm_a".into(), "arm_b".into()],
                target: 0.,
                unit: AssertionUnit::Rad,
                tolerance: 1.,
                band_nm: Some([160., 175.]),
                quantity: Some(DispersionQuantity::Phase),
            }];
            let trace = bench.trace(&system).unwrap();
            let [result] = check_assertions(&bench, &system, &trace)
                .try_into()
                .ok()
                .unwrap();
            assert!(
                matches!(result.status, AssertionStatus::Unreachable),
                "{compensator}: {} {:?}",
                result.status.as_str(),
                result.residual
            );
            let interruption = result.interruption.unwrap();
            assert_eq!(
                interruption.reason, "dispersion_unresolved",
                "{:?}",
                interruption.detail
            );
        }
    }

    #[test]
    fn group_delay_is_evaluated_up_to_the_band_limits() {
        // Equal silica plates balance exactly, including at the 20 µm validation limit
        // where the derivative cannot look past the band edge.
        let system = System::default();
        let mut bench = mach_zehnder_with(
            glass_plate("P", [250., 300., 75.], [0., 1., 0.], "FUSED_SILICA", 3.),
            Some(glass_plate(
                "C",
                [350., 200., 75.],
                [1., 0., 0.],
                "FUSED_SILICA",
                3.,
            )),
        );
        let gd = |band: [f64; 2]| Assertion {
            id: format!("gd{}", band[0]),
            kind: AssertionKind::Dispersion,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit: AssertionUnit::Fs,
            tolerance: 1e-9,
            band_nm: Some(band),
            quantity: Some(DispersionQuantity::GroupDelay),
        };
        bench.assertions = vec![gd([19000., 20000.]), gd([200., 300.]), gd([100., 300.])];
        let trace = bench.trace(&system).unwrap();
        let results = check_assertions(&bench, &system, &trace);
        for result in &results[..2] {
            assert!(
                matches!(result.status, AssertionStatus::Pass),
                "{}: {:?}",
                result.assertion,
                result.interruption
            );
        }
        // The silica model has no real index far in the UV; the break names the
        // material, the wavelength and the library's reason.
        assert!(matches!(results[2].status, AssertionStatus::Unreachable));
        let interruption = results[2].interruption.as_ref().unwrap();
        assert_eq!(
            (interruption.element.as_str(), interruption.reason.as_str()),
            ("P", "material_error")
        );
        let detail = interruption.detail.as_deref().unwrap();
        assert!(
            detail.starts_with("FUSED_SILICA at ") && detail.contains(" nm: "),
            "{detail}"
        );
        // An unbalanced pair at the upper limit reports the one-sided n_g, which
        // agrees with the central difference just inside the edge.
        bench.elements.retain(|e| e.id != "C");
        bench.connections.retain(|c| c.from != "C");
        bench
            .connections
            .iter_mut()
            .filter(|c| c.to == "C")
            .for_each(|c| c.to = "M2".into());
        let trace = bench.trace(&system).unwrap();
        let results = check_assertions(&bench, &system, &trace);
        let Some(AssertionValue::Dispersion(edge)) = &results[0].value else {
            panic!("{:?}", results[0].interruption)
        };
        let ng = |w: f64| {
            let n = |w: f64| system.material_index("FUSED_SILICA", w).unwrap();
            n(w) - w * (n(w + 0.01) - n(w - 0.01)) / 0.02
        };
        let w = edge.wavelength_nm.min(19999.98);
        assert!(
            ((edge.difference - 3. * (ng(w) - 1.) / C_MM_PER_FS) / edge.difference).abs() < 1e-4
        );
    }

    #[test]
    fn splitter_substrate_requires_a_coated_face() {
        let mut bench = template("mach_zehnder").unwrap();
        bench
            .elements
            .iter_mut()
            .find(|e| e.id == "BS1")
            .unwrap()
            .substrate = Some(bench::Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: 4.,
            wedge_deg: None,
            coated_face: None,
        });
        let error = bench.validate().unwrap_err();
        assert!(error.contains("coated face"), "{error}");
    }

    #[test]
    fn phase_plate_adds_its_phase_to_the_branch() {
        let system = System::default();
        let base = mach_zehnder_with(
            mount_element("Q", MASK, [250., 300., 75.], [0., 1., 0.]),
            None,
        );
        let wavelength = base.sources[0].wavelength;
        let mut plated = base.clone();
        plated
            .elements
            .iter_mut()
            .find(|e| e.id == "Q")
            .unwrap()
            .phase = Some(bench::PhasePlate {
            radians: None,
            waves: Some(0.25),
            at_wavelength_nm: wavelength / 2.,
        });
        let phase = |bench: &Bench| {
            let trace = bench.trace(&system).unwrap();
            let path = &trace.branches[0].paths[0];
            assert!(path.interruption.is_none());
            (
                path.phase_rad,
                path.opl_mm,
                trace.branches[1].paths[0].phase_rad,
            )
        };
        let (before, opl, other) = phase(&base);
        let (after, opl_after, other_after) = phase(&plated);
        // Quarter wave at λ/2 is π/4 at λ; geometry and the other arm are unchanged.
        assert!(
            (after - before - std::f64::consts::FRAC_PI_4).abs() < 1e-9,
            "{}",
            after - before
        );
        assert_eq!(opl, opl_after);
        assert_eq!(other, other_after);
        assert!((before - std::f64::consts::TAU * opl / (wavelength * 1e-6)).abs() < 1e-6);
    }

    #[test]
    fn interrupted_arms_identify_the_physical_obstacle_or_missed_input() {
        let system = System::default();
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.push(Assertion {
            id: "overlap".into(),
            kind: AssertionKind::Colinear,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit: AssertionUnit::Mrad,
            tolerance: 1e-6,
            band_nm: None,
            quantity: None,
        });
        let mut blocked = bench.clone();
        blocked
            .elements
            .push(mount_element("STOP", DUMP, [250., 300., 75.], [0., 1., 0.]));
        let trace = blocked.trace(&system).unwrap();
        let partial = &trace.branches[0].paths[0];
        assert!(!partial.nominal_match);
        assert!((partial.length_mm - 100.).abs() <= 1e-9);
        assert!((partial.opl_mm - 100.).abs() <= 1e-9);
        let mut missed = bench;
        missed.elements[1].position[1] += 50.;
        missed.elements[1].position[0] += 100.;
        for (scene, expected) in [
            (
                blocked,
                json!({"element":"STOP", "port":"in", "reason":"unexpected_hit"}),
            ),
            (
                missed,
                json!({"element":"M1", "port":"in", "reason":"missed_target"}),
            ),
        ] {
            let check = lab_check(&scene, &system).unwrap();
            for result in check["assertions"].as_array().unwrap() {
                assert_eq!(result["status"], "unreachable");
                assert_eq!(result["value"], Value::Null);
                assert_eq!(result["residual"], Value::Null);
                assert_eq!(result["break"], expected);
            }
        }
    }

    #[test]
    fn auto_aim_refines_to_the_fixed_arms_off_center_ray() {
        let system = System::default();
        let mut bench = template("mach_zehnder").unwrap();
        bench.elements[1].rotation.as_mut().unwrap()[2] += 0.5;
        bench.elements[2].aim_rotation = true;
        bench.elements[3].aim_rotation = true;
        bench.assertions.push(Assertion {
            id: "overlap".into(),
            kind: AssertionKind::Colinear,
            branches: ["arm_a".into(), "arm_b".into()],
            target: 0.,
            unit: AssertionUnit::Um,
            tolerance: 1e-3,
            band_nm: None,
            quantity: None,
        });

        let initial = lab_check(&bench, &system).unwrap();
        let overlap = &initial["assertions"][1];
        assert_eq!(overlap["status"], "fail");
        assert!(
            (overlap["residual"].as_f64().unwrap()
                - 1000. * overlap["value"]["position_mm"].as_f64().unwrap())
            .abs()
                < 1e-9
        );

        let aimed = aim(&bench, false, None, &system).unwrap();
        assert!(aimed.ok, "{:?}", aimed.conflicts);
        assert_eq!(aimed.adjusted_elements, ["M2", "BS2"]);
        assert_eq!(aimed.bench.elements[1].rotation, bench.elements[1].rotation);
        let trace = aimed.bench.trace(&system).unwrap();
        let [first, second] = trace.branches.as_slice() else {
            panic!("expected two arms")
        };
        let points =
            [first, second].map(|report| trace.segments[report.paths[0].end_segment.unwrap()].to);
        assert!(norm(sub(points[0], points[1])) < 1e-6);
        assert!(norm(sub(points[0], bench.elements[3].position)) > 3.);
        let report = lab_check(&aimed.bench, &system).unwrap();
        let overlap = &report["assertions"][1];
        assert_eq!(overlap["status"], "pass");
        assert!(overlap["value"]["angle_mrad"].as_f64().unwrap() < 1e-6);
    }

    #[test]
    fn branch_via_selects_an_intermediate_splitter_output() {
        let mut bench = template("mach_zehnder").unwrap();
        let mut plate = mount_element("BS3", SPLITTER, [250., 300., 75.], [0., 1., 0.]);
        plate.reflectance = 0.;
        plate.transmittance = 1.;
        bench.elements.push(plate);
        bench.connections[1].to = "BS3".into();
        for (port, to) in [("transmitted", "M1"), ("reflected", "D1")] {
            bench.connections.push(Connection {
                from: "BS3".into(),
                port: port.into(),
                to: to.into(),
            });
        }
        assert!(
            bench
                .validate()
                .unwrap_err()
                .contains("BS3: select an intermediate output in via")
        );
        bench.branches[0].via = vec![Port::new("BS3", "transmitted")];
        let trace = bench.trace(&System::default()).unwrap();
        let arm = &trace.branches[0].paths[0];
        assert!(arm.nominal_match);
        assert!(arm.ports.contains(&Port::new("BS3", "transmitted")));
        assert!((arm.opl_mm - 400.).abs() <= 1e-9);
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
            // The three-hole mask passes only its holes' share of the wide beam.
            let floor = if name == "three_arm" { 0.1 } else { 0.4 };
            assert!(trace.power.detected > floor, "{name}: {:?}", trace.power);
            close(trace.balance_error, 0.);
            assert!(
                trace
                    .terminations
                    .iter()
                    .all(|t| matches!(t.reason.as_str(), "detected" | "beam_stop" | "masked")),
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
        bench.elements[0].rotation = Some([15., 30., 90.]);
        let mut draft = bench.clone();
        for element in &mut draft.elements {
            element.rotation = None;
        }
        let result = auto_aim(&draft, true, Some(85.)).unwrap();
        let adjusted: Bench = serde_json::from_value(result["bench"].clone()).unwrap();
        assert_eq!(adjusted.elements[0].position, [260., 185., 85.]);
        assert_eq!(adjusted.sources[0].position[2], 85.);
        assert_eq!(bench.elements[0].rotation, Some([15., 30., 90.]));
        assert_eq!(result["adjusted_elements"], json!(["M1", "M2", "D"]));
        let normal = bench::rotate([1., 0., 0.], adjusted.elements[0].rotation());
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
            rows.as_chunks::<2>()
                .0
                .iter()
                .all(|[code, _]| code.parse::<i32>().is_ok())
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

    fn layered_bench(elements: Vec<Element>, edges: &[(&str, &str, &str)]) -> Bench {
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.branches.clear();
        bench.elements = elements;
        bench.connections = edges
            .iter()
            .map(|&(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .collect();
        bench
    }

    fn aimed(id: &str, hardware: Hardware, position: V3) -> Element {
        Element {
            rotation: None,
            ..mount_element(id, hardware, position, [1., 0., 0.])
        }
    }

    fn relay_bench(
        sources: &[(&str, V3, V3)],
        elements: Vec<Element>,
        edges: &[(&str, &str, &str)],
    ) -> Bench {
        let mut bench = layered_bench(elements, edges);
        let template = bench.sources[0].clone();
        bench.sources = sources
            .iter()
            .map(|&(id, position, direction)| bench::Source {
                id: id.into(),
                position,
                direction,
                ..template.clone()
            })
            .collect();
        bench
    }

    fn route(id: &str, from: &str, to: &str) -> Branch {
        Branch {
            id: id.into(),
            from: Port::new(from, "out"),
            to: Port::new(to, "in"),
            via: vec![],
            style: None,
        }
    }

    fn mirror(id: &str, at: V3, incoming: V3, outgoing: V3) -> Element {
        mount_element(id, MIRROR, at, sub(outgoing, incoming))
    }

    #[test]
    fn three_and_four_in_plane_reflections_have_opposite_handedness() {
        let system = System::default();
        let (x, y, nx, ny) = ([1., 0., 0.], [0., 1., 0.], [-1., 0., 0.], [0., -1., 0.]);
        let mut bench = relay_bench(
            &[("S1", [100., 100., 100.], x), ("S2", [100., 300., 100.], x)],
            vec![
                mirror("M1", [200., 100., 100.], x, y),
                mirror("M2", [200., 200., 100.], y, nx),
                mirror("M3", [150., 200., 100.], nx, ny),
                mount_element("D1", DETECTOR, [150., 150., 100.], ny),
                mirror("M4", [200., 300., 100.], x, y),
                mirror("M5", [200., 400., 100.], y, nx),
                mirror("M6", [150., 400., 100.], nx, ny),
                mirror("M7", [150., 350., 100.], ny, x),
                mount_element("D2", DETECTOR, [250., 350., 100.], x),
            ],
            &[
                ("S1", "out", "M1"),
                ("M1", "reflected", "M2"),
                ("M2", "reflected", "M3"),
                ("M3", "reflected", "D1"),
                ("S2", "out", "M4"),
                ("M4", "reflected", "M5"),
                ("M5", "reflected", "M6"),
                ("M6", "reflected", "M7"),
                ("M7", "reflected", "D2"),
            ],
        );
        bench.branches = vec![route("three", "S1", "D1"), route("four", "S2", "D2")];
        bench.assertions = vec![Assertion {
            id: "parity".into(),
            kind: AssertionKind::Parity,
            branches: ["three".into(), "four".into()],
            target: 1.,
            unit: AssertionUnit::Count,
            tolerance: 0.,
            band_nm: None,
            quantity: None,
        }];
        let check = lab_check(&bench, &system).unwrap();
        let readings = check["handedness"].as_array().unwrap();
        let by = |id: &str| readings.iter().find(|r| r["branch"] == id).unwrap();
        assert_eq!(by("three")["reflections"], 3);
        assert_eq!(by("three")["parity"], 1);
        assert_eq!(by("four")["reflections"], 4);
        assert_eq!(by("four")["parity"], 0);
        assert_eq!(by("three")["handedness"], -1);
        assert_eq!(by("four")["handedness"], 1);
        let result = &check["assertions"][0];
        assert_eq!(result["status"], "pass", "{result}");
        assert_eq!(result["value"]["reflections"], json!([3, 4]));
        // Opposite handedness leaves no rotation between the two images: an
        // unreflected beam leaving along −y like the three-mirror branch.
        let mirrors = ["M4", "M5", "M6", "M7", "D2"];
        bench.elements.retain(|e| !mirrors.contains(&e.id.as_str()));
        bench
            .connections
            .retain(|c| c.from != "S2" && !mirrors.contains(&c.from.as_str()));
        bench
            .elements
            .push(mount_element("D2", DETECTOR, [400., 200., 100.], ny));
        bench.connections.push(Connection {
            from: "S2".into(),
            port: "out".into(),
            to: "D2".into(),
        });
        bench.sources[1].position = [400., 300., 100.];
        bench.sources[1].direction = ny;
        bench.assertions[0].kind = AssertionKind::ImageRotation;
        bench.assertions[0].unit = AssertionUnit::Rad;
        bench.assertions[0].target = 0.;
        let check = lab_check(&bench, &system).unwrap();
        let result = &check["assertions"][0];
        assert_eq!(result["status"], "unreachable");
        assert_eq!(result["break"]["reason"], "opposite_handedness");
    }

    #[test]
    fn tilted_facet_reflection_mirrors_the_image_and_sets_absolute_parity() {
        let system = System::default();
        let x = [1., 0., 0.];
        // A facet at 60° to the mirror plane: the turn keeps the ray's sign along
        // the element axis, so only the facet normal identifies the reflection.
        let (c, s) = (0.5, 3f64.sqrt() / 2.);
        let mut m = mount_element("M", MIRROR, [200., 100., 100.], [-1., 0., 0.]);
        m.regions = vec![Region {
            port: "F".into(),
            shape: RegionShape::Rect {
                center: [0., 0.],
                size: [50., 50.],
            },
            transmittance: 1.,
            normal: Some([c, s, 0.]),
        }];
        let n = rotate([c, s, 0.], m.rotation());
        let out = sub(x, scale(n, 2. * dot(x, n)));
        let axis = rotate(x, m.rotation());
        assert!(dot(x, axis) * dot(out, axis) > 0.);
        let detector = mount_element(
            "D",
            DETECTOR,
            add([200., 100., 100.], scale(out, 150.)),
            out,
        );
        let mut bench = relay_bench(
            &[("S1", [100., 100., 100.], x)],
            vec![
                m,
                Element {
                    mount: None,
                    ..detector
                },
            ],
            &[("S1", "out", "M"), ("M", "F", "D")],
        );
        bench.branches = vec![route("one", "S1", "D")];
        let parity = |target| Assertion {
            id: "odd".into(),
            kind: AssertionKind::Parity,
            branches: ["one".into(), "one".into()],
            target,
            unit: AssertionUnit::Count,
            tolerance: 0.,
            band_nm: None,
            quantity: None,
        };
        bench.assertions = vec![parity(1.)];
        let check = lab_check(&bench, &system).unwrap();
        let reading = &check["handedness"][0];
        assert_eq!(reading["reflections"], 1, "{reading}");
        assert_eq!(reading["handedness"], -1);
        let result = &check["assertions"][0];
        assert_eq!(result["status"], "pass", "{result}");
        assert_eq!(result["value"]["parity"], 1);
        // The same branch expected even fails.
        bench.assertions = vec![parity(0.)];
        assert_eq!(
            lab_check(&bench, &system).unwrap()["assertions"][0]["status"],
            "fail"
        );
        // Other kinds still need two distinct branches.
        bench.assertions[0].kind = AssertionKind::ImageRotation;
        bench.assertions[0].unit = AssertionUnit::Rad;
        assert!(lab_check(&bench, &system).is_err());
    }

    #[test]
    fn out_of_plane_periscope_rotates_the_image_a_quarter_turn() {
        let system = System::default();
        let (x, y, z) = ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]);
        let mut bench = relay_bench(
            &[("S1", [100., 100., 100.], x), ("S2", [300., 100., 100.], y)],
            vec![
                mirror("M1", [200., 100., 100.], x, z),
                // Upper mirrors without posts, which would stand in the vertical leg.
                Element {
                    mount: None,
                    ..mirror("M2", [200., 100., 200.], z, y)
                },
                mount_element("D1", DETECTOR, [200., 250., 200.], y),
                mirror("M3", [300., 200., 100.], y, z),
                Element {
                    mount: None,
                    ..mirror("M4", [300., 200., 200.], z, y)
                },
                mount_element("D2", DETECTOR, [300., 300., 200.], y),
            ],
            &[
                ("S1", "out", "M1"),
                ("M1", "reflected", "M2"),
                ("M2", "reflected", "D1"),
                ("S2", "out", "M3"),
                ("M3", "reflected", "M4"),
                ("M4", "reflected", "D2"),
            ],
        );
        bench.branches = vec![route("turned", "S1", "D1"), route("straight", "S2", "D2")];
        let rotation = |target: f64, unit: AssertionUnit| Assertion {
            id: format!("rotation_{}", unit.symbol()),
            kind: AssertionKind::ImageRotation,
            branches: ["turned".into(), "straight".into()],
            target,
            unit,
            tolerance: 1e-9,
            band_nm: None,
            quantity: None,
        };
        bench.assertions = vec![
            rotation(PI / 2., AssertionUnit::Rad),
            rotation(PI / 2. * 1000. - 1e6 * TAU, AssertionUnit::Mrad),
        ];
        let check = lab_check(&bench, &system).unwrap();
        for result in check["assertions"].as_array().unwrap() {
            assert_eq!(result["status"], "pass", "{result}");
        }
        assert!((check["assertions"][0]["value"].as_f64().unwrap() - PI / 2.).abs() < 1e-9);
        assert!((check["assertions"][1]["value"].as_f64().unwrap() - PI * 500.).abs() < 1e-6);
        let readings = check["handedness"].as_array().unwrap();
        assert!(
            readings
                .iter()
                .all(|r| r["reflections"] == 2 && r["handedness"] == 1)
        );
        // The quarter turn is signed: the straight periscope sees the turned one at −90°.
        bench.assertions = vec![Assertion {
            branches: ["straight".into(), "turned".into()],
            ..rotation(-PI / 2., AssertionUnit::Rad)
        }];
        assert_eq!(
            lab_check(&bench, &system).unwrap()["assertions"][0]["status"],
            "pass"
        );
    }

    #[test]
    fn lens_group_relay_uses_the_prescription_matrix_once() {
        let system = System::default();
        let mut group = Element {
            kind: Kind::LensGroup,
            rotation: Some([0.; 3]),
            mount: None,
            ..mount_element("G", LENS, [200., 100., 100.], [1., 0., 0.])
        };
        group.reflectance = 0.;
        group.transmittance = 1.;
        group.aperture = 10.;
        group.body_radius = 12.;
        let mut bench = relay_bench(
            &[("S1", [100., 100., 100.], [1., 0., 0.])],
            vec![
                group,
                mount_element("D", DETECTOR, [300., 100., 100.], [1., 0., 0.]),
            ],
            &[("S1", "out", "G"), ("G", "out", "D")],
        );
        bench.sources[0].rings = 0;
        bench.sources[0].beam_radius = 0.;
        bench.branches = vec![route("relay", "S1", "D")];
        bench.relays = vec![Relay {
            id: "group".into(),
            branch: "relay".into(),
            object_distance_mm: 5.,
            image_distance_mm: 7.,
            pupil_stop: None,
            pupil_target: None,
            checks: vec![RelayCheck {
                kind: RelayCheckKind::Strict4f,
                target: 0.,
                tolerance: 1.,
            }],
        }];
        let lab = lab_check(&bench, &system).unwrap();
        let reading = &lab["relays"][0];
        assert!(reading.get("break").is_none(), "{reading}");
        let w = bench.sources[0].wavelength;
        let last = system.surfaces.len() - 1;
        let (group_matrix, _) = system.matrix(w, 0, system.surfaces.len()).unwrap();
        let thickness: f64 = system.surfaces[..last].iter().map(|s| s.thickness).sum();
        let mut object = Matrix::identity();
        object.transfer(105., 1.);
        let mut expected = relay::compose(group_matrix, object);
        expected.transfer(100. - thickness + 7., 1.);
        let abcd: Vec<f64> = reading["abcd"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        for (value, wanted) in abcd
            .iter()
            .zip([expected.a, expected.b, expected.c, expected.d])
        {
            assert!(
                (value - wanted).abs() < 1e-9 * wanted.abs().max(1.),
                "{abcd:?} vs {expected:?}"
            );
        }
        assert_eq!(reading["powered"].as_array().unwrap().len(), 1);
        assert!(
            (reading["powered"][0]["focal_mm"].as_f64().unwrap() + 1. / group_matrix.c).abs()
                < 1e-9
        );
        assert_eq!(
            reading["checks"][0]["break"]["reason"],
            "strict_4f_needs_two_thin_elements"
        );
        // No value, so no basis on the check, while the matrix keeps its own.
        assert!(reading["checks"][0].get("basis").is_none(), "{reading}");
        assert_eq!(
            reading["basis"],
            json!({"kind": "model", "assumptions": [{"code": "paraxial"}]}),
            "{reading}"
        );
        let card = card(&bench, &system, "relay", "group");
        assert_eq!(
            card.lines.last().unwrap(),
            "Basis model · paraxial",
            "{:?}",
            card.lines
        );
        assert_eq!(card.color, FAIL_COLOR);
    }

    #[test]
    fn four_f_relay_reads_its_matrix_spacings_and_pupil_image() {
        let system = System::default();
        let lens = |id: &str, x: f64, focal: f64| Element {
            focal_length: focal,
            ..mount_element(id, LENS, [x, 100., 100.], [1., 0., 0.])
        };
        let mut bench = relay_bench(
            &[("S1", [100., 100., 100.], [1., 0., 0.])],
            vec![
                lens("L1", 200., 100.),
                lens("L2", 500., 200.),
                mount_element("D", DETECTOR, [700., 100., 100.], [1., 0., 0.]),
            ],
            &[("S1", "out", "L1"), ("L1", "out", "L2"), ("L2", "out", "D")],
        );
        bench.branches = vec![route("relay", "S1", "D")];
        let check = |kind, target| RelayCheck {
            kind,
            target,
            tolerance: 1e-9,
        };
        bench.relays = vec![Relay {
            id: "4f".into(),
            branch: "relay".into(),
            object_distance_mm: 0.,
            image_distance_mm: 0.,
            pupil_stop: None,
            pupil_target: None,
            checks: vec![
                check(RelayCheckKind::Conjugate, 0.),
                check(RelayCheckKind::Magnification, -2.),
                check(RelayCheckKind::Afocal, 0.),
                check(RelayCheckKind::Strict4f, 0.),
            ],
        }];
        let lab = lab_check(&bench, &system).unwrap();
        let reading = &lab["relays"][0];
        let abcd: Vec<f64> = reading["abcd"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        for (value, expected) in abcd.iter().zip([-2., 0., 0., -0.5]) {
            assert!((value - expected).abs() < 1e-12, "{abcd:?}");
        }
        assert!(
            reading["checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["passed"] == true),
            "{reading}"
        );
        // Every value is paraxial with the thin lenses' principal planes at
        // their element planes.
        assert!(reading["checks"].as_array().unwrap().iter().all(|c| c["basis"] == json!({
            "kind": "model",
            "assumptions": [{"code": "paraxial"}, {"code": "thin_lens", "elements": ["L1", "L2"]}],
        })), "{reading}");
        // L1's plane images 600 mm past L2 (1/v = 1/200 − 1/300), 400 mm past D.
        assert!((reading["pupil_image_mm"].as_f64().unwrap() - 400.).abs() < 1e-9);
        assert!(
            !lab["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == "relay_check")
        );
        // A 1 mm spacing error between the lenses.
        bench.elements[1].position[0] += 1.;
        let lab = lab_check(&bench, &system).unwrap();
        let strict = &lab["relays"][0]["checks"][3];
        assert_eq!(strict["passed"], false);
        assert!(
            (strict["value"].as_f64().unwrap() - 1.).abs() < 1e-9,
            "{strict}"
        );
        let deviations: Vec<f64> = strict["deviations_mm"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        for (value, expected) in deviations.iter().zip([0., 1., -1.]) {
            assert!((value - expected).abs() < 1e-9, "{deviations:?}");
        }
        // L1 → image now 600 − 401 + 1/(1/200 − 1/301) analytically.
        let v = 1. / (1. / 200. - 1. / 301.);
        assert!((lab["relays"][0]["pupil_image_mm"].as_f64().unwrap() - (v - 199.)).abs() < 1e-9);
        assert!(
            lab["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == "relay_check")
        );
        let failed = card(&bench, &system, "relay", "4f");
        assert_eq!(failed.color, FAIL_COLOR);
        assert!(
            failed
                .lines
                .iter()
                .any(|l| l.starts_with("Strict 4f 1 · ") && l.ends_with(" · fail")),
            "{:?}",
            failed.lines
        );
        bench.elements[1].position[0] -= 1.;
        // An aperture at the object plane (the source, 100 mm before L1) is
        // imaged onto D: matrix [−2, 0; 0, −0.5] from it, so 0 mm past D.
        bench.relays[0].pupil_stop = Some("S1".into());
        let lab = lab_check(&bench, &system).unwrap();
        let reading = &lab["relays"][0];
        assert_eq!(reading["pupil_stop"], "S1");
        assert_eq!(reading["pupil_target"], "D");
        assert!(
            reading["pupil_image_mm"].as_f64().unwrap().abs() < 1e-9,
            "{reading}"
        );
        // The drawing card carries the matrix, spacings, conjugate pupil and checks.
        let shown = card(&bench, &system, "relay", "4f");
        assert_eq!(shown.color, PASS_COLOR);
        assert_eq!(
            shown.lines,
            [
                "Relay 4f · relay · 532 nm",
                "ABCD [-2 0 mm; 0 /mm -0.5]",
                "L1 f 100 @ 100 → L2 f 200 @ 300 mm · image +200 mm",
                "Pupil S1 → 0 mm from D",
                "Conjugate B 0 · target 0 ±1.000000e-9 · pass",
                "Magnification A -2 · target -2 ±1.000000e-9 · pass",
                "Afocal C 0 · target 0 ±1.000000e-9 · pass",
                "Strict 4f 0 · target 0 ±1.000000e-9 · pass",
                "Basis model · paraxial · thin_lens L1, L2",
            ]
        );
        // Without checks the relay only reports its readings, in slate.
        let mut unchecked = bench.clone();
        unchecked.relays[0].checks.clear();
        let lab = lab_check(&unchecked, &system).unwrap();
        assert_eq!(lab["relays"][0]["checks"], json!([]));
        assert!(
            !lab["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == "relay_check"),
            "{}",
            lab["issues"]
        );
        let quiet = card(&unchecked, &system, "relay", "4f");
        assert_eq!(quiet.color, FOLD_COLOR);
        assert_eq!(quiet.lines.len(), 5, "{:?}", quiet.lines);
        assert_eq!(quiet.lines[1], "ABCD [-2 0 mm; 0 /mm -0.5]");
        unchecked.relays[0].checks = vec![check(RelayCheckKind::Afocal, 0.); 9];
        assert!(lab_check(&unchecked, &system).is_err());
        // Measured from L2 instead: its image lies f2 = 200 mm past L2.
        bench.relays[0].pupil_target = Some("L2".into());
        let lab = lab_check(&bench, &system).unwrap();
        assert!((lab["relays"][0]["pupil_image_mm"].as_f64().unwrap() - 200.).abs() < 1e-9);
        // A target upstream of the stop has no forward conjugate.
        bench.relays[0].pupil_stop = Some("L2".into());
        bench.relays[0].pupil_target = Some("L1".into());
        let lab = lab_check(&bench, &system).unwrap();
        assert_eq!(lab["relays"][0]["break"]["reason"], "pupil_undefined");
        assert!(
            lab["relays"][0]["checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["passed"] == true)
        );
        // The matrix is still read; only the pupil is missing.
        let undefined = card(&bench, &system, "relay", "4f");
        assert_eq!(undefined.color, FAIL_COLOR);
        let detail = lab["relays"][0]["break"]["detail"].as_str().unwrap();
        assert_eq!(
            undefined.lines[1], "ABCD [-2 0 mm; 0 /mm -0.5]",
            "{:?}",
            undefined.lines
        );
        assert_eq!(
            undefined.lines[3],
            format!("Pupil unavailable · {detail}"),
            "{:?}",
            undefined.lines
        );
        assert!(
            !undefined
                .lines
                .iter()
                .any(|l| l.contains("ABCD unavailable")),
            "{:?}",
            undefined.lines
        );
        assert_eq!(lab["passed"], false);
        assert!(
            lab["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == "relay_pupil")
        );
        assert!(
            super::check(&bench, &system)
                .unwrap()
                .require_pass()
                .is_err()
        );
        bench.relays[0].pupil_stop = None;
        bench.relays[0].pupil_target = None;
        // Ending the branch at L2 with the image plane 200 mm past it still
        // applies L2: T(200)·L(200)·T(300)·L(100)·T(100) = [−2, 0; 0, −0.5].
        let mut ending = bench.clone();
        ending.branches[0].to = Port::new("L2", "in");
        ending.relays[0].image_distance_mm = 200.;
        let lab = lab_check(&ending, &system).unwrap();
        let reading = &lab["relays"][0];
        let abcd: Vec<f64> = reading["abcd"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        for (value, expected) in abcd.iter().zip([-2., 0., 0., -0.5]) {
            assert!((value - expected).abs() < 1e-12, "{abcd:?}");
        }
        assert_eq!(reading["powered"].as_array().unwrap().len(), 2);
        assert!(
            reading["checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["passed"] == true),
            "{reading}"
        );
        // Only magnification takes a target: B, C and the 4f spacings are always 0.
        for kind in [
            RelayCheckKind::Conjugate,
            RelayCheckKind::Afocal,
            RelayCheckKind::Strict4f,
        ] {
            let mut shifted = bench.clone();
            shifted.relays[0].checks = vec![check(kind, 1.)];
            assert!(lab_check(&shifted, &system).is_err(), "{kind:?}");
        }
        // Three powered elements cannot be a strict 4f.
        bench.elements.push(lens("L3", 600., 50.));
        bench.connections.retain(|c| c.from != "L2");
        bench.connections.push(Connection {
            from: "L2".into(),
            port: "out".into(),
            to: "L3".into(),
        });
        bench.connections.push(Connection {
            from: "L3".into(),
            port: "out".into(),
            to: "D".into(),
        });
        let lab = lab_check(&bench, &system).unwrap();
        assert_eq!(
            lab["relays"][0]["checks"][3]["break"]["reason"],
            "strict_4f_needs_two_thin_elements"
        );
        // An unknown branch is rejected at validation.
        bench.relays[0].branch = "missing".into();
        assert!(lab_check(&bench, &system).is_err());
    }

    #[test]
    fn periscope_heights_are_pending_only_through_their_layer() {
        let system = System::default();
        let on = |mut e: Element, layer: Option<&str>| {
            e.layer = layer.map(Into::into);
            e
        };
        let mut bench = layered_bench(
            vec![
                on(aimed("PL", MIRROR, [300., 200., 60.]), Some("H1")),
                on(aimed("PU", MIRROR, [310., 215., 100.]), Some("H2")),
                on(
                    mount_element("D", DETECTOR, [500., 200., 145.], [1., 0., 0.]),
                    Some("H2"),
                ),
            ],
            &[
                ("S1", "out", "PL"),
                ("PL", "reflected", "PU"),
                ("PU", "reflected", "D"),
            ],
        );
        bench.layers = vec![
            HeightLayer {
                id: "H1".into(),
                height_mm: 75.,
            },
            HeightLayer {
                id: "H2".into(),
                height_mm: 145.,
            },
        ];
        bench.periscopes = vec![Periscope {
            id: "P".into(),
            lower: "PL".into(),
            upper: "PU".into(),
        }];
        let pending = |target: &str, field| PendingInput {
            target: target.into(),
            field,
            note: None,
        };
        // Through its layer the upper mirror's height is the user's to enter;
        // the app's confirmation moves the layer and its members (src/bench.ts),
        // and aiming keeps that height.
        let mut marked = bench.clone();
        marked.pending = vec![
            pending("PU", PendingField::PositionZ),
            pending("PL", PendingField::PositionX),
        ];
        marked.validate_draft().unwrap();
        let mut confirmed = bench.clone();
        confirmed.layers[1].height_mm = 155.;
        for e in confirmed
            .elements
            .iter_mut()
            .filter(|e| e.layer.as_deref() == Some("H2"))
        {
            e.position[2] = 155.;
        }
        let result = aim(&confirmed, false, None, &system).unwrap();
        assert!(result.ok, "{:?}", result.conflicts);
        assert_eq!(
            result
                .bench
                .elements
                .iter()
                .find(|e| e.id == "PU")
                .unwrap()
                .position,
            [300., 200., 155.]
        );
        // Aiming places the upper mirror over the lower one, and a mirror off
        // any layer at the height of the beam it meets: no input can hold.
        let mut off_layer = bench.clone();
        off_layer
            .elements
            .iter_mut()
            .find(|e| e.id == "PU")
            .unwrap()
            .layer = None;
        for (scene, input) in [
            (&bench, pending("PU", PendingField::PositionX)),
            (&bench, pending("PU", PendingField::PositionY)),
            (&off_layer, pending("PU", PendingField::PositionZ)),
        ] {
            let mut b = scene.clone();
            b.pending = vec![input];
            assert!(
                b.validate_draft()
                    .unwrap_err()
                    .contains("auto-aim places periscope mirror PU")
            );
        }
    }

    #[test]
    fn periscope_raises_the_beam_between_layers_with_exact_clearance() {
        let system = System::default();
        let on = |mut e: Element, layer: &str| {
            e.layer = Some(layer.into());
            e
        };
        let d = 20.;
        let mut bench = layered_bench(
            vec![
                on(aimed("PL", MIRROR, [300., 200., 60.]), "H1"),
                on(aimed("PU", MIRROR, [310., 215., 100.]), "H2"),
                on(
                    mount_element("D", DETECTOR, [500., 200., 100.], [1., 0., 0.]),
                    "H2",
                ),
                on(
                    mount_element("O", DETECTOR, [300. + d, 200., 90.], [1., 0., 0.]),
                    "H1",
                ),
            ],
            &[
                ("S1", "out", "PL"),
                ("PL", "reflected", "PU"),
                ("PU", "reflected", "D"),
            ],
        );
        bench.layers = vec![
            HeightLayer {
                id: "H1".into(),
                height_mm: 75.,
            },
            HeightLayer {
                id: "H2".into(),
                height_mm: 145.,
            },
        ];
        bench.periscopes = vec![Periscope {
            id: "P".into(),
            lower: "PL".into(),
            upper: "PU".into(),
        }];
        let result = aim(&bench, false, None, &system).unwrap();
        assert!(result.ok, "{:?} {:?}", result.conflicts, result.warnings);
        let placed = |id: &str| {
            result
                .bench
                .elements
                .iter()
                .find(|e| e.id == id)
                .unwrap()
                .position
        };
        assert_eq!(placed("PL"), [300., 200., 75.]);
        assert_eq!(placed("PU"), [300., 200., 145.]);
        assert_eq!(placed("D")[2], 145.);
        assert_eq!(placed("O")[2], 75.);
        let reading = &result.periscopes[0];
        assert!(reading.interruption.is_none());
        assert!((reading.exit_height_mm.unwrap() - 145.).abs() < 1e-9);
        let direction = reading.exit_direction.unwrap();
        assert!(
            (direction[0] - 1.).abs() < 1e-9
                && direction[1].abs() < 1e-9
                && direction[2].abs() < 1e-9
        );
        // O faces +x 20 mm from the vertical leg: its body disc is 20 mm away;
        // its 25 mm mount box (top at 75 − 18 mm) is 7.5 mm out and 18 mm below
        // the leg's foot, which is nearer.
        let table = result.bench.origin[2];
        let mount_gap = (d - 12.5_f64).hypot(75. - (75. - 18. - table) - table);
        assert!(mount_gap < d);
        // The receiver D's optical face is exempt, but its mount (top 18 mm below the
        // exit beam where it lands) still counts, and is nearer than O.
        assert!(
            (reading.min_clearance_mm.unwrap() - 18.).abs() < 1e-9,
            "{:?}",
            reading.min_clearance_mm
        );
        assert_eq!(reading.nearest_element.as_deref(), Some("D"));
        assert!(reading.conflicts.is_empty());
        let trace = result.bench.trace(&system).unwrap();
        let mut bare = result.bench.clone();
        bare.elements[2].mount = None;
        let without_d = &layout::periscopes(
            &bare,
            &system,
            &center_trace(&bare, &system).unwrap(),
            &bare.trace(&system).unwrap(),
        )[0];
        assert!(
            (without_d.min_clearance_mm.unwrap() - mount_gap).abs() < 1e-9,
            "{:?}",
            without_d.min_clearance_mm
        );
        assert_eq!(without_d.nearest_element.as_deref(), Some("O"));
        let detected = trace
            .terminations
            .iter()
            .find(|t| t.reason == "detected")
            .unwrap();
        assert_eq!(detected.element.as_deref(), Some("D"));
        // Within the 0.7 mm beam radius of the vertical leg: a conflict.
        let mut close = result.bench.clone();
        close.elements[3].position[0] = 300.3;
        let readings = layout::periscopes(
            &close,
            &system,
            &center_trace(&close, &system).unwrap(),
            &close.trace(&system).unwrap(),
        );
        assert!((readings[0].min_clearance_mm.unwrap() - 0.3).abs() < 1e-9);
        assert_eq!(readings[0].conflicts, ["O"]);
        let check = lab_check(&close, &system).unwrap();
        assert!(
            check["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["code"] == "periscope_clearance")
        );
        // A mount standing in the exit leg blocks it: reported, and counted at zero gap.
        let mut blocked = result.bench.clone();
        blocked.elements.push(mount_element(
            "B",
            DETECTOR,
            [400., 200., 200.],
            [1., 0., 0.],
        ));
        let readings = layout::periscopes(
            &blocked,
            &system,
            &center_trace(&blocked, &system).unwrap(),
            &blocked.trace(&system).unwrap(),
        );
        let stop = readings[0].interruption.as_ref().unwrap();
        assert_eq!(
            (stop.element.as_str(), stop.reason.as_str()),
            ("PU", "periscope_leg_blocked")
        );
        assert!(
            readings[0].conflicts.contains(&"B".to_string()),
            "{:?}",
            readings[0].conflicts
        );
        // Solid bodies: a 10 mm slab facing +y whose front face is 10.5 mm beside the
        // exit leg reaches to 0.5 mm of it (a disc model would say 10.5 mm).
        let mut slab = result.bench.clone();
        let mut b = plate("B", [400., 210.5, 145.], [0., 1., 0.], 10.);
        (b.mount, b.body_radius) = (None, 18.);
        slab.elements.push(b);
        let readings = layout::periscopes(
            &slab,
            &system,
            &center_trace(&slab, &system).unwrap(),
            &slab.trace(&system).unwrap(),
        );
        assert!(
            (readings[0].min_clearance_mm.unwrap() - 0.5).abs() < 1e-9,
            "{:?}",
            readings[0].min_clearance_mm
        );
        assert_eq!(readings[0].conflicts, ["B"]);
        // Review counterexample: a slab across the exit leg whose back face the tracer
        // strikes first is touching it, even though its front face is 10 mm on.
        let mut across = result.bench.clone();
        let mut b = plate("B", [400., 200., 150.], [1., 0., 0.], 10.);
        (b.mount, b.body_radius) = (None, 18.);
        across.elements.push(b);
        let readings = layout::periscopes(
            &across,
            &system,
            &center_trace(&across, &system).unwrap(),
            &across.trace(&system).unwrap(),
        );
        assert_eq!(readings[0].min_clearance_mm, Some(0.));
        assert!(
            readings[0].conflicts.contains(&"B".to_string()),
            "{:?}",
            readings[0].conflicts
        );
        // Every source's legs count, whatever order the sources are listed in: O 3 mm
        // from the vertical leg clears S1's 0.7 mm beam but not S2's 4 mm one.
        let mut two = result.bench.clone();
        two.elements[3].position[0] = 303.;
        let mut wide = two.sources[0].clone();
        (wide.id, wide.beam_radius) = ("S2".into(), 4.);
        two.sources.push(wide);
        two.connections.push(Connection {
            from: "S2".into(),
            port: "out".into(),
            to: "PL".into(),
        });
        for order in [false, true] {
            if order {
                two.sources.reverse();
            }
            let readings = layout::periscopes(
                &two,
                &system,
                &center_trace(&two, &system).unwrap(),
                &two.trace(&system).unwrap(),
            );
            assert_eq!(readings[0].conflicts, ["O"], "reversed: {order}");
            assert!((readings[0].min_clearance_mm.unwrap() - 3.).abs() < 1e-9);
        }
        // A wedge is its exact slab, not the cylinder around its thick edge (review 3):
        // 20 mm, 45°, facing +y; the exit axis is at local x = −38.5, z = 18.
        let mut wedged = result.bench.clone();
        let mut b = plate("B", [400., 238.5, 127.], [0., 1., 0.], 20.);
        (b.mount, b.body_radius) = (None, 18.);
        b.substrate.as_mut().unwrap().wedge_deg = Some(45.);
        wedged.elements.push(b);
        let readings = layout::periscopes(
            &wedged,
            &system,
            &center_trace(&wedged, &system).unwrap(),
            &wedged.trace(&system).unwrap(),
        );
        let exact = 18.5_f64.hypot(18.) - 18.;
        let b_gap = {
            let mut alone = wedged.clone();
            alone
                .elements
                .retain(|e| ["PL", "PU", "B"].contains(&e.id.as_str()));
            alone.connections.retain(|c| c.to != "D");
            let trace = center_trace(&wedged, &system).unwrap();
            layout::periscopes(&alone, &system, &trace, &wedged.trace(&system).unwrap())[0]
                .min_clearance_mm
                .unwrap()
        };
        assert!((b_gap - exact).abs() < 1e-9, "{b_gap} vs {exact}");
        assert!(!readings[0].conflicts.contains(&"B".to_string()));
        // One source split into two arms that both ride the periscope: the second arm
        // (y = 203) passes 0.5 mm from O, the first (y = 200) 3.5 mm.
        let mut split = result.bench.clone();
        let source_z = split.sources[0].position[2];
        let mut bs = mount_element("BS", SPLITTER, [200., 200., source_z], [-1., 1., 0.]);
        let mut turn = mount_element("M", MIRROR, [200., 203., source_z], [1., -1., 0.]);
        for e in [&mut bs, &mut turn] {
            (e.mount, e.body_radius, e.aperture) = (None, 1., 1.);
        }
        split.elements.extend([bs, turn]);
        let o = &mut split.elements[3];
        (o.mount, o.body_radius, o.layer, o.position) = (None, 18., None, [300., 203.5, 110.]);
        o.rotation = Some(normal_rotation([0., 1., 0.]));
        split.connections.retain(|c| c.to != "PL");
        split.connections.extend([
            Connection {
                from: "S1".into(),
                port: "out".into(),
                to: "BS".into(),
            },
            Connection {
                from: "BS".into(),
                port: "transmitted".into(),
                to: "PL".into(),
            },
            Connection {
                from: "BS".into(),
                port: "reflected".into(),
                to: "M".into(),
            },
            Connection {
                from: "M".into(),
                port: "reflected".into(),
                to: "PL".into(),
            },
        ]);
        let readings = layout::periscopes(
            &split,
            &system,
            &center_trace(&split, &system).unwrap(),
            &split.trace(&system).unwrap(),
        );
        assert!(
            (readings[0].min_clearance_mm.unwrap() - 0.5).abs() < 1e-9,
            "{:?}",
            readings[0].min_clearance_mm
        );
        assert_eq!(readings[0].conflicts, ["O"]);
        // A lens group's air gap is free (review 4): flat glass pieces at axial
        // [0, 5] and [95, 100] mm facing +y from y = 150; the exit axis is at 50.
        let mut grouped = result.bench.clone();
        let mut g = mount_element("G", MIRROR, [400., 150., 145.], [0., 1., 0.]);
        (g.kind, g.mount, g.body_radius, g.reflectance) = (Kind::LensGroup, None, 18., 0.);
        grouped.elements.push(g);
        let prescription = System {
            surfaces: [
                (5., "N-BK7_SCHOTT"),
                (90., "AIR"),
                (5., "N-BK7_SCHOTT"),
                (50., "AIR"),
            ]
            .map(|(thickness, material)| {
                super::super::Surface::sphere(0., thickness, material, 18.)
            })
            .into(),
            ..System::default()
        };
        let readings = layout::periscopes(
            &grouped,
            &prescription,
            &center_trace(&grouped, &prescription).unwrap(),
            &grouped.trace(&prescription).unwrap(),
        );
        assert!(
            (readings[0].min_clearance_mm.unwrap() - 18.).abs() < 1e-9,
            "{:?}",
            readings[0].min_clearance_mm
        );
        assert!(!readings[0].conflicts.contains(&"G".to_string()));
        let mut alone = grouped.clone();
        alone
            .elements
            .retain(|e| ["PL", "PU", "G"].contains(&e.id.as_str()));
        let g_gap = layout::periscopes(
            &alone,
            &prescription,
            &center_trace(&grouped, &prescription).unwrap(),
            &grouped.trace(&prescription).unwrap(),
        )[0]
        .min_clearance_mm
        .unwrap();
        assert!((g_gap - 45.).abs() < 1e-9, "{g_gap}");
        // An unlit region's zero-power probe rides the periscope too (review 4): only
        // the lit arm (y = 200, 3.5 mm from O) counts, not the dark one at y = 203.
        let mut masked = result.bench.clone();
        let mut a = mount_element("A", MASK, [200., 200., source_z], [1., 0., 0.]);
        (a.mount, a.body_radius, a.aperture) = (None, 6., 6.);
        a.regions = [("C", 0.), ("R", 3.)]
            .map(|(port, y)| Region {
                port: port.into(),
                shape: RegionShape::Circle {
                    center: [y, 0.],
                    radius: 1.,
                },
                transmittance: 1.,
                normal: None,
            })
            .into();
        masked.elements.push(a);
        let o = &mut masked.elements[3];
        (o.mount, o.body_radius, o.layer, o.position) = (None, 18., None, [300., 203.5, 110.]);
        o.rotation = Some(normal_rotation([0., 1., 0.]));
        masked.connections.retain(|c| c.to != "PL");
        masked.connections.extend([
            Connection {
                from: "S1".into(),
                port: "out".into(),
                to: "A".into(),
            },
            Connection {
                from: "A".into(),
                port: "C".into(),
                to: "PL".into(),
            },
            Connection {
                from: "A".into(),
                port: "R".into(),
                to: "PL".into(),
            },
        ]);
        let trace = center_trace(&masked, &system).unwrap();
        assert!(trace.segments.iter().any(|s| s.power == 0. && s.flight.as_ref().is_some_and(|f| f.from_element == "PL")));
        let readings =
            layout::periscopes(&masked, &system, &trace, &masked.trace(&system).unwrap());
        assert!(
            (readings[0].min_clearance_mm.unwrap() - 3.5).abs() < 1e-9,
            "{:?}",
            readings[0].min_clearance_mm
        );
        assert!(
            readings[0].conflicts.is_empty(),
            "{:?}",
            readings[0].conflicts
        );
        // Review 5: a 3 mm beam whose first ring (y = +3) lights R. R's centre ray is
        // a zero-power probe in the centre trace, but its beam carries power in the
        // full trace, so its leg at y = 203 counts: 0.5 mm from O.
        let mut ringed = masked.clone();
        (ringed.sources[0].beam_radius, ringed.sources[0].rings) = (3., 1);
        let readings = layout::periscopes(
            &ringed,
            &system,
            &center_trace(&ringed, &system).unwrap(),
            &ringed.trace(&system).unwrap(),
        );
        assert!(
            (readings[0].min_clearance_mm.unwrap() - 0.5).abs() < 1e-9,
            "{:?}",
            readings[0].min_clearance_mm
        );
        assert_eq!(readings[0].conflicts, ["O"]);
        // Review 5: curved glass is its real shape. A plano-convex group (R = +20,
        // 15 mm BK7) facing +y from (400, 199, 128): the exit axis is at local
        // x = 1, z = 17, and its nearest glass is on the sphere, √(19² + 17²) − 20 away.
        let mut convex = result.bench.clone();
        let mut g = mount_element("G", MIRROR, [400., 199., 128.], [0., 1., 0.]);
        (g.kind, g.mount, g.body_radius, g.aperture, g.reflectance) =
            (Kind::LensGroup, None, 18., 18., 0.);
        convex.elements.push(g);
        let lens = System {
            surfaces: vec![
                super::super::Surface::sphere(20., 15., "N-BK7_SCHOTT", 18.),
                super::super::Surface::sphere(0., 50., "AIR", 18.),
            ],
            ..System::default()
        };
        let mut alone = convex.clone();
        alone
            .elements
            .retain(|e| ["PL", "PU", "G"].contains(&e.id.as_str()));
        let readings = layout::periscopes(
            &alone,
            &lens,
            &center_trace(&convex, &lens).unwrap(),
            &convex.trace(&lens).unwrap(),
        );
        let exact = 19_f64.hypot(17.) - 20.;
        assert!(
            (readings[0].min_clearance_mm.unwrap() - exact).abs() < 1e-9,
            "{:?} vs {exact}",
            readings[0].min_clearance_mm
        );
        assert!(readings[0].conflicts.is_empty());
        // Review 6: glass that is not a solid of revolution. A cylindrical first
        // face (toroid with flat revolution) bulging along local z only, at
        // (400, 200.5, 145) facing +y: the exit axis runs at local x = −0.5, z = 0,
        // and all glass has x ≥ 0 with the vertex (0, 0, 0) on it, so 0.5 mm away,
        // inside the 0.7 mm beam.
        let mut cylinder = result.bench.clone();
        let mut g = mount_element("G", MIRROR, [400., 200.5, 145.], [0., 1., 0.]);
        (g.kind, g.mount, g.body_radius, g.aperture, g.reflectance) =
            (Kind::LensGroup, None, 18., 18., 0.);
        cylinder.elements.push(g);
        let mut front = super::super::Surface::sphere(20., 15., "N-BK7_SCHOTT", 18.);
        front.toroid = Some(super::super::Toroid {
            revolution: 0.,
            even_y: vec![],
        });
        let lens = System {
            surfaces: vec![front, super::super::Surface::sphere(0., 50., "AIR", 18.)],
            ..System::default()
        };
        let readings = layout::periscopes(
            &cylinder,
            &lens,
            &center_trace(&cylinder, &lens).unwrap(),
            &cylinder.trace(&lens).unwrap(),
        );
        assert!(
            (readings[0].min_clearance_mm.unwrap() - 0.5).abs() < 1e-9,
            "{:?}",
            readings[0].min_clearance_mm
        );
        assert!(
            readings[0].conflicts.contains(&"G".to_string()),
            "{:?}",
            readings[0].conflicts
        );
        // Review 7: a cylindrical face of radius 10 on an 18 mm body ends at |z| = 10,
        // so the glass has flat side walls there; the exit axis at local z = −10.5
        // passes 0.5 mm from one. Such glass is not measured: the reading says so,
        // and names G as a conflict (auto-aim turns the interruption into a conflict).
        let mut walled = result.bench.clone();
        let mut g = mount_element("G", MIRROR, [280., 200., 155.5], [1., 0., 0.]);
        (g.kind, g.mount, g.body_radius, g.aperture, g.reflectance) =
            (Kind::LensGroup, None, 18., 9., 0.);
        walled.elements.push(g);
        let mut front = super::super::Surface::sphere(10., 300., "N-BK7_SCHOTT", 9.);
        front.toroid = Some(super::super::Toroid {
            revolution: 0.,
            even_y: vec![],
        });
        let lens = System {
            surfaces: vec![front, super::super::Surface::sphere(0., 50., "AIR", 9.)],
            ..System::default()
        };
        let readings = layout::periscopes(
            &walled,
            &lens,
            &center_trace(&walled, &lens).unwrap(),
            &walled.trace(&lens).unwrap(),
        );
        let stop = readings[0].interruption.as_ref().unwrap();
        assert_eq!(
            (stop.element.as_str(), stop.reason.as_str()),
            ("G", "periscope_clearance_unresolved")
        );
        assert!(
            readings[0].conflicts.contains(&"G".to_string()),
            "{:?}",
            readings[0].conflicts
        );
        // A periscope mirror on a stage keeps its layer exemption.
        let mut staged_mirror = result.bench.clone();
        let pu = staged_mirror.elements[1].clone();
        staged_mirror.elements[1].layer = Some("H1".into());
        staged_mirror.stages.push(Stage {
            id: "st".into(),
            members: vec!["PU".into()],
            reference_pose: [(
                "PU".to_string(),
                StagePose {
                    position: pu.position,
                    rotation: pu.rotation(),
                },
            )]
            .into(),
            axis: [1., 0., 0.],
            travel: [-5., 5.],
            position_mm: 0.,
            pzt: None,
        });
        let kept = aim(&staged_mirror, false, None, &system).unwrap();
        assert!(kept.ok, "{:?} {:?}", kept.conflicts, kept.warnings);
        // A stage member off its layer height cannot be aimed onto it: refused.
        let mut staged = bench.clone();
        staged.stages.push(Stage {
            id: "st".into(),
            members: vec!["D".into()],
            reference_pose: [(
                "D".to_string(),
                StagePose {
                    position: [500., 200., 100.],
                    rotation: [0., 0., 0.],
                },
            )]
            .into(),
            axis: [1., 0., 0.],
            travel: [-5., 5.],
            position_mm: 0.,
            pzt: None,
        });
        let refused = aim(&staged, false, None, &system).unwrap();
        assert!(!refused.ok);
        assert!(
            refused.conflicts.contains(&Port::new("D", "layer")),
            "{:?}",
            refused.conflicts
        );
        // A target on the lower layer would leave the upper mirror tilted down: refused.
        let mut tilted = bench.clone();
        tilted.elements[2].layer = Some("H1".into());
        let refused = aim(&tilted, false, None, &system).unwrap();
        assert!(!refused.ok);
        assert!(
            refused.conflicts.contains(&Port::new("PU", "reflected")),
            "{:?}",
            refused.conflicts
        );
        assert_eq!(
            refused.periscopes[0].interruption.as_ref().unwrap().reason,
            "periscope_exit_not_level"
        );
        // A layered element off its layer height fails validation.
        let mut off = result.bench.clone();
        off.elements[2].position[2] = 140.;
        assert!(off.validate().unwrap_err().contains("layer H2"));
        let mut broken = result.bench.clone();
        broken.periscopes[0].upper = "D".into();
        assert!(broken.validate().unwrap_err().contains("Invalid periscope"));
    }

    #[test]
    fn fold_length_follows_the_four_mirror_trace() {
        let system = System::default();
        let points = [
            [300., 200., 75.],
            [300., 320., 75.],
            [420., 320., 75.],
            [420., 200., 75.],
            [600., 200., 75.],
        ];
        let mut bench = layered_bench(
            vec![
                aimed("M1", MIRROR, points[0]),
                aimed("M2", MIRROR, points[1]),
                aimed("M3", MIRROR, points[2]),
                aimed("M4", MIRROR, points[3]),
                mount_element("D", DETECTOR, points[4], [1., 0., 0.]),
            ],
            &[
                ("S1", "out", "M1"),
                ("M1", "reflected", "M2"),
                ("M2", "reflected", "M3"),
                ("M3", "reflected", "M4"),
                ("M4", "reflected", "D"),
            ],
        );
        bench.folds = vec![Branch {
            id: "delay".into(),
            from: Port::new("M1", "reflected"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        let result = aim(&bench, false, None, &system).unwrap();
        assert!(result.ok, "{:?}", result.conflicts);
        let expected: f64 = points.windows(2).map(|w| norm(sub(w[1], w[0]))).sum();
        let trace = result.bench.trace(&system).unwrap();
        let path = &trace.folds[0].paths[0];
        assert!(path.interruption.is_none(), "{:?}", path.interruption);
        assert!(
            (path.length_mm - expected).abs() < 1e-9,
            "{} vs {expected}",
            path.length_mm
        );
        let check = lab_check(&result.bench, &system).unwrap();
        let fold = &check["folds"][0];
        assert_eq!(fold["id"], "delay");
        assert!((fold["paths"][0]["length_mm"].as_f64().unwrap() - expected).abs() < 1e-9);
        // 120 + 120 + 120 + 180 mm through air: the length and OPL are both 540 mm.
        assert!((expected - 540.).abs() < 1e-9);
        let card = card(&result.bench, &system, "fold", "delay");
        assert_eq!(card.lines, ["Fold delay", "Length 540 mm · OPL 540 mm"]);
        assert_eq!(check["folds"][0]["from"], "M1");
        assert_eq!(check["folds"][0]["to"], "D");
        assert_eq!(card.color, FOLD_COLOR);
        let mut repeated = result.bench.clone();
        repeated.folds.push(repeated.folds[0].clone());
        assert!(repeated.validate().unwrap_err().contains("repeated fold"));
        let mut wrong_port = result.bench.clone();
        wrong_port.folds[0].to.port = "b".into();
        assert!(
            wrong_port
                .validate()
                .unwrap_err()
                .contains("arrives at input D / in")
        );
    }

    #[test]
    fn zone_reports_area_and_members_outside_it() {
        let system = System::default();
        let mut bench = template("mach_zehnder").unwrap();
        bench.zones = vec![
            Zone {
                id: "arms".into(),
                label: "Interferometer arms".into(),
                polygon_mm: None,
                rect: Some(ZoneRect {
                    min: [200., 150.],
                    max: [500., 400.],
                }),
                members: vec!["BS1".into(), "M1".into()],
            },
            Zone {
                id: "detect".into(),
                label: String::new(),
                polygon_mm: Some(vec![[600., 350.], [700., 350.], [700., 450.], [600., 450.]]),
                rect: None,
                members: vec![],
            },
        ];
        let zones = layout::zones(&bench);
        assert_eq!(zones[0].area_mm2, 300. * 250.);
        assert_eq!(zones[0].bounds_mm, [[200., 150.], [500., 400.]]);
        // M1 sits on the top edge (y = 400): half its mount base is outside.
        assert_eq!(zones[0].out_of_bounds, ["M1"]);
        assert_eq!(zones[1].members, ["D1"]);
        assert!(zones[1].out_of_bounds.is_empty());
        let check = lab_check(&bench, &system).unwrap();
        let issues = check["issues"].as_array().unwrap();
        assert!(
            issues
                .iter()
                .any(|i| i["code"] == "zone_bounds" && i["element_id"] == "M1")
        );
        assert_eq!(check["zones"][1]["members"][0], "D1");
        // Both zones get a footprint card on the drawing and the exported blueprint.
        let arms = card(&bench, &system, "zone", "arms");
        assert_eq!(
            arms.lines,
            [
                "Zone arms · Interferometer arms",
                "Area 75000 mm² · 300 × 250 mm",
                "Members BS1, M1",
                "Out of zone M1",
            ]
        );
        assert_eq!(arms.color, FAIL_COLOR);
        let detect = card(&bench, &system, "zone", "detect");
        assert_eq!(
            detect.lines,
            ["Zone detect", "Area 10000 mm² · 100 × 100 mm", "Members D1"]
        );
        assert_eq!(detect.color, ZONE_COLOR);
        let svg = export_blueprint(&bench, &system, "svg").unwrap()["content"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            svg.contains(r#"data-kind="zone" data-id="arms""#) && svg.contains("Area 75000 mm²")
        );
        let mut crossed = bench.clone();
        crossed.zones[1].polygon_mm =
            Some(vec![[600., 350.], [700., 450.], [700., 350.], [600., 450.]]);
        assert!(crossed.validate().unwrap_err().contains("Invalid zone"));
        let mut unknown = bench.clone();
        unknown.zones[0].members.push("nope".into());
        assert!(unknown.validate().unwrap_err().contains("Invalid zone"));
    }

    #[test]
    fn every_template_is_offered_by_the_bench_view_in_both_languages() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap();
        let view = read("src/components/BenchView.vue");
        let list = view
            .split("const TEMPLATES = [")
            .nth(1)
            .unwrap()
            .split("] as const")
            .next()
            .unwrap();
        let offered: Vec<&str> = list
            .split(',')
            .map(|n| n.trim().trim_matches('\''))
            .filter(|n| !n.is_empty())
            .collect();
        let mut expected = TEMPLATES.to_vec();
        let mut sorted = offered.clone();
        expected.sort();
        sorted.sort();
        assert_eq!(sorted, expected);
        for lang in ["src/i18n/zh-CN.ts", "src/i18n/en-US.ts"] {
            let names = read(lang);
            let names = names
                .split("        templates: {")
                .nth(1)
                .unwrap()
                .split('}')
                .next()
                .unwrap();
            for name in TEMPLATES {
                assert!(names.contains(&format!("{name}: '")), "{lang} lacks {name}");
            }
        }
    }

    fn card(bench: &Bench, system: &System, kind: &str, id: &str) -> drawing::Card {
        let mut plan = blueprint_drawing(bench, system, 4.).unwrap();
        let i = plan
            .cards
            .iter()
            .position(|c| c.kind == kind && c.id == id)
            .unwrap_or_else(|| panic!("no {kind} card {id}"));
        plan.cards.swap_remove(i)
    }

    fn on_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
        let (d, q) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
        let len = d[0].hypot(d[1]);
        let t = (q[0] * d[0] + q[1] * d[1]) / (len * len);
        (q[0] * d[1] - q[1] * d[0]).abs() / len < 1e-6 && (-1e-9..=1. + 1e-9).contains(&t)
    }

    fn inside(p: [f64; 2], corner: [f64; 2], size: [f64; 2]) -> bool {
        p[0] > corner[0] + 1e-6
            && p[0] < corner[0] + size[0] - 1e-6
            && p[1] > corner[1] + 1e-6
            && p[1] < corner[1] + size[1] - 1e-6
    }

    #[test]
    fn templates_draw_every_part_with_labels_clear_of_symbols_beams_and_each_other() {
        for name in TEMPLATES {
            assert_drawn_with_clear_labels(name, &template(name).unwrap(), 4.);
        }
    }

    #[test]
    fn a_compact_layout_with_large_text_keeps_labels_clear_and_draws_leaders() {
        // Mach–Zehnder squeezed to 30 % about its source, so parts sit about a
        // body diameter apart, with 7 mm text: labels must move off their anchors.
        let mut bench = template("mach_zehnder").unwrap();
        let center = bench.sources[0].position;
        let squeeze = |p: &mut V3| {
            for axis in 0..2 {
                p[axis] = center[axis] + 0.3 * (p[axis] - center[axis]);
            }
        };
        bench
            .elements
            .iter_mut()
            .for_each(|e| squeeze(&mut e.position));
        let plan = assert_drawn_with_clear_labels("compact mach_zehnder", &bench, 7.);
        let parts: Vec<&drawing::Symbol> = plan.symbols.iter().collect();
        let nearest = parts
            .iter()
            .flat_map(|a| {
                parts
                    .iter()
                    .filter(move |b| b.id != a.id)
                    .map(move |b| (a.center[0] - b.center[0]).hypot(a.center[1] - b.center[1]))
            })
            .fold(f64::INFINITY, f64::min);
        assert!(
            nearest < 50.,
            "layout not compact: nearest parts {nearest} mm apart"
        );
        assert!(
            plan.labels.iter().any(|l| l.leader),
            "no label needed a leader"
        );
        for label in plan.labels.iter().filter(|l| l.leader) {
            assert!(
                !inside(label.anchor, label.corner, label.size),
                "{:?}: leader drawn to a point inside its box",
                label.text
            );
        }
    }

    /// Independent geometric checks of a blueprint drawing: every part drawn
    /// and labelled, labels clear of every symbol box, of each other and of
    /// every beam, and arrows on their beams pointing downstream.
    fn assert_drawn_with_clear_labels(name: &str, bench: &Bench, text_mm: f64) -> drawing::Drawing {
        let system = System::default();
        {
            let plan = blueprint_drawing(bench, &system, text_mm).unwrap();
            let ids: Vec<&str> = bench
                .elements
                .iter()
                .map(|e| e.id.as_str())
                .chain(bench.sources.iter().map(|s| s.id.as_str()))
                .collect();
            assert_eq!(plan.symbols.len(), ids.len(), "{name}");
            for id in &ids {
                let symbol = plan
                    .symbols
                    .iter()
                    .find(|s| s.id == *id)
                    .unwrap_or_else(|| panic!("{name}: no symbol for {id}"));
                assert!(
                    symbol.strokes.iter().all(|s| s.points.len() >= 2),
                    "{name}: {id}"
                );
                assert!(
                    plan.labels.iter().any(|l| l.target == *id),
                    "{name}: {id} has no label"
                );
            }
            assert!(
                plan.beams.iter().any(|b| b.arrow.is_some()),
                "{name}: no direction arrows"
            );
            let symbol_boxes: Vec<([f64; 2], [f64; 2])> = plan
                .symbols
                .iter()
                .map(|s| {
                    s.strokes.iter().flat_map(|k| &k.points).fold(
                        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
                        |(lo, hi), p| {
                            (
                                [lo[0].min(p[0]), lo[1].min(p[1])],
                                [hi[0].max(p[0]), hi[1].max(p[1])],
                            )
                        },
                    )
                })
                .collect();
            for (i, label) in plan.labels.iter().enumerate() {
                assert_eq!(label.overlaps, 0, "{name}: label {:?} overlaps", label.text);
                // Every symbol's bounding box, the label's own target included, stays clear.
                for (symbol, (lo, hi)) in plan.symbols.iter().zip(&symbol_boxes) {
                    let apart = label.corner[0] + label.size[0] <= lo[0] + 1e-6
                        || hi[0] <= label.corner[0] + 1e-6
                        || label.corner[1] + label.size[1] <= lo[1] + 1e-6
                        || hi[1] <= label.corner[1] + 1e-6;
                    assert!(
                        apart,
                        "{name}: label {:?} covers symbol {}",
                        label.text, symbol.id
                    );
                }
                // Independent of the placer: no other label box and no beam enters this box.
                for other in &plan.labels[i + 1..] {
                    let apart = label.corner[0] + label.size[0] <= other.corner[0] + 1e-6
                        || other.corner[0] + other.size[0] <= label.corner[0] + 1e-6
                        || label.corner[1] + label.size[1] <= other.corner[1] + 1e-6
                        || other.corner[1] + other.size[1] <= label.corner[1] + 1e-6;
                    assert!(apart, "{name}: {:?} overlaps {:?}", label.text, other.text);
                }
                for beam in &plan.beams {
                    for k in 0..=400 {
                        let t = k as f64 / 400.;
                        let p = [
                            beam.from[0] + t * (beam.to[0] - beam.from[0]),
                            beam.from[1] + t * (beam.to[1] - beam.from[1]),
                        ];
                        assert!(
                            !inside(p, label.corner, label.size),
                            "{name}: a beam crosses label {:?}",
                            label.text
                        );
                    }
                }
            }
            for beam in &plan.beams {
                if let Some([tip, a, b]) = beam.arrow {
                    let (mid, d) = (
                        [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.],
                        [beam.to[0] - beam.from[0], beam.to[1] - beam.from[1]],
                    );
                    assert!(
                        (tip[0] - mid[0]) * d[0] + (tip[1] - mid[1]) * d[1] > 0.,
                        "{name}: arrow points upstream"
                    );
                    assert!(
                        on_segment(tip, beam.from, beam.to),
                        "{name}: arrow off its beam"
                    );
                }
            }
            plan
        }
    }

    #[test]
    fn branch_beams_share_the_reading_colour_in_drawing_and_legend() {
        let system = System::default();
        let bench = template("delay_line").unwrap();
        let plan = blueprint_drawing(&bench, &system, 4.).unwrap();
        let scene = bench.resolved_stages().unwrap();
        let trace = scene.as_ref().trace_resolved(&system).unwrap();
        let readings = lab_readings(scene.as_ref(), &system, &trace);
        assert!(readings.branches.len() >= 2);
        let mut colors = HashSet::new();
        for branch in &readings.branches {
            let beams: Vec<_> = plan
                .beams
                .iter()
                .filter(|b| b.branch.as_deref() == Some(branch.id.as_str()))
                .collect();
            assert!(!beams.is_empty(), "{}", branch.id);
            assert!(
                beams.iter().all(|b| b.color == branch.color),
                "{}",
                branch.id
            );
            assert!(
                plan.legend
                    .iter()
                    .any(|e| e.kind == "branch" && e.key == branch.id && e.color == branch.color)
            );
            assert!(
                colors.insert(branch.color.as_str()),
                "branches share {}",
                branch.color
            );
        }
        assert!(
            plan.beams
                .iter()
                .filter(|b| b.branch.is_none())
                .all(|b| b.color == drawing::NEUTRAL)
        );
    }

    #[test]
    fn branch_style_colours_and_dots_beams_legend_readings_and_svg() {
        let system = System::default();
        let mut bench = template("delay_line").unwrap();
        let id = bench.branches[0].id.clone();
        bench.branches[0].style = Some(BranchStyle {
            color: Some("#1a2b3c".into()),
            dash: true,
        });
        let plan = blueprint_drawing(&bench, &system, 4.).unwrap();
        let beams: Vec<_> = plan
            .beams
            .iter()
            .filter(|b| b.branch.as_deref() == Some(id.as_str()))
            .collect();
        assert!(!beams.is_empty());
        for beam in &beams {
            assert_eq!(beam.color, "#1a2b3c");
            assert_eq!(
                beam.dash,
                if beam.dashed {
                    vec![3., 1.2, 0.8, 1.2]
                } else {
                    vec![0.8, 1.2]
                }
            );
        }
        let entry = plan
            .legend
            .iter()
            .find(|e| e.kind == "branch" && e.key == id)
            .unwrap();
        assert_eq!(
            (entry.color.as_str(), entry.dash.as_slice()),
            ("#1a2b3c", [0.8, 1.2].as_slice())
        );
        // Other branches keep their palette colour and solid lines.
        assert!(
            plan.beams
                .iter()
                .filter(|b| b.branch.is_some() && b.branch.as_deref() != Some(id.as_str()))
                .all(|b| b.color != "#1a2b3c" && b.dash.iter().all(|_| b.dashed))
        );
        let scene = bench.resolved_stages().unwrap();
        let trace = scene.as_ref().trace_resolved(&system).unwrap();
        let readings = lab_readings(scene.as_ref(), &system, &trace);
        assert_eq!(
            readings.branches.iter().find(|b| b.id == id).unwrap().color,
            "#1a2b3c"
        );
        assert!(
            plan.cards
                .iter()
                .any(|c| c.kind == "branch" && c.id == id && c.color == "#1a2b3c")
        );
        let svg = export_blueprint(&bench, &system, "svg").unwrap()["content"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            svg.contains(r##"stroke="#1a2b3c""##) && svg.contains(r#"stroke-dasharray="0.8 1.2""#),
            "{svg}"
        );
        for bad in ["#12345", "1a2b3c4", "#12345g"] {
            bench.branches[0].style = Some(BranchStyle {
                color: Some(bad.into()),
                dash: false,
            });
            let error = bench.validate().unwrap_err();
            assert!(error.contains("Invalid branch style"), "{bad}: {error}");
        }
        let parsed: Branch = serde_json::from_value(json!({
            "id": "b", "from": {"element": "S1", "port": "out"}, "to": {"element": "D", "port": "in"},
            "style": {"color": "#00ff00"}
        })).unwrap();
        assert_eq!(parsed.style.unwrap().color.as_deref(), Some("#00ff00"));
    }

    #[test]
    fn drawing_positions_are_the_stage_solved_part_positions() {
        let system = System::default();
        let mut bench = template("delay_line").unwrap();
        let m3 = bench
            .elements
            .iter()
            .find(|e| e.id == "M3")
            .unwrap()
            .position;
        bench.stages[0].position_mm = 2.5;
        let plan = blueprint_drawing(&bench, &system, 4.).unwrap();
        let drawn = |id: &str| plan.positions.iter().find(|p| p.id == id).unwrap().position;
        // The stage carries M3 along +X; the source stays put.
        assert_eq!(drawn("M3"), [m3[0] + 2.5, m3[1], m3[2]]);
        assert_eq!(drawn(&bench.sources[0].id), bench.sources[0].position);
        assert_eq!(
            plan.positions.len(),
            bench.elements.len() + bench.sources.len()
        );
    }

    #[test]
    fn catalog_plano_convex_lenses_are_drawn_plano_with_the_flat_face_on_minus_x() {
        let system = System::default();
        let bench = template("beam_expander_focus").unwrap();
        let lenses: Vec<_> = bench
            .elements
            .iter()
            .filter(|e| e.kind == Kind::Lens && e.label.contains("LA1509"))
            .collect();
        assert!(!lenses.is_empty());
        let plan = blueprint_drawing(&bench, &system, 4.).unwrap();
        for lens in lenses {
            assert_eq!(lens.lens_form, Some(LensForm::Plano), "{}", lens.id);
            let symbol = plan.symbols.iter().find(|s| s.id == lens.id).unwrap();
            assert_eq!(
                symbol.kind,
                if lens.focal_length < 0. {
                    "plano_concave_lens"
                } else {
                    "plano_convex_lens"
                }
            );
            let n = rotate([1., 0., 0.], lens.rotation());
            let along =
                |p: &[f64; 2]| (p[0] - lens.position[0]) * n[0] + (p[1] - lens.position[1]) * n[1];
            let face = symbol
                .strokes
                .iter()
                .find(|s| s.role == drawing::Role::Face)
                .unwrap();
            assert!(
                face.points.iter().all(|p| along(p).abs() < 1e-9),
                "{}: flat face off the element plane",
                lens.id
            );
            assert!(
                symbol
                    .strokes
                    .iter()
                    .flat_map(|s| &s.points)
                    .all(|p| along(p) > -1e-9),
                "{}: glass on the −X side",
                lens.id
            );
        }
        let lenses = catalog(Some("LA1509"), None);
        assert_eq!(lenses["items"][0]["lens_form"], "plano");
        assert!(
            catalog(Some("KM100"), None)["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| i.get("lens_form").is_none())
        );
    }

    #[test]
    fn blueprint_and_gui_share_the_plate_splitter_holes_and_result_cards() {
        let system = System::default();
        let bench = template("delay_line").unwrap();
        let plan = blueprint_drawing(&bench, &system, 4.).unwrap();
        // The template's splitters are BSW10 plates (Kind::Splitter, no modelled substrate).
        let plates: Vec<_> = bench
            .elements
            .iter()
            .filter(|e| e.kind == SPLITTER.kind)
            .collect();
        assert!(!plates.is_empty() && SPLITTER.part == "BSW10");
        for e in plates {
            assert_eq!(
                plan.symbols.iter().find(|s| s.id == e.id).unwrap().kind,
                "plate_splitter",
                "{}",
                e.id
            );
        }
        let grid = Grid::new(&bench).unwrap();
        let expected = ((grid.last[0] + 1) * (grid.last[1] + 1)) as usize;
        assert_eq!(plan.holes.len(), expected);
        assert_eq!(plan.holes[0], [bench.origin[0], bench.origin[1]]);
        assert_eq!(
            plan.holes[1],
            [bench.origin[0], bench.origin[1] + bench.hole_pitch]
        );
        let svg = export_blueprint(&bench, &system, "svg").unwrap()["content"]
            .as_str()
            .unwrap()
            .to_owned();
        let holes = svg
            .split(r#"<g id="holes""#)
            .nth(1)
            .unwrap()
            .split("</g>")
            .next()
            .unwrap();
        assert_eq!(holes.matches("<circle").count(), plan.holes.len());
        assert!(!plan.cards.is_empty());
        let readings = svg
            .split(r#"<g id="lab-readings""#)
            .nth(1)
            .unwrap()
            .split("</g><g id=")
            .next()
            .unwrap();
        for card in &plan.cards {
            assert!(
                readings.contains(&format!(
                    r#"data-kind="{}" data-id="{}""#,
                    card.kind, card.id
                )),
                "{}",
                card.id
            );
            for line in &card.lines {
                assert!(readings.contains(&xml(line)), "{line}");
            }
        }
        assert_eq!(readings.matches("data-kind=").count(), plan.cards.len());
    }

    #[test]
    fn upper_layer_beams_are_dashed_and_paired_periscopes_mark_their_rise() {
        let system = System::default();
        let on = |mut e: Element, layer: &str| {
            e.layer = Some(layer.into());
            e
        };
        let mut bench = layered_bench(
            vec![
                on(aimed("PL", MIRROR, [300., 200., 60.]), "H1"),
                on(aimed("PU", MIRROR, [310., 215., 100.]), "H2"),
                on(
                    mount_element("D", DETECTOR, [500., 200., 100.], [1., 0., 0.]),
                    "H2",
                ),
            ],
            &[
                ("S1", "out", "PL"),
                ("PL", "reflected", "PU"),
                ("PU", "reflected", "D"),
            ],
        );
        bench.layers = vec![
            HeightLayer {
                id: "H1".into(),
                height_mm: 75.,
            },
            HeightLayer {
                id: "H2".into(),
                height_mm: 145.,
            },
        ];
        bench.periscopes = vec![Periscope {
            id: "P".into(),
            lower: "PL".into(),
            upper: "PU".into(),
        }];
        let result = aim(&bench, false, None, &system).unwrap();
        assert!(result.ok, "{:?}", result.conflicts);
        let plan = blueprint_drawing(&result.bench, &system, 4.).unwrap();
        let mark = &plan.periscopes[0];
        assert_eq!((mark.id.as_str(), mark.at), ("P", [300., 200.]));
        assert!((mark.rise_mm - 70.).abs() < 1e-9);
        assert!(
            plan.labels
                .iter()
                .any(|l| l.kind == "periscope" && l.target == "P" && l.text.contains("+70 mm"))
        );
        let flat: Vec<_> = plan
            .beams
            .iter()
            .filter(|b| (b.to[0] - b.from[0]).hypot(b.to[1] - b.from[1]) > 1.)
            .collect();
        let lower = flat
            .iter()
            .find(|b| on_segment([250., 200.], b.from, b.to))
            .expect("lower beam");
        assert!(!lower.dashed && lower.layer.as_deref() == Some("H1"));
        let upper = flat
            .iter()
            .find(|b| on_segment([400., 200.], b.from, b.to))
            .expect("upper beam");
        assert!(upper.dashed && upper.layer.as_deref() == Some("H2"));
        assert!(
            plan.legend
                .iter()
                .any(|e| e.kind == "layer" && e.key == "H2" && e.dashed)
        );
        let svg = export_blueprint(&result.bench, &system, "svg").unwrap()["content"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            svg.contains(r#"data-periscope="P" data-rise="70""#),
            "{svg}"
        );
    }

    #[test]
    fn pending_inputs_warn_with_their_placeholder_and_bad_entries_are_rejected() {
        let system = System::default();
        let mut bench = template("delay_line").unwrap();
        let source = bench.sources[0].id.clone();
        let input = |target: &str, field, note: Option<&str>| PendingInput {
            target: target.into(),
            field,
            note: note.map(Into::into),
        };
        bench.pending = vec![
            input("M3", PendingField::PositionX, Some("Read the rail scale")),
            input(&source, PendingField::Wavelength, None),
        ];
        bench.validate().unwrap();
        let check = lab_check(&bench, &system).unwrap();
        let pending: Vec<_> = check["issues"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["code"] == "pending_input")
            .collect();
        assert_eq!(pending.len(), 2);
        assert_eq!(
            (
                pending[0]["severity"].as_str(),
                pending[0]["element_id"].as_str()
            ),
            (Some("warning"), Some("M3"))
        );
        let x = bench
            .elements
            .iter()
            .find(|e| e.id == "M3")
            .unwrap()
            .position[0];
        assert_eq!(
            pending[0]["message"],
            format!("M3 position_x is a placeholder ({x} mm) awaiting input: Read the rail scale")
        );
        let wavelength = bench.sources[0].wavelength;
        assert_eq!(
            pending[1]["message"],
            format!("{source} wavelength is a placeholder ({wavelength} nm) awaiting input")
        );
        // A pending input never passes the bench, and clears once removed.
        assert_eq!(
            (check["passed"].as_bool(), check["pending"].as_u64()),
            (Some(false), Some(2))
        );
        let report = super::check(&bench, &system).unwrap();
        assert!(
            report
                .require_pass()
                .unwrap_err()
                .ends_with("0 errors and 2 pending inputs")
        );
        let mut supplied = bench.clone();
        supplied.pending.clear();
        assert_eq!(lab_check(&supplied, &system).unwrap()["passed"], true);
        let parsed: PendingInput = serde_json::from_value(
            json!({"target":"M3","field":"position_x","note":"Read the rail scale"}),
        )
        .unwrap();
        assert_eq!(parsed, bench.pending[0]);
        assert!(
            serde_json::from_value::<PendingInput>(json!({"target":"M3","field":"colour"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<PendingInput>(
                json!({"target":"M3","field":"aperture","value":3})
            )
            .is_err()
        );
        let plain = bench
            .elements
            .iter()
            .find(|e| e.substrate.is_none())
            .unwrap()
            .id
            .clone();
        for bad in [
            input("nope", PendingField::Aperture, None),
            input("M3", PendingField::Wavelength, None),
            input(&source, PendingField::FocalLength, None),
            input(&plain, PendingField::SubstrateThicknessMm, None),
            input("M3", PendingField::Aperture, Some(&"x".repeat(257))),
        ] {
            let mut b = bench.clone();
            b.pending = vec![bad];
            assert!(
                b.validate()
                    .unwrap_err()
                    .contains("Invalid pending input 0")
            );
        }
        let mut twice = bench.clone();
        twice
            .pending
            .push(input("M3", PendingField::PositionX, None));
        assert!(
            twice
                .validate()
                .unwrap_err()
                .contains("Invalid pending input 2")
        );
    }

    #[test]
    fn annotations_sit_by_their_element_or_branch_and_bad_references_are_rejected() {
        let system = System::default();
        let mut bench = template("delay_line").unwrap();
        let branch = bench.branches[0].id.clone();
        bench.annotations = vec![
            Annotation {
                target: AnnotationTarget::Element("M3".into()),
                text: "Clean before use".into(),
            },
            Annotation {
                target: AnnotationTarget::Branch(branch.clone()),
                text: "Reference arm".into(),
            },
        ];
        let plan = blueprint_drawing(&bench, &system, 4.).unwrap();
        let notes: Vec<_> = plan
            .labels
            .iter()
            .filter(|l| l.kind == "annotation")
            .collect();
        assert_eq!(notes.len(), 2);
        let m3 = bench
            .elements
            .iter()
            .find(|e| e.id == "M3")
            .unwrap()
            .position;
        assert_eq!(
            (notes[0].target.as_str(), notes[0].anchor),
            ("M3", [m3[0], m3[1]])
        );
        assert_eq!(
            (notes[1].target.as_str(), notes[1].text.as_str()),
            (branch.as_str(), "Reference arm")
        );
        assert!(
            plan.beams
                .iter()
                .filter(|b| b.branch.as_deref() == Some(branch.as_str()))
                .any(|b| on_segment(notes[1].anchor, b.from, b.to))
        );
        assert!(notes.iter().all(|n| n.overlaps == 0));
        let svg = export_blueprint(&bench, &system, "svg").unwrap()["content"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            svg.contains(&format!(
                r#"data-label="annotation" data-target="{branch}""#
            )) && svg.contains("Clean before use")
        );
        // The target is one object naming exactly one element or branch.
        let parsed = serde_json::from_value::<Annotation>(
            json!({"target":{"element":"M3"},"text":"Clean before use"}),
        )
        .unwrap();
        assert_eq!(parsed, bench.annotations[0]);
        for shape in [
            json!({"target":{"element":"M3","branch":branch},"text":"x"}),
            json!({"target":{},"text":"x"}),
            json!({"element":"M3","text":"x"}),
            json!({"target":{"element":"M3"}}),
        ] {
            assert!(
                serde_json::from_value::<Annotation>(shape.clone()).is_err(),
                "{shape}"
            );
        }
        let element = |id: &str| AnnotationTarget::Element(id.into());
        let note = |text: String, target: AnnotationTarget| Annotation { target, text };
        for bad in [
            note("x".into(), element("nope")),
            note("x".into(), AnnotationTarget::Branch("nope".into())),
            note(String::new(), element("M3")),
            note("x".repeat(257), element("M3")),
        ] {
            let mut b = bench.clone();
            b.annotations = vec![bad];
            assert!(b.validate().unwrap_err().contains("Invalid annotation 0"));
        }
        let mut source_note = bench.clone();
        source_note.annotations = vec![note("Laser".into(), element("S1"))];
        source_note.validate().unwrap();
        let mut many = bench.clone();
        many.annotations = vec![bench.annotations[0].clone(); 65];
        assert!(many.validate().unwrap_err().contains("annotation budget"));
    }
}
