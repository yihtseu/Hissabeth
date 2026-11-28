//! The bench drawing shared by the workbench's 2D view and the SVG blueprint:
//! oriented element symbols, beams coloured by named branch with propagation
//! arrows and height-layer line styles, periscope height marks, zones, stages,
//! rulers, a legend, assertion badges and labels placed clear of symbols and beams.
//! Every coordinate is absolute table-frame XY in millimetres (Y up).
use super::{
    bench::{Action, AnnotationTarget, Bench, Element, Face, Kind, LensForm, Trace, rotate},
    reading_number,
};
use serde::Serialize;
use std::collections::BTreeSet;

/// Colours of named branches that declare none, assigned in `bench.branches` order.
pub(crate) const PALETTE: &[&str] = &[
    "#2162a1", "#a85c00", "#7554a3", "#26734d", "#b04375", "#00838f", "#8a6d00", "#c2410c",
];
/// Beams on no named branch.
pub(crate) const NEUTRAL: &str = "#007c91";
/// A beam whose mean height is this far (mm) from the base height is drawn dashed.
const LAYER_TOLERANCE: f64 = 0.5;

/// Dash patterns along a beam (mm): an upper height layer, a dotted branch, both.
const LAYER_DASH: [f64; 2] = [3., 1.5];
const BRANCH_DOT: [f64; 2] = [0.8, 1.2];
const LAYER_AND_DOT: [f64; 4] = [3., 1.2, 0.8, 1.2];

fn dash(layer: bool, dotted: bool) -> Vec<f64> {
    match (layer, dotted) {
        (false, false) => vec![],
        (true, false) => LAYER_DASH.to_vec(),
        (false, true) => BRANCH_DOT.to_vec(),
        (true, true) => LAYER_AND_DOT.to_vec(),
    }
}

/// A named branch's colour and whether it is dotted: its declared style, else the
/// palette colour at its place in `bench.branches`. Shared by the drawing, its
/// legend and the branch readings.
pub(crate) fn branch_style(bench: &Bench, id: &str) -> (String, bool) {
    let index = bench.branches.iter().position(|b| b.id == id);
    let style = index.and_then(|i| bench.branches[i].style.as_ref());
    let color = style
        .and_then(|s| s.color.clone())
        .unwrap_or_else(|| PALETTE[index.unwrap_or(0) % PALETTE.len()].to_owned());
    (color, style.is_some_and(|s| s.dash))
}

type P = [f64; 2];

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct Drawing {
    /// Table-frame XY of the table's hole-grid origin corner, and the table size (mm).
    pub origin: P,
    pub size: P,
    pub hole_pitch: f64,
    /// Table-frame XY of every breadboard hole, column by column; empty above
    /// 25,000 holes, where the blueprint refuses to draw them.
    pub holes: Vec<P>,
    pub hole_radius: f64,
    /// Text height (mm) the labels were laid out for.
    pub text_mm: f64,
    /// Millimetre ruler ticks measured from the origin corner.
    pub rulers: Vec<Tick>,
    pub zones: Vec<ZoneMark>,
    pub stages: Vec<StageMark>,
    pub symbols: Vec<Symbol>,
    /// Unbranched beams first, then each named branch's beams in branch order
    /// (segments shared by several branches repeat under each).
    pub beams: Vec<BeamMark>,
    pub periscopes: Vec<PeriscopeMark>,
    pub labels: Vec<Label>,
    pub legend: Vec<LegendEntry>,
    pub assertions: Vec<AssertionMark>,
    /// Result cards under the table: stage positions, branch path lengths, then
    /// assertion results.
    pub cards: Vec<Card>,
    /// Table-frame position (mm) of every element and source as drawn, after
    /// stage motion: a client moves a symbol by its part's offset from here.
    pub positions: Vec<PartPosition>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct PartPosition {
    pub id: String,
    pub position: [f64; 3],
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct Card {
    /// stage, branch, assertion, zone, fold or relay.
    pub kind: &'static str,
    pub id: String,
    /// Frame and text colour: the branch colour, green/red/amber for a passing,
    /// failing or undecided check (a relay with every check passed is green), red
    /// for a zone with members outside or an interrupted fold, slate otherwise
    /// (a relay without checks included).
    pub color: String,
    pub lines: Vec<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct Tick {
    /// "x" (along the table width) or "y".
    pub axis: &'static str,
    /// Distance from the origin corner (mm).
    pub value_mm: f64,
    /// Major ticks carry a number.
    pub major: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct ZoneMark {
    pub id: String,
    pub label: String,
    pub outline: Vec<P>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct StageMark {
    pub id: String,
    /// Box around the member symbols.
    pub outline: [P; 4],
    /// The stage axis across the travel range, from the minimum to the maximum position.
    pub travel: [P; 2],
}

#[derive(Serialize, schemars::JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    /// Outline drawn on white.
    Body,
    /// Solid dark area.
    Fill,
    /// Optical face or coating, drawn heavier.
    Face,
    /// Thin hatching and marks.
    Hatch,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct Stroke {
    pub role: Role,
    pub closed: bool,
    pub points: Vec<P>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct Symbol {
    /// Element or source ID.
    pub id: String,
    /// mirror, curved_mirror, pbs (polarizing cube), plate_splitter, lens, lens_group,
    /// waveplate, polarizer, camera, aperture, blocker, slm, nd_filter, mask,
    /// tip_tilt, ground_glass or laser.
    pub kind: &'static str,
    pub center: P,
    pub strokes: Vec<Stroke>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct BeamMark {
    /// Named branch, or null for a beam on no named branch.
    pub branch: Option<String>,
    /// Ray split path of the traced segment ("0", "0/1", …), as in bench_trace.
    pub path: String,
    pub color: String,
    /// The beam runs off the base height.
    pub dashed: bool,
    /// Dash pattern (mm along the beam, empty for solid): dashes off the base
    /// height, dots for a branch styled `dash`, both combined.
    pub dash: Vec<f64>,
    /// The height layer at the beam's height, when one matches.
    pub layer: Option<String>,
    pub from: P,
    pub to: P,
    /// Propagation arrowhead at the midpoint (tip first), on beams long enough for one.
    pub arrow: Option<[P; 3]>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct PeriscopeMark {
    pub id: String,
    pub at: P,
    /// Upper minus lower mirror height (mm).
    pub rise_mm: f64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct Label {
    /// element, source, periscope, stage, zone, assertion or annotation.
    pub kind: &'static str,
    /// The element, source, periscope, stage, zone, assertion or branch it names.
    pub target: String,
    pub text: String,
    /// The point the label describes.
    pub anchor: P,
    /// Lower-left corner and size of the text box.
    pub corner: P,
    pub size: P,
    /// The box sits away from its anchor; draw a leader line.
    pub leader: bool,
    /// Overlaps with symbols, beams or other labels that no candidate position avoided.
    pub overlaps: usize,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct LegendEntry {
    /// branch, beam, layer or symbol.
    pub kind: &'static str,
    pub key: String,
    pub text: String,
    pub color: String,
    /// Upper height layer.
    pub dashed: bool,
    /// Dash pattern of the swatch (mm), as on the beams it explains.
    pub dash: Vec<f64>,
}

#[derive(Serialize, schemars::JsonSchema, Clone)]
pub(crate) struct AssertionMark {
    pub id: String,
    pub passed: bool,
    /// Status word: pass, fail, indeterminate or unreachable.
    pub text: String,
    /// The element where the assertion is evaluated.
    pub element: Option<String>,
}

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: P, s: f64) -> P {
    [a[0] * s, a[1] * s]
}
fn unit(v: P) -> Option<P> {
    let n = v[0].hypot(v[1]);
    (n > 1e-9).then(|| mul(v, 1. / n))
}
fn xy(v: [f64; 3]) -> P {
    [v[0], v[1]]
}

/// Local frame of a symbol: `n` points out of the optical face toward the light,
/// `t` along the face.
struct Frame {
    c: P,
    n: P,
    t: P,
}
impl Frame {
    fn at(&self, a: f64, b: f64) -> P {
        add(self.c, add(mul(self.n, a), mul(self.t, b)))
    }
    fn rect(&self, a: [f64; 2], b: [f64; 2], role: Role) -> Stroke {
        Stroke {
            role,
            closed: true,
            points: vec![
                self.at(a[0], b[0]),
                self.at(a[1], b[0]),
                self.at(a[1], b[1]),
                self.at(a[0], b[1]),
            ],
        }
    }
    fn line(&self, from: [f64; 2], to: [f64; 2], role: Role) -> Stroke {
        Stroke {
            role,
            closed: false,
            points: vec![self.at(from[0], from[1]), self.at(to[0], to[1])],
        }
    }
    fn poly(&self, points: impl IntoIterator<Item = [f64; 2]>, role: Role, closed: bool) -> Stroke {
        Stroke {
            role,
            closed,
            points: points.into_iter().map(|[a, b]| self.at(a, b)).collect(),
        }
    }
    /// Short ticks behind the face at `depth`, the hatching of a reflecting back.
    fn back_hatch(&self, depth: f64, half: f64) -> Vec<Stroke> {
        let step = 2. * half / 5.;
        (0..5)
            .map(|i| {
                let b = -half + (i as f64 + 0.5) * step;
                self.line(
                    [-depth, b],
                    [-depth - 0.7 * step, b - 0.7 * step],
                    Role::Hatch,
                )
            })
            .collect()
    }
}

fn circle(c: P, r: f64, role: Role) -> Stroke {
    Stroke {
        role,
        closed: true,
        points: (0..24)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 24.;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect(),
    }
}

/// The side of the element light arrives from: the XY normal flipped toward the
/// first traced beam ending on it.
fn facing(element: &Element, trace: &Trace) -> Option<P> {
    let n = unit(xy(rotate([1., 0., 0.], element.rotation())))?;
    let incoming = trace
        .segments
        .iter()
        .filter(|s| s.element.as_deref() == Some(element.id.as_str()))
        .find_map(|s| unit(sub(xy(s.to), xy(s.from))));
    Some(match incoming {
        Some(d) if d[0] * n[0] + d[1] * n[1] > 0. => mul(n, -1.),
        _ => n,
    })
}

fn element_symbol(element: &Element, trace: &Trace) -> Symbol {
    let c = xy(element.position);
    let r = element.body_radius;
    let Some(n) = facing(element, trace) else {
        // Face-up element: its outline seen from above.
        return Symbol {
            id: element.id.clone(),
            kind: kind_name(element),
            center: c,
            strokes: vec![
                circle(c, r, Role::Body),
                circle(c, element.aperture.min(r), Role::Face),
            ],
        };
    };
    let f = Frame {
        c,
        n,
        t: [-n[1], n[0]],
    };
    let thin = (0.12 * r).clamp(0.6, 3.);
    let th = element
        .substrate
        .as_ref()
        .map_or((0.25 * r).clamp(1., 8.), |s| s.thickness_mm);
    let mut strokes = Vec::new();
    match (element.kind, &element.action) {
        (_, Some(Action::Slm { segments, .. })) => {
            strokes.push(f.rect([-th, 0.], [-r, r], Role::Body));
            let columns = segments[0].clamp(1, 16);
            strokes.extend((1..columns).map(|i| {
                let b = -r + 2. * r * i as f64 / columns as f64;
                f.line([-th, b], [0., b], Role::Hatch)
            }));
            strokes.push(f.line([0., -r], [0., r], Role::Face));
        }
        (_, Some(Action::Nd { .. })) => strokes.push(f.rect([-thin, thin], [-r, r], Role::Fill)),
        (_, Some(Action::Mask { .. })) => {
            strokes.push(f.rect([-thin, thin], [-r, r], Role::Body));
            strokes.push(f.line([-thin, -0.4 * r], [thin, 0.4 * r], Role::Hatch));
            strokes.push(f.line([-thin, 0.4 * r], [thin, -0.4 * r], Role::Hatch));
        }
        (_, Some(Action::Rgg { .. })) => {
            strokes.push(f.rect([-thin, thin], [-r, r], Role::Body));
            strokes.push(f.poly(
                (0..=8).map(|i| {
                    [
                        if i % 2 == 0 { -2. * thin } else { 2. * thin },
                        -r + r * i as f64 / 4.,
                    ]
                }),
                Role::Hatch,
                false,
            ));
        }
        (Kind::Mirror, action) => {
            strokes.push(f.rect([-th, 0.], [-r, r], Role::Body));
            strokes.push(f.line([0., -r], [0., r], Role::Face));
            strokes.extend(f.back_hatch(th, r));
            if matches!(action, Some(Action::TipTilt { .. })) {
                strokes.push(f.poly(
                    [
                        [-th - 0.4 * r, -0.5 * r],
                        [-th - 0.6 * r, 0.],
                        [-th - 0.4 * r, 0.5 * r],
                    ],
                    Role::Hatch,
                    false,
                ));
            }
        }
        (Kind::CurvedMirror, _) => {
            let sag = 0.25 * r * element.focal_length.signum();
            let arc = |shift: f64| {
                (0..=12).map(move |i| {
                    let b = -r + r * i as f64 / 6.;
                    [sag * (b / r).powi(2) + shift, b]
                })
            };
            strokes.push(f.poly(
                arc(0.).chain(arc(-th).collect::<Vec<_>>().into_iter().rev()),
                Role::Body,
                true,
            ));
            strokes.push(f.poly(arc(0.), Role::Face, false));
            strokes.extend(f.back_hatch(th + sag.max(0.), r));
        }
        (Kind::Splitter, _) => {
            // The substrate lies behind the element's own +X face whichever side light
            // arrives from, as the slab trace models it.
            let n0 = unit(xy(rotate([1., 0., 0.], element.rotation()))).unwrap_or(n);
            let g = Frame {
                c,
                n: n0,
                t: [-n0[1], n0[0]],
            };
            let th = element
                .substrate
                .as_ref()
                .map_or(thin.max(1.), |s| s.thickness_mm);
            strokes.push(g.rect([-th, 0.], [-r, r], Role::Body));
            let face = match element.substrate.as_ref().and_then(|s| s.coated_face) {
                Some(Face::Back) => -th,
                _ => 0.,
            };
            strokes.push(g.line([face, -r], [face, r], Role::Face));
        }
        (Kind::Pbs, _) => {
            strokes.push(f.poly([[r, 0.], [0., r], [-r, 0.], [0., -r]], Role::Body, true));
            strokes.push(f.line([0., -r], [0., r], Role::Face));
            strokes.extend(
                [-0.5, 0., 0.5].map(|b| f.line([-0.15 * r, b * r], [0.15 * r, b * r], Role::Hatch)),
            );
        }
        (Kind::Lens, _) if element.lens_form == Some(LensForm::Plano) => {
            // Flat face on the element's own −X side, so it reads as mounted.
            let n0 = unit(xy(rotate([1., 0., 0.], element.rotation()))).unwrap_or(n);
            let g = Frame {
                c,
                n: n0,
                t: [-n0[1], n0[0]],
            };
            if element.focal_length < 0. {
                let (edge, middle) = (0.35 * r, 0.1 * r);
                strokes.push(g.poly(
                    [[0., -r], [0., r], [edge, r], [middle, 0.], [edge, -r]],
                    Role::Body,
                    true,
                ));
            } else {
                let bulge = 0.4 * r;
                strokes.push(g.poly(
                    (0..=12).map(|i| {
                        let b = -r + r * i as f64 / 6.;
                        [bulge * (1. - (b / r).powi(2)), b]
                    }),
                    Role::Body,
                    true,
                ));
            }
            strokes.push(g.line([0., -r], [0., r], Role::Face));
        }
        (Kind::Lens, _) if element.focal_length < 0. => {
            let (edge, middle) = (0.35 * r, 0.1 * r);
            strokes.push(f.poly(
                [
                    [edge, -r],
                    [middle, 0.],
                    [edge, r],
                    [-edge, r],
                    [-middle, 0.],
                    [-edge, -r],
                ],
                Role::Body,
                true,
            ));
        }
        (Kind::Lens, _) => {
            let bulge = 0.3 * r;
            let side = |sign: f64| {
                (0..=12).map(move |i| {
                    let b = -r + r * i as f64 / 6.;
                    [sign * bulge * (1. - (b / r).powi(2)), sign * b]
                })
            };
            strokes.push(f.poly(side(1.).chain(side(-1.)), Role::Body, true));
        }
        (Kind::LensGroup, _) => {
            strokes.push(f.rect([-0.6 * r, 0.6 * r], [-r, r], Role::Body));
            strokes.extend(
                [-0.3, 0.3].map(|a| f.line([a * r, -0.8 * r], [a * r, 0.8 * r], Role::Hatch)),
            );
        }
        (Kind::Waveplate, _) => {
            strokes.push(f.rect([-thin, thin], [-r, r], Role::Body));
            strokes.push(f.line([-3. * thin, 0.], [3. * thin, 0.], Role::Hatch));
        }
        (Kind::Polarizer, _) => {
            strokes.push(f.rect([-thin, thin], [-r, r], Role::Body));
            strokes.extend(
                [-0.6, -0.2, 0.2, 0.6]
                    .map(|b| f.line([-3. * thin, b * r], [3. * thin, b * r], Role::Hatch)),
            );
        }
        (Kind::Detector, _) => {
            strokes.push(f.rect([-1.2 * r, 0.], [-0.8 * r, 0.8 * r], Role::Fill));
            strokes.push(f.line([0., -r], [0., r], Role::Face));
        }
        (Kind::Aperture, _) => {
            let gap = element.aperture.min(0.9 * r);
            strokes.push(f.rect([-thin, thin], [gap, r], Role::Fill));
            strokes.push(f.rect([-thin, thin], [-r, -gap], Role::Fill));
        }
        (Kind::Blocker, _) => strokes.push(f.rect([-0.5 * r, 0.], [-r, r], Role::Fill)),
    }
    Symbol {
        id: element.id.clone(),
        kind: kind_name(element),
        center: c,
        strokes,
    }
}

fn kind_name(element: &Element) -> &'static str {
    match (&element.action, element.kind) {
        (Some(Action::Slm { .. }), _) => "slm",
        (Some(Action::Nd { .. }), _) => "nd_filter",
        (Some(Action::Mask { .. }), _) => "mask",
        (Some(Action::Rgg { .. }), _) => "ground_glass",
        (Some(Action::TipTilt { .. }), Kind::Mirror) => "tip_tilt",
        (_, Kind::Mirror) => "mirror",
        (_, Kind::CurvedMirror) => "curved_mirror",
        (_, Kind::Pbs) => "pbs",
        (_, Kind::Splitter) => "plate_splitter",
        (_, Kind::Lens) if element.lens_form == Some(LensForm::Plano) => {
            if element.focal_length < 0. {
                "plano_concave_lens"
            } else {
                "plano_convex_lens"
            }
        }
        (_, Kind::Lens) => "lens",
        (_, Kind::LensGroup) => "lens_group",
        (_, Kind::Waveplate) => "waveplate",
        (_, Kind::Polarizer) => "polarizer",
        (_, Kind::Detector) => "camera",
        (_, Kind::Aperture) => "aperture",
        (_, Kind::Blocker) => "blocker",
    }
}

fn kind_text(kind: &str) -> &'static str {
    match kind {
        "mirror" => "Mirror",
        "curved_mirror" => "Curved mirror",
        "pbs" => "Polarizing beam splitter",
        "plate_splitter" => "Plate beam splitter",
        "lens" => "Lens",
        "plano_convex_lens" => "Plano-convex lens",
        "plano_concave_lens" => "Plano-concave lens",
        "lens_group" => "Lens group",
        "waveplate" => "Waveplate",
        "polarizer" => "Polarizer",
        "camera" => "Camera",
        "aperture" => "Aperture",
        "blocker" => "Beam block",
        "slm" => "SLM",
        "nd_filter" => "ND filter",
        "mask" => "Mask",
        "tip_tilt" => "Tip-tilt mirror",
        "ground_glass" => "Ground glass",
        _ => "Laser",
    }
}

/// Laser head behind the emission point.
fn source_symbol(id: &str, position: [f64; 3], direction: [f64; 3]) -> Symbol {
    let c = xy(position);
    let n = unit(xy(direction)).unwrap_or([1., 0.]);
    let f = Frame {
        c,
        n,
        t: [-n[1], n[0]],
    };
    Symbol {
        id: id.to_owned(),
        kind: "laser",
        center: f.at(-15., 0.),
        strokes: vec![
            f.rect([-30., 0.], [-6., 6.], Role::Fill),
            f.line([0., -2.], [0., 2.], Role::Face),
        ],
    }
}

#[derive(Clone, Copy)]
struct Box2 {
    min: P,
    max: P,
}
impl Box2 {
    fn around(points: impl IntoIterator<Item = P>) -> Self {
        points.into_iter().fold(
            Self {
                min: [f64::INFINITY; 2],
                max: [f64::NEG_INFINITY; 2],
            },
            |b, p| Self {
                min: [b.min[0].min(p[0]), b.min[1].min(p[1])],
                max: [b.max[0].max(p[0]), b.max[1].max(p[1])],
            },
        )
    }
    fn grow(self, by: f64) -> Self {
        Self {
            min: sub(self.min, [by; 2]),
            max: add(self.max, [by; 2]),
        }
    }
    fn overlaps(&self, o: &Self) -> bool {
        self.min[0] < o.max[0]
            && o.min[0] < self.max[0]
            && self.min[1] < o.max[1]
            && o.min[1] < self.max[1]
    }
    fn contains(&self, p: P) -> bool {
        (self.min[0]..=self.max[0]).contains(&p[0]) && (self.min[1]..=self.max[1]).contains(&p[1])
    }
    /// The segment `a`–`b` passes through the box (Liang–Barsky clipping).
    fn crosses(&self, a: P, b: P) -> bool {
        let d = sub(b, a);
        let (mut lo, mut hi) = (0_f64, 1_f64);
        for axis in 0..2 {
            if d[axis].abs() < 1e-12 {
                if a[axis] <= self.min[axis] || a[axis] >= self.max[axis] {
                    return false;
                }
                continue;
            }
            let t0 = (self.min[axis] - a[axis]) / d[axis];
            let t1 = (self.max[axis] - a[axis]) / d[axis];
            lo = lo.max(t0.min(t1));
            hi = hi.min(t0.max(t1));
        }
        lo < hi
    }
    fn distance(&self, p: P) -> f64 {
        let dx = (self.min[0] - p[0]).max(p[0] - self.max[0]).max(0.);
        let dy = (self.min[1] - p[1]).max(p[1] - self.max[1]).max(0.);
        dx.hypot(dy)
    }
}

/// Greedy label placement around anchors, clear of symbols, beams and earlier labels.
struct Placer {
    text: f64,
    table: Box2,
    symbols: Vec<(String, Box2)>,
    beams: Vec<[P; 2]>,
    labels: Vec<Label>,
}
impl Placer {
    fn size(&self, text: &str) -> P {
        // Wide (CJK) characters take a full em, others about 0.6 em.
        let ems: f64 = text
            .chars()
            .map(|c| if c as u32 >= 0x2e80 { 1. } else { 0.6 })
            .sum();
        [ems * self.text + 0.6 * self.text, 1.3 * self.text]
    }
    fn cost(&self, b: &Box2) -> usize {
        let symbols = self.symbols.iter().filter(|(_, s)| s.overlaps(b)).count();
        let beams = self.beams.iter().filter(|[p, q]| b.crosses(*p, *q)).count();
        let labels = self
            .labels
            .iter()
            .filter(|l| {
                Box2 {
                    min: l.corner,
                    max: add(l.corner, l.size),
                }
                .overlaps(b)
            })
            .count();
        symbols + beams + labels
    }
    /// Place a label at the first candidate box with no overlap, else the least
    /// overlapping one; candidates keep inside the table when they can.
    fn place(&mut self, kind: &'static str, target: &str, text: String, anchor: P, reach: f64) {
        let size = self.size(&text);
        let [w, h] = size;
        // Box corner offsets for the eight compass directions, nearest edge at the ring.
        let directions: [(P, P); 8] = [
            ([0.707, 0.707], [0., 0.]),
            ([1., 0.], [0., -h / 2.]),
            ([0.707, -0.707], [0., -h]),
            ([0., 1.], [-w / 2., 0.]),
            ([0., -1.], [-w / 2., -h]),
            ([-0.707, 0.707], [-w, 0.]),
            ([-1., 0.], [-w, -h / 2.]),
            ([-0.707, -0.707], [-w, -h]),
        ];
        let mut best: Option<(usize, bool, P)> = None;
        'search: for ring in 0..8 {
            let radius = reach + 0.6 * self.text + ring as f64 * 1.2 * h;
            for (direction, offset) in directions {
                let corner = add(add(anchor, mul(direction, radius)), offset);
                let b = Box2 {
                    min: corner,
                    max: add(corner, size),
                };
                let outside = !(self.table.contains(b.min) && self.table.contains(b.max));
                let cost = self.cost(&b);
                if best.is_none_or(|(c, o, _)| (cost, outside) < (c, o)) {
                    best = Some((cost, outside, corner));
                    if cost == 0 && !outside {
                        break 'search;
                    }
                }
            }
        }
        let (overlaps, _, corner) = best.expect("eight rings of candidates");
        let gap = Box2 {
            min: corner,
            max: add(corner, size),
        }
        .distance(anchor);
        self.labels.push(Label {
            kind,
            target: target.to_owned(),
            text,
            anchor,
            corner,
            size,
            leader: gap > reach + 1.5 * h,
            overlaps,
        });
    }
}

fn number(value: f64) -> String {
    reading_number((value * 10.).round() / 10. + 0.)
}

fn ruler(axis: &'static str, length: f64) -> impl Iterator<Item = Tick> {
    let step = [
        1., 2., 5., 10., 25., 50., 100., 250., 500., 1000., 2500., 5000., 10_000.,
    ]
    .into_iter()
    .find(|s| length / s <= 40.)
    .unwrap_or(length / 40.);
    let count = (length / step + 1e-9).floor() as usize;
    (0..=count.min(4000)).map(move |i| Tick {
        axis,
        value_mm: i as f64 * step,
        major: i % 2 == 0,
    })
}

/// Build the drawing of a stage-resolved bench from its centre-ray trace.
pub(crate) fn drawing(
    bench: &Bench,
    trace: &Trace,
    assertions: Vec<AssertionMark>,
    text_mm: f64,
) -> Drawing {
    let origin = xy(bench.origin);
    let size = bench.table_size;
    let symbols: Vec<Symbol> = bench
        .elements
        .iter()
        .map(|e| element_symbol(e, trace))
        .chain(
            bench
                .sources
                .iter()
                .map(|s| source_symbol(&s.id, s.position, s.direction)),
        )
        .collect();
    let symbol_box = |id: &str| {
        symbols
            .iter()
            .find(|s| s.id == id)
            .map(|s| Box2::around(s.strokes.iter().flat_map(|k| k.points.iter().copied())))
    };

    // Beam heights: the lowest layer, else the lowest source, is the base.
    let base = bench
        .layers
        .iter()
        .map(|l| l.height_mm)
        .chain(
            bench
                .layers
                .is_empty()
                .then(|| bench.sources.iter().map(|s| s.position[2]))
                .into_iter()
                .flatten(),
        )
        .fold(f64::INFINITY, f64::min);
    let arrow_size = (0.8 * text_mm).max(1.5);
    let beam = |index: usize, branch: Option<&str>, color: &str, dotted: bool| {
        let s = &trace.segments[index];
        let (from, to) = (xy(s.from), xy(s.to));
        let d = unit(sub(to, from))?;
        let length = sub(to, from)[0].hypot(sub(to, from)[1]);
        let z = (s.from[2] + s.to[2]) / 2.;
        let arrow = (length >= 4. * arrow_size).then(|| {
            let tip = add(mul(add(from, to), 0.5), mul(d, arrow_size / 2.));
            let back = sub(tip, mul(d, arrow_size));
            let side = mul([-d[1], d[0]], arrow_size * 0.4);
            [tip, add(back, side), sub(back, side)]
        });
        let dashed = base.is_finite() && (z - base).abs() > LAYER_TOLERANCE;
        Some(BeamMark {
            branch: branch.map(str::to_owned),
            path: s.branch.clone(),
            color: color.to_owned(),
            dashed,
            dash: dash(dashed, dotted),
            layer: bench
                .layers
                .iter()
                .find(|l| (l.height_mm - z).abs() <= LAYER_TOLERANCE)
                .map(|l| l.id.clone()),
            from,
            to,
            arrow,
        })
    };
    let branched: BTreeSet<usize> = trace
        .branches
        .iter()
        .flat_map(|b| {
            b.paths
                .iter()
                .flat_map(|p| p.segment_indices.iter().copied())
        })
        .collect();
    let mut beams: Vec<BeamMark> = (0..trace.segments.len())
        .filter(|i| !branched.contains(i))
        .filter_map(|i| beam(i, None, NEUTRAL, false))
        .collect();
    for report in &trace.branches {
        let (color, dotted) = branch_style(bench, &report.id);
        let mut seen = BTreeSet::new();
        for &i in report.paths.iter().flat_map(|p| &p.segment_indices) {
            if seen.insert(i)
                && let Some(mark) = beam(i, Some(&report.id), &color, dotted)
            {
                beams.push(mark);
            }
        }
    }

    let position = |id: &str| {
        bench
            .elements
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.position)
    };
    let periscopes: Vec<PeriscopeMark> = bench
        .periscopes
        .iter()
        .filter_map(|p| {
            let (lower, upper) = (position(&p.lower)?, position(&p.upper)?);
            Some(PeriscopeMark {
                id: p.id.clone(),
                at: xy(lower),
                rise_mm: upper[2] - lower[2],
            })
        })
        .collect();

    let zones: Vec<ZoneMark> = bench
        .zones
        .iter()
        .map(|z| ZoneMark {
            id: z.id.clone(),
            label: z.label.clone(),
            outline: z.outline(),
        })
        .collect();
    let stages: Vec<StageMark> = bench
        .stages
        .iter()
        .filter_map(|stage| {
            let b = stage
                .members
                .iter()
                .filter_map(|m| symbol_box(m))
                .reduce(|a, b| Box2::around([a.min, a.max, b.min, b.max]))?
                .grow(3.);
            // Foreshortened by the axis tilt; a vertical axis travels in place.
            let axis = xy(stage.axis);
            let center = mul(add(b.min, b.max), 0.5);
            let end = |t: f64| add(center, mul(axis, t - stage.position_mm));
            Some(StageMark {
                id: stage.id.clone(),
                outline: [b.min, [b.max[0], b.min[1]], b.max, [b.min[0], b.max[1]]],
                travel: [end(stage.travel[0]), end(stage.travel[1])],
            })
        })
        .collect();

    let mut placer = Placer {
        text: text_mm,
        table: Box2 {
            min: origin,
            max: add(origin, size),
        },
        symbols: symbols
            .iter()
            .map(|s| {
                (
                    s.id.clone(),
                    Box2::around(s.strokes.iter().flat_map(|k| k.points.iter().copied())).grow(0.5),
                )
            })
            .chain(periscopes.iter().map(|p| {
                (
                    p.id.clone(),
                    Box2 {
                        min: p.at,
                        max: p.at,
                    }
                    .grow(3.5),
                )
            }))
            .collect(),
        beams: beams.iter().map(|b| [b.from, b.to]).collect(),
        labels: vec![],
    };
    let coordinates = |p: P| {
        format!(
            "({}, {})",
            number(p[0] - origin[0]),
            number(p[1] - origin[1])
        )
    };
    for e in &bench.elements {
        let reach = symbol_box(&e.id).map_or(e.body_radius, |b| {
            b.distance(xy(e.position))
                .max((b.max[0] - b.min[0]).max(b.max[1] - b.min[1]) / 2.)
        });
        placer.place(
            "element",
            &e.id,
            format!("{} {}", e.id, coordinates(xy(e.position))),
            xy(e.position),
            reach,
        );
    }
    for s in &bench.sources {
        let center = symbol_box(&s.id).map_or(xy(s.position), |b| mul(add(b.min, b.max), 0.5));
        placer.place(
            "source",
            &s.id,
            format!("{} {}", s.id, coordinates(xy(s.position))),
            center,
            16.,
        );
    }
    for p in &periscopes {
        let rise = if p.rise_mm >= 0. {
            format!("+{}", number(p.rise_mm))
        } else {
            number(p.rise_mm)
        };
        placer.place(
            "periscope",
            &p.id,
            format!("{} ΔZ {rise} mm", p.id),
            p.at,
            4.,
        );
    }
    for (stage, mark) in bench.stages.iter().zip(&stages) {
        let b = Box2::around(mark.outline);
        let corner = [b.min[0], b.max[1]];
        placer.place(
            "stage",
            &stage.id,
            format!("Stage {} · {} mm", stage.id, number(stage.position_mm)),
            corner,
            0.,
        );
    }
    for zone in &zones {
        let b = Box2::around(zone.outline.iter().copied());
        let text = if zone.label.is_empty() {
            zone.id.clone()
        } else {
            zone.label.clone()
        };
        placer.place("zone", &zone.id, text, [b.min[0], b.max[1]], 0.);
    }
    for a in &assertions {
        if let Some(at) = a.element.as_deref().and_then(position) {
            let mark = match (a.passed, a.text.as_str()) {
                (true, _) => "✓",
                (false, "indeterminate") => "?",
                _ => "✗",
            };
            placer.place("assertion", &a.id, format!("{mark} {}", a.id), xy(at), 4.);
        }
    }
    for note in &bench.annotations {
        let (target, anchor) = match &note.target {
            AnnotationTarget::Element(id) => (
                id.clone(),
                bench
                    .elements
                    .iter()
                    .map(|e| (&e.id, e.position))
                    .chain(bench.sources.iter().map(|s| (&s.id, s.position)))
                    .find(|(i, _)| *i == id)
                    .map(|(_, p)| xy(p)),
            ),
            AnnotationTarget::Branch(id) => (
                id.clone(),
                beams
                    .iter()
                    .filter(|b| b.branch.as_deref() == Some(id.as_str()))
                    .max_by(|a, b| {
                        let len = |m: &BeamMark| sub(m.to, m.from)[0].hypot(sub(m.to, m.from)[1]);
                        len(a).total_cmp(&len(b))
                    })
                    .map(|b| mul(add(b.from, b.to), 0.5))
                    .or_else(|| {
                        bench
                            .branches
                            .iter()
                            .find(|b| &b.id == id)
                            .and_then(|b| position(&b.from.element))
                            .map(xy)
                    }),
            ),
        };
        if let Some(anchor) = anchor {
            placer.place("annotation", &target, note.text.clone(), anchor, 3.);
        }
    }

    let entry =
        |kind: &'static str, key: String, text: String, color: &str, dashed: bool, dotted: bool| {
            LegendEntry {
                kind,
                key,
                text,
                color: color.to_owned(),
                dashed,
                dash: dash(dashed, dotted),
            }
        };
    let mut legend: Vec<LegendEntry> = trace
        .branches
        .iter()
        .map(|b| {
            let (color, dotted) = branch_style(bench, &b.id);
            entry("branch", b.id.clone(), b.id.clone(), &color, false, dotted)
        })
        .collect();
    if beams.iter().any(|b| b.branch.is_none()) {
        legend.push(entry(
            "beam",
            "unbranched".into(),
            "Beam on no named branch".into(),
            NEUTRAL,
            false,
            false,
        ));
    }
    for layer in &bench.layers {
        let text = format!("Layer {} · Z {} mm", layer.id, number(layer.height_mm));
        legend.push(entry(
            "layer",
            layer.id.clone(),
            text,
            "#334e68",
            (layer.height_mm - base).abs() > LAYER_TOLERANCE,
            false,
        ));
    }
    if bench.layers.is_empty() && beams.iter().any(|b| b.dashed) {
        legend.push(entry(
            "layer",
            "raised".into(),
            "Beam off the source height".into(),
            "#334e68",
            true,
            false,
        ));
    }
    let kinds: BTreeSet<&str> = symbols.iter().map(|s| s.kind).collect();
    legend.extend(kinds.into_iter().map(|k| {
        entry(
            "symbol",
            k.into(),
            kind_text(k).into(),
            "#102a43",
            false,
            false,
        )
    }));

    Drawing {
        origin,
        size,
        hole_pitch: bench.hole_pitch,
        holes: vec![],
        hole_radius: 1.5_f64.min(bench.hole_pitch / 5.),
        text_mm,
        rulers: ruler("x", size[0]).chain(ruler("y", size[1])).collect(),
        zones,
        stages,
        symbols,
        beams,
        periscopes,
        labels: placer.labels,
        legend,
        assertions,
        cards: vec![],
        positions: bench
            .elements
            .iter()
            .map(|e| (&e.id, e.position))
            .chain(bench.sources.iter().map(|s| (&s.id, s.position)))
            .map(|(id, position)| PartPosition {
                id: id.clone(),
                position,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optics::bench::Substrate;
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn every_part_kind_draws_its_own_symbol_facing_the_beam() {
        let bench = super::super::template("delay_line").unwrap();
        let trace = super::super::center_trace(&bench, &super::super::System::default()).unwrap();
        let base = bench.elements.iter().find(|e| e.id == "M3").unwrap();
        let n = facing(base, &trace).unwrap();
        let action =
            |value: serde_json::Value| Some(serde_json::from_value::<Action>(value).unwrap());
        let variant = |kind: Kind, action: Option<Action>, plate: bool| {
            let mut e = base.clone();
            e.kind = kind;
            e.action = action;
            e.substrate = plate.then(|| Substrate {
                material: "N-BK7".into(),
                thickness_mm: 3.,
                wedge_deg: None,
                coated_face: Some(Face::Front),
            });
            e
        };
        let cases = [
            (variant(Kind::Mirror, None, false), "mirror"),
            (variant(Kind::CurvedMirror, None, false), "curved_mirror"),
            (variant(Kind::Pbs, None, false), "pbs"),
            (variant(Kind::Splitter, None, true), "plate_splitter"),
            (variant(Kind::Lens, None, false), "lens"),
            (
                {
                    let mut e = variant(Kind::Lens, None, false);
                    e.lens_form = Some(LensForm::Plano);
                    e
                },
                "plano_convex_lens",
            ),
            (
                {
                    let mut e = variant(Kind::Lens, None, false);
                    e.lens_form = Some(LensForm::Plano);
                    e.focal_length = -50.;
                    e
                },
                "plano_concave_lens",
            ),
            (variant(Kind::LensGroup, None, false), "lens_group"),
            (variant(Kind::Waveplate, None, false), "waveplate"),
            (variant(Kind::Polarizer, None, false), "polarizer"),
            (variant(Kind::Detector, None, false), "camera"),
            (variant(Kind::Aperture, None, false), "aperture"),
            (variant(Kind::Blocker, None, false), "blocker"),
            (
                variant(
                    Kind::Aperture,
                    action(
                        json!({"kind":"slm","segments":[4,2],"size_mm":[10.,5.],"phases_rad":[0.,0.,0.,0.,0.,0.,0.,0.]}),
                    ),
                    false,
                ),
                "slm",
            ),
            (
                variant(Kind::Aperture, action(json!({"kind":"nd","od":1.})), false),
                "nd_filter",
            ),
            (
                variant(Kind::Aperture, action(json!({"kind":"mask"})), false),
                "mask",
            ),
            (
                variant(
                    Kind::Aperture,
                    action(
                        json!({"kind":"rgg","correlation_um":10.,"roughness_waves":1.,"seed":1}),
                    ),
                    false,
                ),
                "ground_glass",
            ),
            (
                variant(
                    Kind::Mirror,
                    action(json!({"kind":"tip_tilt","tilt_mrad":[1.,0.]})),
                    false,
                ),
                "tip_tilt",
            ),
        ];
        let mut drawn = HashSet::new();
        for (element, kind) in &cases {
            let symbol = element_symbol(element, &trace);
            assert_eq!(symbol.kind, *kind);
            assert!(
                drawn.insert(serde_json::to_string(&symbol.strokes).unwrap()),
                "{kind} looks like another part"
            );
            let reach = 3. * element.body_radius.max(element.aperture);
            for p in symbol.strokes.iter().flat_map(|s| &s.points) {
                assert!(
                    (p[0] - symbol.center[0]).hypot(p[1] - symbol.center[1]) <= reach,
                    "{kind} strays from its element"
                );
            }
        }
        // The mirror's coated face lies across the facing normal, so turning the
        // element turns its symbol.
        let mirror = element_symbol(&cases[0].0, &trace);
        let face = mirror
            .strokes
            .iter()
            .find(|s| s.role == Role::Face)
            .unwrap();
        let d = sub(face.points[1], face.points[0]);
        assert!((d[0] * n[0] + d[1] * n[1]).abs() < 1e-9 * d[0].hypot(d[1]));
        let source = &bench.sources[0];
        let laser = source_symbol(&source.id, source.position, source.direction);
        assert_eq!(laser.kind, "laser");
        // The laser body sits behind its emission point.
        let behind = |p: &P| {
            (p[0] - source.position[0]) * source.direction[0]
                + (p[1] - source.position[1]) * source.direction[1]
        };
        assert!(
            laser
                .strokes
                .iter()
                .flat_map(|s| &s.points)
                .all(|p| behind(p) <= 1e-9)
        );
        assert!(behind(&laser.center) < 0.);
    }

    /// Stroke points of `symbol` in the frame of unit normal `n` at the symbol
    /// centre: (along n, along the face).
    fn local(symbol: &Symbol, role: Option<Role>, n: P) -> Vec<P> {
        symbol
            .strokes
            .iter()
            .filter(|s| role.is_none_or(|r| s.role == r))
            .flat_map(|s| &s.points)
            .map(|p| {
                let d = sub(*p, symbol.center);
                [d[0] * n[0] + d[1] * n[1], -d[0] * n[1] + d[1] * n[0]]
            })
            .collect()
    }

    #[test]
    fn symbols_turn_with_a_known_yaw_and_plate_substrates_stay_on_the_minus_x_side() {
        let bench = super::super::template("delay_line").unwrap();
        let trace = super::super::center_trace(&bench, &super::super::System::default()).unwrap();
        let base = bench.elements.iter().find(|e| e.id == "M3").unwrap();
        // Yaw 30° and no traced light: the symbol faces the element's own +X.
        let yaw = 30_f64.to_radians();
        let n = [yaw.cos(), yaw.sin()];
        let at = |kind: Kind| {
            let mut e = base.clone();
            e.id = "unlit".into();
            e.kind = kind;
            e.action = None;
            e.substrate = None;
            e.rotation = Some([0., 0., 30.]);
            e.focal_length = 50.;
            e
        };
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let range = |points: &[P], axis: usize| {
            points
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(p[axis]), hi.max(p[axis]))
                })
        };
        let r = base.body_radius;
        let thin = (0.12 * r).clamp(0.6, 3.);
        let th = (0.25 * r).clamp(1., 8.);
        type Span = (f64, f64);
        let with = |kind: Kind, edit: &dyn Fn(&mut Element)| {
            let mut e = at(kind);
            edit(&mut e);
            e
        };
        let plate = |face: Face| {
            move |e: &mut Element| {
                e.substrate = Some(Substrate {
                    material: "N-BK7".into(),
                    thickness_mm: 4.,
                    wedge_deg: None,
                    coated_face: Some(face),
                });
            }
        };
        let action = |value: serde_json::Value| {
            let action: Action = serde_json::from_value(value).unwrap();
            move |e: &mut Element| e.action = Some(action.clone())
        };
        let plano = |f: f64| {
            move |e: &mut Element| {
                e.lens_form = Some(LensForm::Plano);
                e.focal_length = f;
            }
        };
        let gap = base.aperture.min(0.9 * r);
        let slm = action(
            json!({"kind":"slm","segments":[4,2],"size_mm":[10.,5.],"phases_rad":[0.,0.,0.,0.,0.,0.,0.,0.]}),
        );
        let expect: Vec<(Element, Role, Span, Span)> = vec![
            // (element, role, along-n range, along-face range)
            (at(Kind::Mirror), Role::Face, (0., 0.), (-r, r)),
            (at(Kind::Mirror), Role::Body, (-th, 0.), (-r, r)),
            (at(Kind::Splitter), Role::Body, (-thin.max(1.), 0.), (-r, r)),
            // A modelled substrate sets the drawn thickness; a coated back face is drawn at −t.
            (
                with(Kind::Splitter, &plate(Face::Back)),
                Role::Body,
                (-4., 0.),
                (-r, r),
            ),
            (
                with(Kind::Splitter, &plate(Face::Back)),
                Role::Face,
                (-4., -4.),
                (-r, r),
            ),
            (
                with(Kind::Splitter, &plate(Face::Front)),
                Role::Face,
                (0., 0.),
                (-r, r),
            ),
            (at(Kind::Pbs), Role::Body, (-r, r), (-r, r)),
            (at(Kind::Pbs), Role::Face, (0., 0.), (-r, r)),
            (at(Kind::Lens), Role::Body, (-0.3 * r, 0.3 * r), (-r, r)),
            (
                with(Kind::Lens, &plano(50.)),
                Role::Body,
                (0., 0.4 * r),
                (-r, r),
            ),
            (with(Kind::Lens, &plano(50.)), Role::Face, (0., 0.), (-r, r)),
            (
                with(Kind::Lens, &plano(-50.)),
                Role::Body,
                (0., 0.35 * r),
                (-r, r),
            ),
            (at(Kind::Waveplate), Role::Body, (-thin, thin), (-r, r)),
            (
                at(Kind::Detector),
                Role::Fill,
                (-1.2 * r, 0.),
                (-0.8 * r, 0.8 * r),
            ),
            (at(Kind::Detector), Role::Face, (0., 0.), (-r, r)),
            (at(Kind::Blocker), Role::Fill, (-0.5 * r, 0.), (-r, r)),
            (at(Kind::Aperture), Role::Fill, (-thin, thin), (-r, r)),
            (with(Kind::Aperture, &slm), Role::Body, (-th, 0.), (-r, r)),
            (with(Kind::Aperture, &slm), Role::Face, (0., 0.), (-r, r)),
            (
                with(Kind::Aperture, &slm),
                Role::Hatch,
                (-th, 0.),
                (-r / 2., r / 2.),
            ),
        ];
        for (element, role, along, across) in &expect {
            let symbol = element_symbol(element, &trace);
            let kind = symbol.kind;
            let points = local(&symbol, Some(*role), n);
            assert!(!points.is_empty(), "{kind} {role:?}");
            let (a, b) = (range(&points, 0), range(&points, 1));
            assert!(
                close(a.0, along.0) && close(a.1, along.1),
                "{kind} {role:?} along n: {a:?}"
            );
            assert!(
                close(b.0, across.0) && close(b.1, across.1),
                "{kind} {role:?} across: {b:?}"
            );
        }
        // The aperture's opening is the clear aperture, centred on the beam axis.
        let aperture = element_symbol(&at(Kind::Aperture), &trace);
        let opening = local(&aperture, Some(Role::Fill), n)
            .iter()
            .map(|p| p[1].abs())
            .fold(f64::INFINITY, f64::min);
        assert!(close(opening, gap), "{opening} vs {gap}");
        // A source at yaw 30°: the laser body lies 6–30 mm behind its emission point.
        let laser = source_symbol("S", [10., 20., 75.], [n[0], n[1], 0.]);
        let body = local(&laser, Some(Role::Fill), n);
        let to_emission = |p: &P| [p[0] - 15., p[1]];
        let (a, b) = (
            range(&body.iter().map(to_emission).collect::<Vec<_>>(), 0),
            range(&body, 1),
        );
        assert!(
            close(a.0, -30.) && close(a.1, 0.) && close(b.0, -6.) && close(b.1, 6.),
            "{a:?} {b:?}"
        );
        assert!(
            close(laser.center[0], 10. - 15. * n[0]) && close(laser.center[1], 20. - 15. * n[1])
        );

        // A plate splitter keeps its substrate behind its own +X face whichever
        // side the light comes from; a mirror turns to face the light.
        let bs = bench
            .elements
            .iter()
            .find(|e| e.kind == Kind::Splitter)
            .unwrap();
        let mut flipped = 0;
        for turn in [0., 180.] {
            // The plano lens keeps its flat face on −X just as the plate keeps its substrate.
            let mut lens = bs.clone();
            lens.kind = Kind::Lens;
            lens.lens_form = Some(LensForm::Plano);
            lens.focal_length = 75.;
            let [rx, ry, rz] = lens.rotation();
            lens.rotation = Some([rx, ry, rz + turn]);
            let mut unlit = lens.clone();
            unlit.id = "unlit".into();
            assert_eq!(
                serde_json::to_string(&element_symbol(&lens, &trace).strokes).unwrap(),
                serde_json::to_string(&element_symbol(&unlit, &trace).strokes).unwrap(),
                "light direction moved the plano lens",
            );
            let mut e = bs.clone();
            let [rx, ry, rz] = e.rotation();
            e.rotation = Some([rx, ry, rz + turn]);
            let n0 = unit(xy(rotate([1., 0., 0.], e.rotation()))).unwrap();
            let symbol = element_symbol(&e, &trace);
            assert_eq!(symbol.kind, "plate_splitter");
            let (lo, hi) = range(&local(&symbol, Some(Role::Body), n0), 0);
            assert!(
                close(hi, 0.) && close(lo, -thin.max(1.)),
                "turn {turn}: {lo} {hi}"
            );
            let face = facing(&e, &trace).unwrap();
            flipped += usize::from(face[0] * n0[0] + face[1] * n0[1] < 0.);
            let mut lit = e.clone();
            lit.id = "unlit".into();
            assert_eq!(
                serde_json::to_string(&symbol.strokes).unwrap(),
                serde_json::to_string(&element_symbol(&lit, &trace).strokes).unwrap(),
                "light direction moved the plate",
            );
        }
        assert_eq!(
            flipped, 1,
            "one of the two turns has the light arriving on the +X side"
        );
    }

    #[test]
    fn stage_travel_is_drawn_foreshortened_and_rulers_count_from_the_table_corner() {
        let mut bench = super::super::template("delay_line").unwrap();
        bench.origin = [125., -40., bench.origin[2]];
        let stage = bench.stages.first_mut().expect("delay_line has a stage");
        stage.axis = [3_f64.sqrt() / 2., 0., 0.5];
        stage.travel = [0., 20.];
        let scene = bench.resolved_stages().unwrap();
        let trace =
            super::super::center_trace(scene.as_ref(), &super::super::System::default()).unwrap();
        let plan = drawing(scene.as_ref(), &trace, vec![], 4.);
        let [a, b] = plan.stages[0].travel;
        assert!(((b[0] - a[0]).hypot(b[1] - a[1]) - 20. * 3_f64.sqrt() / 2.).abs() < 1e-9);
        assert_eq!(plan.origin, [125., -40.]);
        for (axis, length) in [("x", bench.table_size[0]), ("y", bench.table_size[1])] {
            let ticks: Vec<f64> = plan
                .rulers
                .iter()
                .filter(|t| t.axis == axis)
                .map(|t| t.value_mm)
                .collect();
            assert_eq!(
                ticks[0], 0.,
                "{axis} starts at the corner, not at the origin coordinate"
            );
            let step = ticks[1] - ticks[0];
            assert!(ticks.windows(2).all(|w| (w[1] - w[0] - step).abs() < 1e-9));
            assert!(
                *ticks.last().unwrap() <= length && ticks.last().unwrap() + step > length,
                "{axis}"
            );
        }
    }
}
