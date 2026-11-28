//! Shaded axonometric layout of a bench, in the manner of a non-sequential
//! shaded model: parametric solids, triangulated with outward normals and
//! flat Lambert shading, and sampled ray bundles, projected orthographically onto a millimetre page and depth-sorted
//! (painter's algorithm) into one SVG of vector polygons and lines, which the
//! PDF and PNG deliverables reuse.
//!
//! The view follows the GUI 3D mode in `BenchView.vue`: with yaw `a` and pitch
//! `p`, table-centred coordinates (x, y, z) go to page right
//! `cos a·x − sin a·y` and page up `sin a·sin p·x + cos a·sin p·y + cos p·z`.
//! The depth axis toward the viewer completes the right-handed frame.

use super::{BlueprintRevision, Grid, base_corners, drawing, xml};
use crate::optics::{
    System,
    bench::{self, Action, Bench, Element, Kind, LensForm, Port, Trace},
    math::{V3, add, dot, norm, scale, sub, unit},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    f64::consts::TAU,
    fmt::Write,
};

/// Shaded 3-D layout options, given beside `format` in the export call.
#[derive(Clone, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Layout3dOptions {
    /// With pdf or png, true delivers the 3-D layout instead of the plan; any
    /// other layout option implies it. Format layout3d needs none of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout3d: Option<bool>,
    /// Explicit view, as in the workbench 3D mode; excludes `preset`. Without
    /// either, the iso preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<View3d>,
    /// Standard view: iso (true isometric, azimuth −45°, elevation
    /// arctan(1/√2) ≈ 35.26°), top (elevation 90°) or side (elevation 0°,
    /// looking along +y).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<Preset>,
    /// Rays drawn per source (default 7, at most 101), sampled over the
    /// source's beam radius: full hexagonal rings of 1 + 3n(n+1) rays from
    /// the axis out, the last ring thinned evenly to the requested count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 101))]
    pub rays_per_source: Option<usize>,
    /// Ray colours: branch (named branch colours as on the blueprint, other
    /// light in the neutral colour; the default), wavelength (visible light in
    /// its spectral colour, UV violet and IR crimson) or field (one colour per
    /// source, each source being one field point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_by: Option<ColorBy>,
    /// gradient (blue to white; the default) or white for print.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Background>,
    /// Optional bench hardware, all hidden by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<Show3d>,
    /// Outline every face with a darker edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<bool>,
    /// Solid thickness overrides by element id (mm): mirror and plate
    /// thickness, lens edge thickness, barrel length, camera or SLM body depth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_mm: Option<BTreeMap<String, f64>>,
    /// Lens shape overrides by lens id; omitted values follow from the focal
    /// length, index and lens form (lensmaker's equation, 2 mm edge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens_shape: Option<BTreeMap<String, LensShape>>,
}

impl Layout3dOptions {
    /// The options when they ask for the 3-D layout.
    pub(crate) fn requested(&self) -> Option<&Self> {
        let any = self.view.is_some()
            || self.preset.is_some()
            || self.rays_per_source.is_some()
            || self.color_by.is_some()
            || self.background.is_some()
            || self.show.is_some()
            || self.stroke.is_some()
            || self.thickness_mm.is_some()
            || self.lens_shape.is_some();
        (self.layout3d.unwrap_or(any)).then_some(self)
    }

    fn rays(&self) -> usize {
        self.rays_per_source.unwrap_or(7)
    }
}

/// A lens drawn from its surface radii (mm) instead of the lensmaker's
/// estimate. A radius is positive where the face is convex (bulges out of
/// the glass), negative where concave and 0 for a flat face; the front face
/// is the one on the element's +X side.
#[derive(Clone, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LensShape {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = -1e6, max = 1e6))]
    pub front_radius_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = -1e6, max = 1e6))]
    pub back_radius_mm: Option<f64>,
    /// Glass thickness on the axis; without it the edge is 2 mm (or the
    /// thickness_mm override) thick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0, max = 1e4))]
    pub center_thickness_mm: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct View3d {
    /// Yaw about the table normal (degrees), as the workbench 3D mode.
    #[schemars(range(min = -720.0, max = 720.0))]
    pub azimuth_deg: f64,
    /// Pitch (degrees): 90 looks straight down, 0 along the table.
    #[schemars(range(min = -90.0, max = 90.0))]
    pub elevation_deg: f64,
    /// Magnification over the fit-to-page scale (default 1).
    #[serde(default = "one")]
    #[schemars(range(min = 0.05, max = 50.0))]
    pub zoom: f64,
    /// Shift of the drawing on the page (mm, right and down).
    #[serde(default)]
    #[schemars(inner(range(min = -10000.0, max = 10000.0)))]
    pub pan_mm: [f64; 2],
}

fn one() -> f64 {
    1.
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Iso,
    Top,
    Side,
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case")]
pub enum ColorBy {
    #[default]
    Branch,
    Wavelength,
    Field,
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    #[default]
    Gradient,
    White,
}

/// Optional bench hardware, all hidden by default.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Show3d {
    #[serde(default)]
    pub table: bool,
    #[serde(default)]
    pub holes: bool,
    #[serde(default)]
    pub mounts: bool,
}

type Rgb = [f64; 3];
type P2 = [f64; 2];

/// Page (A3 landscape) and the drawing area inside its margins and footer.
const PAGE: P2 = [420., 297.];
const AREA_MIN: P2 = [12., 12.];
const AREA_MAX: P2 = [408., 255.];
/// Facets around a solid of revolution and radial steps across a curved surface.
const SEGMENTS: usize = 32;
const RINGS: usize = 6;
/// Opacity of a solid drawn in full.
const OPAQUE: f64 = 1.;
/// Segments of a source below this fraction of its strongest segment are dropped.
const MIN_POWER_FRACTION: f64 = 1e-3;
/// Glass index when an element states none (N-BK7 at 587.6 nm).
const GLASS_INDEX: f64 = 1.5168;
const STRAY: &str = "#8a8f98";

/// One triangle of a solid's mesh, in bench coordinates.
#[derive(Clone, Debug)]
pub(crate) struct Face {
    pub points: Vec<V3>,
    /// Whether edge i (points i → i + 1) belongs to the outline of the
    /// polygon the triangle came from, rather than lying inside it.
    pub outline: Vec<bool>,
    /// Outward unit normal, shared by the polygon's triangles.
    pub normal: V3,
    pub color: Rgb,
    pub alpha: f64,
    pub part: String,
}

/// Newell's normal of a polygon (length twice its area).
fn newell(points: &[V3]) -> V3 {
    let mut n = [0.; 3];
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

fn centroid(points: &[V3]) -> V3 {
    scale(
        points.iter().fold([0.; 3], |s, &p| add(s, p)),
        1. / points.len() as f64,
    )
}

/// Faces of one solid around `center`; each face normal points away from the
/// centre and degenerate faces drop.
pub(crate) struct Solid {
    part: String,
    /// What the solid draws, independent of `part`, which may be any element id.
    ground: Ground,
    center: V3,
    faces: Vec<Face>,
}

/// The bench surface drawn under everything else, or a part standing on it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ground {
    Part,
    Table,
    Holes,
}

impl Solid {
    fn new(part: &str, center: V3) -> Self {
        Self {
            part: part.to_owned(),
            ground: Ground::Part,
            center,
            faces: vec![],
        }
    }

    /// The table top or its hole pattern.
    fn ground(part: &str, ground: Ground, center: V3) -> Self {
        Self {
            ground,
            ..Self::new(part, center)
        }
    }

    /// A face whose normal points away from the solid's centre: for decals
    /// on a body and other faces where that centre lies behind them.
    fn face(&mut self, points: Vec<V3>, color: Rgb, alpha: f64) {
        let center = self.center;
        self.face_from(points, color, alpha, center);
    }

    /// A planar convex polygon whose normal points away from `inside`, a
    /// point in the solid, added as a fan of triangles.
    fn face_from(&mut self, points: Vec<V3>, color: Rgb, alpha: f64, inside: V3) {
        let n = newell(&points);
        if norm(n) < 1e-9 {
            return;
        }
        let Ok(mut normal) = unit(n) else { return };
        if dot(normal, sub(centroid(&points), inside)) < 0. {
            normal = scale(normal, -1.);
        }
        let last = points.len() - 1;
        for k in 1..last {
            let triangle = vec![points[0], points[k], points[k + 1]];
            if norm(newell(&triangle)) < 1e-12 {
                continue;
            }
            let outline = vec![k == 1, true, k + 1 == last];
            self.faces.push(Face {
                points: triangle,
                outline,
                normal,
                color,
                alpha,
                part: self.part.clone(),
            });
        }
    }

    /// A solid of revolution about the local x axis: each profile edge (x, r)
    /// → (x', r') sweeps one band of `SEGMENTS` faces, a triangle fan where it
    /// meets the axis, or one polygon for a flat cap. `edges` gives each
    /// edge's colour and opacity. Each face's normal is the outward normal of
    /// its profile edge (the profile closed along the axis), swept to the
    /// face's angle, so concave faces point the right way.
    fn revolve(&mut self, frame: &Frame, profile: &[(f64, f64)], edges: &[(Rgb, f64)]) {
        debug_assert_eq!(profile.len(), edges.len() + 1);
        let ring = |(x, r): (f64, f64), j: usize| {
            let (s, c) = (TAU * j as f64 / SEGMENTS as f64).sin_cos();
            frame.world([x, r * c, r * s])
        };
        // Twice the signed area of the closed (x, r) profile: positive when
        // it runs counter-clockwise, so the outward side of an edge is its right.
        let turn = (0..profile.len())
            .map(|i| {
                let (a, b) = (profile[i], profile[(i + 1) % profile.len()]);
                a.0 * b.1 - b.0 * a.1
            })
            .sum::<f64>()
            .signum();
        let origin = frame.world([0.; 3]);
        // Outward direction of the band from profile edge a → b at angle j + ½.
        let outward = |a: (f64, f64), b: (f64, f64), j: f64| {
            let (nx, nr) = (turn * (b.1 - a.1), -turn * (b.0 - a.0));
            let (s, c) = (TAU * j / SEGMENTS as f64).sin_cos();
            sub(frame.world([nx, nr * c, nr * s]), origin)
        };
        let add_face = |solid: &mut Self, points: Vec<V3>, out: V3, color: Rgb, alpha: f64| {
            let inside = sub(centroid(&points), out);
            solid.face_from(points, color, alpha, inside);
        };
        for (k, &(color, alpha)) in edges.iter().enumerate() {
            let (a, b) = (profile[k], profile[k + 1]);
            if a.1 == 0. && b.1 == 0. {
                continue;
            }
            if a.0 == b.0 && (a.1 == 0. || b.1 == 0.) {
                let rim = if a.1 == 0. { b } else { a };
                add_face(
                    self,
                    (0..SEGMENTS).map(|j| ring(rim, j)).collect(),
                    outward(a, b, 0.),
                    color,
                    alpha,
                );
                continue;
            }
            for j in 0..SEGMENTS {
                let mut points = vec![ring(a, j)];
                if a.1 != 0. {
                    points.push(ring(a, j + 1));
                }
                if b.1 != 0. {
                    points.push(ring(b, j + 1));
                }
                points.push(ring(b, j));
                add_face(self, points, outward(a, b, j as f64 + 0.5), color, alpha);
            }
        }
    }

    /// An axis-aligned box in the local frame from `low` to `high`.
    fn cuboid(&mut self, frame: &Frame, low: V3, high: V3, color: Rgb, alpha: f64) {
        let corner = |i: usize| {
            frame.world(std::array::from_fn(|k| {
                if i >> k & 1 == 1 { high[k] } else { low[k] }
            }))
        };
        let inside = frame.world(std::array::from_fn(|k| (low[k] + high[k]) / 2.));
        for [a, b, c, d] in [
            [0, 2, 6, 4],
            [1, 5, 7, 3],
            [0, 4, 5, 1],
            [2, 3, 7, 6],
            [0, 1, 3, 2],
            [4, 6, 7, 5],
        ] {
            self.face_from(
                vec![corner(a), corner(b), corner(c), corner(d)],
                color,
                alpha,
                inside,
            );
        }
    }

    /// A prism between two equal convex polygons.
    fn prism(&mut self, low: &[V3], high: &[V3], color: Rgb, alpha: f64) {
        let inside = scale(add(centroid(low), centroid(high)), 0.5);
        for i in 0..low.len() {
            let j = (i + 1) % low.len();
            self.face_from(vec![low[i], low[j], high[j], high[i]], color, alpha, inside);
        }
        self.face_from(low.to_vec(), color, alpha, inside);
        self.face_from(high.to_vec(), color, alpha, inside);
    }

    /// A ring from radius `inner` to `outer` about the local x axis (a disk
    /// when `inner` is 0) between the front plane x = `front` and the back
    /// face x = `back(y)`, which must lie behind the front.
    /// `styles` colour the front, back, outer rim and bore.
    fn ring(
        &mut self,
        frame: &Frame,
        front: f64,
        back: impl Fn(f64) -> f64,
        [inner, outer]: [f64; 2],
        styles: [(Rgb, f64); 4],
    ) {
        let [front_style, back_style, rim_style, bore_style] = styles;
        let angle = |j: usize| (TAU * j as f64 / SEGMENTS as f64).sin_cos();
        let at = |x: f64, r: f64, j: usize| {
            let (s, c) = angle(j);
            [x, r * c, r * s]
        };
        let f = |r: f64, j: usize| at(front, r, j);
        let b = |r: f64, j: usize| {
            let (_, c) = angle(j);
            at(back(r * c), r, j)
        };
        let world = |points: &[V3]| points.iter().map(|&p| frame.world(p)).collect::<Vec<_>>();
        let behind = frame.world([front - 1., 0., 0.]);
        if inner <= 0. {
            self.face_from(
                world(&(0..SEGMENTS).map(|j| f(outer, j)).collect::<Vec<_>>()),
                front_style.0,
                front_style.1,
                behind,
            );
            let back_face: Vec<V3> = (0..SEGMENTS).map(|j| b(outer, j)).collect();
            let ahead = frame.world([front, 0., 0.]);
            self.face_from(world(&back_face), back_style.0, back_style.1, ahead);
        }
        for j in 0..SEGMENTS {
            let k = j + 1;
            if inner > 0. {
                self.face_from(
                    world(&[f(inner, j), f(outer, j), f(outer, k), f(inner, k)]),
                    front_style.0,
                    front_style.1,
                    behind,
                );
                let quad = [b(inner, j), b(outer, j), b(outer, k), b(inner, k)];
                let ahead = frame.world([front, quad[0][1], quad[0][2]]);
                self.face_from(world(&quad), back_style.0, back_style.1, ahead);
                // The bore's material lies outward of it.
                let bore = [f(inner, j), f(inner, k), b(inner, k), b(inner, j)];
                let x = bore.iter().map(|p| p[0]).sum::<f64>() / 4.;
                let (s, c) = (TAU * (j as f64 + 0.5) / SEGMENTS as f64).sin_cos();
                let mid = (inner + outer) / 2.;
                self.face_from(
                    world(&bore),
                    bore_style.0,
                    bore_style.1,
                    frame.world([x, mid * c, mid * s]),
                );
            }
            let rim = [f(outer, j), f(outer, k), b(outer, k), b(outer, j)];
            let x = rim.iter().map(|p| p[0]).sum::<f64>() / 4.;
            self.face_from(
                world(&rim),
                rim_style.0,
                rim_style.1,
                frame.world([x, 0., 0.]),
            );
        }
    }
}

/// A part's local frame: x is its optical axis (an element's surface normal,
/// a source's direction), y and z span its face.
struct Frame {
    origin: V3,
    axes: [V3; 3],
}

impl Frame {
    /// The element frame, its x axis reversed by `side` −1 so +x faces the light.
    fn element(e: &Element, side: f64) -> Self {
        let r = e.rotation();
        let [x, y, z] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]].map(|a| bench::rotate(a, r));
        Self {
            origin: e.position,
            axes: [scale(x, side), scale(y, side), z],
        }
    }

    fn along(origin: V3, x: V3) -> Result<Self, String> {
        let x = unit(x)?;
        let y =
            unit(bench::cross([0., 0., 1.], x)).or_else(|_| unit(bench::cross([0., 1., 0.], x)))?;
        Ok(Self {
            origin,
            axes: [x, y, bench::cross(x, y)],
        })
    }

    fn world(&self, [x, y, z]: V3) -> V3 {
        add(
            self.origin,
            add(
                scale(self.axes[0], x),
                add(scale(self.axes[1], y), scale(self.axes[2], z)),
            ),
        )
    }
}

mod palette {
    use super::Rgb;
    pub const MIRROR: Rgb = [0.70, 0.73, 0.78];
    pub const SILVER: Rgb = [0.86, 0.89, 0.95];
    pub const GLASS: Rgb = [0.56, 0.76, 0.95];
    pub const COATING: Rgb = [0.95, 0.76, 0.32];
    pub const WAVEPLATE: Rgb = [0.74, 0.60, 0.95];
    pub const POLARIZER: Rgb = [0.38, 0.40, 0.46];
    pub const BARREL: Rgb = [0.25, 0.26, 0.30];
    pub const BODY: Rgb = [0.22, 0.24, 0.28];
    pub const SENSOR: Rgb = [0.12, 0.48, 0.38];
    pub const SLM: Rgb = [0.38, 0.22, 0.55];
    pub const IRIS: Rgb = [0.16, 0.16, 0.18];
    pub const BLOCK: Rgb = [0.08, 0.08, 0.09];
    pub const LASER: Rgb = [0.32, 0.34, 0.38];
    pub const EMITTER: Rgb = [0.95, 0.30, 0.20];
    pub const MOUNT: Rgb = [0.56, 0.58, 0.62];
    pub const PERISCOPE: Rgb = [0.62, 0.64, 0.68];
    pub const TABLE: Rgb = [0.44, 0.47, 0.52];
    pub const HOLE: Rgb = [0.18, 0.18, 0.20];
}

/// Thickness of an element's solid when not overridden (mm): a mirror is a
/// sixth of its diameter (6.35 mm for 1"), clamped to 3–25 mm.
pub(crate) fn default_thickness(e: &Element) -> f64 {
    match e.kind {
        _ if matches!(e.action, Some(Action::Slm { .. })) => 15.,
        Kind::Mirror | Kind::CurvedMirror => (e.body_radius / 2.).clamp(3., 25.),
        Kind::Splitter => e.substrate.as_ref().map_or(3., |s| s.thickness_mm),
        Kind::Waveplate => 1.,
        Kind::Polarizer => 2.,
        // Edge thickness; the centre adds both sags.
        Kind::Lens => 2.,
        Kind::LensGroup => 2. * e.body_radius,
        Kind::Detector => e.body_radius,
        Kind::Aperture | Kind::Blocker | Kind::Pbs => 2.,
    }
}

/// Radii of curvature of a lens's two faces, each positive when the face
/// bulges outward: thin-lens lensmaker's equation 1/f = (n − 1)(1/R1 + 1/R2)
/// with equal radii, or a plano back face.
pub(crate) fn lens_bulges(e: &Element) -> [f64; 2] {
    let n = e.index.unwrap_or(GLASS_INDEX);
    match e.lens_form {
        Some(LensForm::Plano) => [(n - 1.) * e.focal_length, f64::INFINITY],
        _ => [2. * (n - 1.) * e.focal_length; 2],
    }
}

/// Signed sag at radius `r` of a sphere of signed radius `radius`, which is
/// kept above `r`.
fn sag(radius: f64, r: f64, rim: f64) -> f64 {
    if !radius.is_finite() || radius == 0. {
        return 0.;
    }
    let radius = radius.signum() * radius.abs().max(rim * 1.001);
    radius.signum() * (radius.abs() - (radius * radius - r * r).sqrt())
}

/// Radial stations across a face of radius `rim`, from the axis out.
fn stations(rim: f64) -> impl DoubleEndedIterator<Item = f64> + Clone {
    (0..=RINGS).map(move |i| rim * i as f64 / RINGS as f64)
}

/// The solid of one element with default lens shapes.
#[cfg(test)]
pub(crate) fn element_solid(e: &Element, thickness: Option<f64>, side: f64) -> Solid {
    shaped_solid(e, thickness, None, side)
}

/// Face radii (positive convex, infinite flat), edge thickness and sag scale
/// of the lens solid as drawn. With a substrate its axial vertices sit on the
/// traced slab, and sags that would meet before the rim are flattened (scale
/// below 1) to keep a tenth of the centre thickness at the edge.
fn lens_profile(
    e: &Element,
    thickness: Option<f64>,
    shape: Option<&LensShape>,
) -> ([f64; 2], f64, f64) {
    let derived = lens_bulges(e);
    let radius = |given: Option<f64>, default: f64| {
        given.map_or(default, |r| if r == 0. { f64::INFINITY } else { r })
    };
    let radii = [
        radius(shape.and_then(|s| s.front_radius_mm), derived[0]),
        radius(shape.and_then(|s| s.back_radius_mm), derived[1]),
    ];
    let rim = e.body_radius;
    let rises = radii.map(|radius| sag(radius, rim, rim));
    let rise = rises[0] + rises[1];
    if let Some(s) = &e.substrate {
        // The traced glass is the slab: both vertices sit on its planes, so the
        // axial thickness is the substrate's. A convex face curves into the slab
        // (scaled so the edge keeps a tenth of the thickness); a concave face,
        // which would leave it, is drawn flat.
        let t = s.thickness_mm;
        let convex: f64 = rises.iter().map(|r| r.max(0.)).sum();
        let k = if t - convex < 0.1 * t {
            0.9 * t / convex
        } else {
            1.
        };
        return (radii, t - k * convex, k);
    }
    let edge = match shape.and_then(|s| s.center_thickness_mm) {
        Some(center) => center - rise,
        // A diverging lens keeps at least 1 mm at its centre.
        None => thickness
            .unwrap_or_else(|| default_thickness(e))
            .max(1. - rise),
    };
    (radii, edge, 1.)
}

/// The solid of one element. A one-sided body (mirror, camera, SLM) sits
/// behind the face the light meets: `side` −1 when light arrives travelling
/// along the element normal. Every other solid keeps the element's own
/// orientation (a plano lens its flat face at −x, a substrate its back face
/// there).
pub(crate) fn shaped_solid(
    e: &Element,
    thickness: Option<f64>,
    shape: Option<&LensShape>,
    side: f64,
) -> Solid {
    use palette::*;
    let one_sided = e.substrate.is_none()
        && matches!(e.kind, Kind::Mirror | Kind::CurvedMirror | Kind::Detector);
    let frame = Frame::element(e, if one_sided { side } else { 1. });
    let t = thickness.unwrap_or_else(|| {
        e.substrate
            .as_ref()
            .map_or_else(|| default_thickness(e), |s| s.thickness_mm)
    });
    let rim = e.body_radius;
    let mut solid = Solid::new(&e.id, e.position);
    let disk = |solid: &mut Solid, front: f64, back: f64, face: (Rgb, f64), body: (Rgb, f64)| {
        solid.center = frame.world([(front + back) / 2., 0., 0.]);
        solid.revolve(
            &frame,
            &[(front, 0.), (front, rim), (back, rim), (back, 0.)],
            &[face, body, body],
        );
    };
    match (e.kind, &e.action) {
        (Kind::Mirror | Kind::CurvedMirror, Some(Action::Slm { size_mm, .. })) => {
            let half = size_mm.map(|s| s / 2.);
            let body = [half[0] + 4., half[1] + 4.];
            solid.center = frame.world([-t / 2., 0., 0.]);
            solid.cuboid(
                &frame,
                [-t, -body[0], -body[1]],
                [0., body[0], body[1]],
                BODY,
                OPAQUE,
            );
            solid.face(
                [[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]]
                    .iter()
                    .map(|[y, z]| frame.world([0.05, y * half[0], z * half[1]]))
                    .collect(),
                SLM,
                OPAQUE,
            );
        }
        (Kind::Aperture, Some(Action::Slm { size_mm, .. })) => {
            // Transmissive SLM: a frame around a translucent active panel the
            // beam passes through, centred on the element plane.
            let half = size_mm.map(|s| s / 2.);
            let body = [half[0] + 4., half[1] + 4.];
            let (front, back) = (t / 2., -t / 2.);
            solid.center = e.position;
            for (low, high) in [
                ([back, half[0], -body[1]], [front, body[0], body[1]]),
                ([back, -body[0], -body[1]], [front, -half[0], body[1]]),
                ([back, -half[0], half[1]], [front, half[0], body[1]]),
                ([back, -half[0], -body[1]], [front, half[0], -half[1]]),
            ] {
                solid.cuboid(&frame, low, high, BODY, OPAQUE);
            }
            solid.cuboid(
                &frame,
                [-0.5, -half[0], -half[1]],
                [0.5, half[0], half[1]],
                SLM,
                0.55,
            );
        }
        (Kind::Mirror, _) => disk(&mut solid, 0., -t, (SILVER, OPAQUE), (MIRROR, OPAQUE)),
        (Kind::CurvedMirror, _) => {
            // Mirror radius 2f: concave (f > 0) dishes toward +x.
            let radius = 2. * e.focal_length;
            let front = |r: f64| sag(radius, r, rim);
            let back = -t.max(-front(rim) + 1.);
            let mut profile: Vec<(f64, f64)> = stations(rim).map(|r| (front(r), r)).collect();
            profile.extend([(back, rim), (back, 0.)]);
            let mut edges = vec![(SILVER, OPAQUE); RINGS];
            edges.extend([(MIRROR, OPAQUE); 2]);
            solid.center = frame.world([back / 2., 0., 0.]);
            solid.revolve(&frame, &profile, &edges);
        }
        (Kind::Lens, _) => {
            // Each face bulges out of the edge planes ±t/2 by its sag. With a
            // substrate the axial vertices sit at x = 0 and −thickness and the
            // back face tilts by the wedge, as the traced slab does.
            let ([front, back], t, k) = lens_profile(e, thickness, shape);
            let rise = [front, back].map(|radius| k * sag(radius, rim, rim));
            let vertices = match &e.substrate {
                Some(s) => [0., -s.thickness_mm],
                None => [t / 2. + rise[0], -t / 2. - rise[1]],
            };
            // With a substrate a concave face is drawn flat on its slab plane.
            let scales = [front, back].map(|radius| {
                if e.substrate.is_some() && sag(radius, rim, rim) < 0. {
                    0.
                } else {
                    k
                }
            });
            let surface = |r: f64| {
                [
                    vertices[0] - scales[0] * sag(front, r, rim),
                    vertices[1] + scales[1] * sag(back, r, rim),
                ]
            };
            let mut profile: Vec<(f64, f64)> = stations(rim).map(|r| (surface(r)[0], r)).collect();
            profile.extend(stations(rim).rev().map(|r| (surface(r)[1], r)));
            solid.center = frame.world([(vertices[0] + vertices[1]) / 2., 0., 0.]);
            solid.revolve(&frame, &profile, &vec![(GLASS, 0.55); 2 * RINGS + 1]);
            if let Some(s) = e
                .substrate
                .as_ref()
                .filter(|s| s.wedge_deg.is_some_and(|w| w != 0.))
            {
                // Shear by depth below the front vertex, so the back vertex and a
                // flat back face land on the traced wedged face, x = −t − y·tan(wedge),
                // and curved faces stay inside the slab.
                let (t, wedge) = (s.thickness_mm, s.wedge_deg.unwrap_or(0.).to_radians().tan());
                let local = |p: V3| [0, 1, 2].map(|i| dot(sub(p, frame.origin), frame.axes[i]));
                for face in &mut solid.faces {
                    for p in &mut face.points {
                        let [x, y, z] = local(*p);
                        *p = frame.world([x + x / t * y * wedge, y, z]);
                    }
                    if let Ok(n) = unit(newell(&face.points)) {
                        face.normal = if dot(n, face.normal) < 0. {
                            scale(n, -1.)
                        } else {
                            n
                        };
                    }
                }
            }
        }
        (Kind::LensGroup, _) => disk(
            &mut solid,
            t / 2.,
            -t / 2.,
            (GLASS, OPAQUE),
            (BARREL, OPAQUE),
        ),
        (Kind::Aperture | Kind::Splitter | Kind::Pbs | Kind::Waveplate | Kind::Polarizer, _)
            if e.substrate.is_some() =>
        {
            // The traced slab: x from 0 back to −t, its back face wedged
            // about the local z axis.
            let substrate = e.substrate.as_ref().expect("guarded");
            let wedge = substrate.wedge_deg.unwrap_or(0.).to_radians().tan();
            let back = |y: f64| -t - wedge * y;
            solid.center = frame.world([-t / 2., 0., 0.]);
            let glass = (GLASS, 0.35);
            let tint = match e.kind {
                Kind::Waveplate => (WAVEPLATE, 0.4),
                Kind::Polarizer => (POLARIZER, 0.5),
                _ => glass,
            };
            let coated = |face| {
                if matches!(e.kind, Kind::Splitter | Kind::Pbs)
                    && substrate.coated_face == Some(face)
                {
                    (COATING, 0.45)
                } else {
                    tint
                }
            };
            if e.kind == Kind::Aperture {
                // A compensator: glass across the opening, held in an opaque ring.
                let a = e.aperture.min(rim);
                solid.ring(&frame, 0., back, [0., a], [glass; 4]);
                if a < rim * 0.999 {
                    solid.ring(&frame, 0., back, [a, rim], [(IRIS, OPAQUE); 4]);
                }
            } else {
                solid.ring(
                    &frame,
                    0.,
                    back,
                    [0., rim],
                    [
                        coated(bench::Face::Front),
                        coated(bench::Face::Back),
                        tint,
                        tint,
                    ],
                );
            }
        }
        (Kind::Splitter, _) => {
            solid.center = frame.world([-t / 2., 0., 0.]);
            solid.revolve(
                &frame,
                &[(0., 0.), (0., rim), (-t, rim), (-t, 0.)],
                &[(COATING, 0.45), (GLASS, 0.35), (GLASS, 0.35)],
            );
        }
        (Kind::Waveplate, _) => disk(
            &mut solid,
            t / 2.,
            -t / 2.,
            (WAVEPLATE, 0.4),
            (WAVEPLATE, 0.4),
        ),
        (Kind::Polarizer, _) => disk(
            &mut solid,
            t / 2.,
            -t / 2.,
            (POLARIZER, 0.5),
            (POLARIZER, 0.5),
        ),
        (Kind::Pbs, _) => {
            // Without a substrate, a cube whose diagonal plane is the element plane: the coating
            // diagonal spans the body diameter.
            let h = rim / 2_f64.sqrt();
            let cube = Frame {
                origin: e.position,
                axes: [
                    add(frame.axes[0], frame.axes[1]),
                    sub(frame.axes[1], frame.axes[0]),
                    frame.axes[2],
                ]
                .map(|a| unit(a).unwrap_or(a)),
            };
            solid.cuboid(&cube, [-h, -h, -h], [h, h, h], GLASS, 0.35);
            solid.face(
                [[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]]
                    .iter()
                    .map(|[y, z]| frame.world([0., y * rim, z * h]))
                    .collect(),
                COATING,
                0.6,
            );
        }
        (Kind::Detector, _) => {
            let body = rim * 0.9;
            solid.center = frame.world([-t / 2., 0., 0.]);
            solid.cuboid(&frame, [-t, -body, -body], [0., body, body], BODY, OPAQUE);
            let a = e.aperture.min(body);
            solid.face(
                [[-a, -a], [a, -a], [a, a], [-a, a]]
                    .iter()
                    .map(|&[y, z]| frame.world([0.05, y, z]))
                    .collect(),
                SENSOR,
                OPAQUE,
            );
        }
        (Kind::Aperture, _) => {
            let a = e.aperture.min(rim * 0.999);
            solid.center = frame.origin;
            solid.ring(&frame, t / 2., |_| -t / 2., [a, rim], [(IRIS, OPAQUE); 4]);
        }
        (Kind::Blocker, _) => disk(
            &mut solid,
            t / 2.,
            -t / 2.,
            (BLOCK, OPAQUE),
            (BLOCK, OPAQUE),
        ),
    }
    solid
}

/// A laser head behind the source with its emitting face.
fn source_solid(s: &bench::Source) -> Result<Solid, String> {
    use palette::*;
    let frame = Frame::along(s.position, s.direction)?;
    let (length, half) = (60., 12_f64.max(2. * s.beam_radius));
    let mut solid = Solid::new(&s.id, frame.world([-length / 2., 0., 0.]));
    solid.cuboid(
        &frame,
        [-length, -half, -half],
        [0., half, half],
        LASER,
        OPAQUE,
    );
    let r = s.beam_radius.max(1.).min(half);
    solid.face(
        (0..16)
            .map(|i| {
                let (sin, cos) = (TAU * i as f64 / 16.).sin_cos();
                frame.world([0.05, r * cos, r * sin])
            })
            .collect(),
        EMITTER,
        OPAQUE,
    );
    Ok(solid)
}

fn polygon_at(center: V3, radius: f64, sides: usize, z: f64) -> Vec<V3> {
    (0..sides)
        .map(|i| {
            let (s, c) = (TAU * i as f64 / sides as f64).sin_cos();
            [center[0] + radius * c, center[1] + radius * s, z]
        })
        .collect()
}

/// Every solid of the scene: elements, sources, periscope columns and, when
/// asked, the table, its holes and the mounts.
fn scene_solids(
    bench: &Bench,
    trace: &Trace,
    options: &Layout3dOptions,
) -> Result<Vec<Solid>, String> {
    use palette::*;
    let mut solids = Vec::new();
    let top = bench.origin[2];
    let show = options.show.unwrap_or_default();
    if show.table {
        let [w, h] = bench.table_size;
        let frame = Frame {
            origin: bench.origin,
            axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        };
        let mut table = Solid::ground(
            "table",
            Ground::Table,
            add(bench.origin, [w / 2., h / 2., -6.35]),
        );
        table.cuboid(&frame, [0., 0., -12.7], [w, h, 0.], TABLE, OPAQUE);
        solids.push(table);
    }
    let grid = Grid::new(bench)?;
    if show.holes
        && let Ok(holes) = grid.drawing_holes()
    {
        let mut drilled = Solid::ground("holes", Ground::Holes, add(bench.origin, [0., 0., -1.]));
        for hole in holes {
            let p = grid.point(hole);
            drilled.face(polygon_at(p, 3., 8, top + 0.02), HOLE, OPAQUE);
        }
        solids.push(drilled);
    }
    for e in &bench.elements {
        if show.mounts
            && let Some(mount) = &e.mount
        {
            let mut post = Solid::new(
                &format!("{}:mount", e.id),
                [e.position[0], e.position[1], top],
            );
            let base = base_corners(e, mount);
            post.prism(
                &base.map(|[x, y, _]| [x, y, top]),
                &base.map(|[x, y, _]| [x, y, top + 6.]),
                MOUNT,
                OPAQUE,
            );
            let post_top = e.position[2] - e.body_radius;
            if post_top > top + 6. {
                let radius = 6.35_f64.min(mount.base_size[0].min(mount.base_size[1]) / 2.);
                let ring = polygon_at(e.position, radius, 16, top + 6.);
                post.prism(
                    &ring,
                    &ring
                        .iter()
                        .map(|&[x, y, _]| [x, y, post_top])
                        .collect::<Vec<_>>(),
                    MOUNT,
                    OPAQUE,
                );
            }
            solids.push(post);
        }
        let thickness = options
            .thickness_mm
            .as_ref()
            .and_then(|m| m.get(&e.id))
            .copied();
        let shape = options.lens_shape.as_ref().and_then(|m| m.get(&e.id));
        solids.push(shaped_solid(e, thickness, shape, lit_side(e, trace)));
    }
    for s in &bench.sources {
        solids.push(source_solid(s)?);
    }
    let element = |id: &str| bench.elements.iter().find(|e| e.id == id);
    for p in &bench.periscopes {
        let (Some(lower), Some(upper)) = (element(&p.lower), element(&p.upper)) else {
            continue;
        };
        let radius = lower.body_radius.min(upper.body_radius) * 0.8;
        let low = polygon_at(lower.position, radius, 12, lower.position[2]);
        let high: Vec<V3> = polygon_at(upper.position, radius, 12, upper.position[2]);
        let mut column = Solid::new(&p.id, scale(add(lower.position, upper.position), 0.5));
        column.prism(&low, &high, PERISCOPE, 0.18);
        solids.push(column);
    }
    Ok(solids)
}

/// −1 when the light reaching `e` travels along its normal, else 1.
fn lit_side(e: &Element, trace: &Trace) -> f64 {
    let normal = bench::rotate([1., 0., 0.], e.rotation());
    trace
        .segments
        .iter()
        .filter(|s| s.status != "substrate" && s.element.as_deref() == Some(e.id.as_str()))
        .max_by(|a, b| a.power.total_cmp(&b.power))
        .map_or(1., |s| {
            if dot(sub(s.to, s.from), normal) > 0. {
                -1.
            } else {
                1.
            }
        })
}

/// Hexagonal rings holding at least `rays` rays: 1 + 3n(n+1) ≥ rays.
pub(crate) fn rings_for(rays: usize) -> usize {
    (0..8).find(|n| 1 + 3 * n * (n + 1) >= rays).unwrap_or(8)
}

/// Whether launch sample `index` (the axis, then ring k's 6k samples) is
/// one of the `rays` drawn: the inner rings in full and the outer ring
/// thinned to evenly spaced samples.
pub(crate) fn drawn_sample(index: usize, rays: usize) -> bool {
    let rings = rings_for(rays);
    let inner = if rings == 0 {
        0
    } else {
        1 + 3 * (rings - 1) * rings
    };
    if index < inner {
        return true;
    }
    let (outer, wanted) = (6 * rings.max(1), rays - inner);
    let j = index - inner;
    // Sample m of `wanted` sits at ⌊m·outer/wanted⌋.
    j < outer && (0..wanted).any(|m| m * outer / wanted == j)
}

/// The launch sample a segment descends from, from its branch label
/// `source:index[/…]`.
fn sample_index(source: &str, branch: &str) -> Option<usize> {
    let rest = branch.strip_prefix(source)?.strip_prefix(':')?;
    rest[..rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len())]
        .parse()
        .ok()
}

/// Approximate sRGB of a visible wavelength after Dan Bruton, "Approximate RGB
/// values for visible wavelengths" (1996): piecewise-linear hues over
/// 380–780 nm, dimmed toward both ends, gamma 0.8. Ultraviolet is drawn violet
/// and infrared crimson as pseudo-colours.
pub(crate) fn wavelength_rgb(nm: f64) -> Rgb {
    if nm < 380. {
        return [0.45, 0.10, 0.75];
    }
    if nm > 780. {
        return [0.75, 0.08, 0.35];
    }
    let (r, g, b) = match nm {
        nm if nm < 440. => (-(nm - 440.) / 60., 0., 1.),
        nm if nm < 490. => (0., (nm - 440.) / 50., 1.),
        nm if nm < 510. => (0., 1., -(nm - 510.) / 20.),
        nm if nm < 580. => ((nm - 510.) / 70., 1., 0.),
        nm if nm < 645. => (1., -(nm - 645.) / 65., 0.),
        _ => (1., 0., 0.),
    };
    let factor = match nm {
        nm if nm < 420. => 0.3 + 0.7 * (nm - 380.) / 40.,
        nm if nm > 700. => 0.3 + 0.7 * (780. - nm) / 80.,
        _ => 1.,
    };
    [r, g, b].map(|c: f64| if c > 0. { (c * factor).powf(0.8) } else { 0. })
}

fn hex(c: Rgb) -> String {
    let [r, g, b] = c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// One traced ray piece in bench coordinates.
#[derive(Clone, Debug)]
pub(crate) struct Ray {
    pub from: V3,
    pub to: V3,
    pub color: String,
    /// Escaped or missed its target: drawn grey and dashed.
    pub stray: bool,
}

/// Rays of every source sampled into hexagonal bundles and traced along the
/// solved paths, with the legend of their colours.
fn bundle_trace(
    bench: &Bench,
    system: &System,
    options: &Layout3dOptions,
) -> Result<Trace, String> {
    let rings = rings_for(options.rays());
    let mut scene = bench.clone();
    for s in &mut scene.sources {
        s.rings = rings;
    }
    for e in &mut scene.elements {
        e.scatter = None;
        e.ghosts = 0;
    }
    scene.trace_resolved(system)
}

fn rays(
    bench: &Bench,
    trace: &Trace,
    options: &Layout3dOptions,
) -> (Vec<Ray>, Vec<(String, String)>) {
    // A beam is identified by its port history; each named branch claims the
    // beams of its paths' segments, first branch first.
    let mut branch_of: HashMap<&[Port], &str> = HashMap::new();
    for report in &trace.branches {
        for &i in report.paths.iter().flat_map(|p| &p.segment_indices) {
            branch_of
                .entry(trace.segments[i].beam.as_slice())
                .or_insert(&report.id);
        }
    }
    let strongest: HashMap<&str, f64> = trace.segments.iter().fold(HashMap::new(), |mut m, s| {
        let best = m.entry(s.source.as_str()).or_insert(0.);
        *best = s.power.max(*best);
        m
    });
    let source_index = |id: &str| bench.sources.iter().position(|s| s.id == id).unwrap_or(0);
    let [w, h] = bench.table_size;
    let (low, high) = (
        sub(bench.origin, [0.05 * w, 0.05 * h, 0.]),
        add(bench.origin, [1.05 * w, 1.05 * h, 0.]),
    );
    let mut legend: Vec<(String, String)> = Vec::new();
    let mut note = |color: &str, label: String| {
        if !legend.iter().any(|(c, l)| c == color && *l == label) {
            legend.push((color.to_owned(), label));
        }
    };
    let mut out = Vec::new();
    let wanted = options.rays();
    for s in &trace.segments {
        if s.power < MIN_POWER_FRACTION * strongest[s.source.as_str()]
            || !sample_index(&s.source, &s.branch).is_some_and(|i| drawn_sample(i, wanted))
        {
            continue;
        }
        let stray = matches!(s.status.as_str(), "escaped" | "missed_target");
        let mut to = s.to;
        if stray {
            // Stray light ends where it leaves the table (plus 5 %).
            let d = sub(s.to, s.from);
            let leave = (0..2)
                .filter(|&k| d[k] != 0.)
                .map(|k| ((if d[k] > 0. { high[k] } else { low[k] }) - s.from[k]) / d[k])
                .fold(1_f64, f64::min)
                .max(0.);
            to = add(s.from, scale(d, leave));
        }
        let color = if stray {
            note(STRAY, "escaped or missed".into());
            STRAY.to_owned()
        } else {
            match options.color_by.unwrap_or_default() {
                ColorBy::Branch => match branch_of.get(s.beam.as_slice()) {
                    Some(id) => {
                        let color = drawing::branch_style(bench, id).0;
                        note(&color, (*id).to_owned());
                        color
                    }
                    None => {
                        note(drawing::NEUTRAL, "no named branch".into());
                        drawing::NEUTRAL.to_owned()
                    }
                },
                ColorBy::Wavelength => {
                    let nm = bench.sources[source_index(&s.source)].wavelength;
                    let color = hex(wavelength_rgb(nm));
                    note(&color, format!("{nm} nm"));
                    color
                }
                ColorBy::Field => {
                    let color = drawing::PALETTE[source_index(&s.source) % drawing::PALETTE.len()]
                        .to_owned();
                    note(&color, s.source.clone());
                    color
                }
            }
        };
        out.push(Ray {
            from: s.from,
            to,
            color,
            stray,
        });
    }
    (out, legend)
}

/// Orthographic camera: unit page axes `ex` (right) and `ey` (up), depth
/// axis `ez` toward the viewer, all about the table centre.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Camera {
    pub ex: V3,
    pub ey: V3,
    pub ez: V3,
    pub center: V3,
    /// Page millimetres per bench millimetre.
    pub scale: f64,
    pub offset: P2,
}

impl Camera {
    fn new(azimuth_deg: f64, elevation_deg: f64, center: V3) -> Self {
        let (sa, ca) = azimuth_deg.to_radians().sin_cos();
        let (sp, cp) = elevation_deg.to_radians().sin_cos();
        Self {
            ex: [ca, -sa, 0.],
            ey: [sa * sp, ca * sp, cp],
            ez: [-sa * cp, -ca * cp, sp],
            center,
            scale: 1.,
            offset: [0., 0.],
        }
    }

    /// Page position (mm, y down) of a bench point.
    pub fn page(&self, p: V3) -> P2 {
        let d = sub(p, self.center);
        [
            self.offset[0] + self.scale * dot(d, self.ex),
            self.offset[1] - self.scale * dot(d, self.ey),
        ]
    }

    /// Bench millimetres toward the viewer.
    pub fn depth(&self, p: V3) -> f64 {
        dot(sub(p, self.center), self.ez)
    }

    /// The bench-frame line through a page point, as (point at depth 0, ez).
    fn unproject(&self, [u, v]: P2) -> V3 {
        let (x, y) = (
            (u - self.offset[0]) / self.scale,
            -(v - self.offset[1]) / self.scale,
        );
        add(self.center, add(scale(self.ex, x), scale(self.ey, y)))
    }
}

/// Yaw, pitch, zoom and pan of the options.
fn view(options: &Layout3dOptions) -> Result<(f64, f64, f64, P2), String> {
    match (&options.view, options.preset) {
        (Some(_), Some(_)) => Err("Give layout3d either view or preset, not both".into()),
        (Some(v), None) => {
            if !(v.azimuth_deg.is_finite()
                && v.elevation_deg.abs() <= 90.
                && v.zoom > 0.
                && v.zoom.is_finite())
            {
                return Err("layout3d view needs a finite azimuth, elevation within ±90° and a positive zoom".into());
            }
            Ok((v.azimuth_deg, v.elevation_deg, v.zoom, v.pan_mm))
        }
        (None, preset) => Ok(match preset.unwrap_or(Preset::Iso) {
            Preset::Iso => (-45., (1. / 2_f64.sqrt()).atan().to_degrees(), 1., [0., 0.]),
            Preset::Top => (0., 90., 1., [0., 0.]),
            Preset::Side => (0., 0., 1., [0., 0.]),
        }),
    }
}

/// A projected face or ray piece in paint order.
#[derive(Clone, Debug)]
pub(crate) enum Mark {
    /// `outline` flags the edges drawn in outline mode (see [`Face`]).
    Face {
        points: Vec<P2>,
        outline: Vec<bool>,
        fill: String,
        alpha: f64,
        part: String,
    },
    Ray {
        from: P2,
        to: P2,
        color: String,
        stray: bool,
    },
}

/// The page-projected face with its plane, for depth queries.
struct Flat {
    points: Vec<P2>,
    low: P2,
    high: P2,
    plane: (V3, V3),
    alpha: f64,
}

impl Flat {
    fn contains(&self, p: P2) -> bool {
        if p[0] < self.low[0] || p[0] > self.high[0] || p[1] < self.low[1] || p[1] > self.high[1] {
            return false;
        }
        let mut sign = 0.;
        for (i, a) in self.points.iter().enumerate() {
            let b = self.points[(i + 1) % self.points.len()];
            let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
            if c.abs() < 1e-12 {
                continue;
            }
            if sign * c < 0. {
                return false;
            }
            sign = c;
        }
        true
    }

    /// Depth of the face plane under a page point, or None edge-on.
    fn depth_at(&self, camera: &Camera, p: P2) -> Option<f64> {
        let (point, normal) = self.plane;
        let along = dot(normal, camera.ez);
        (along.abs() > 1e-9).then(|| {
            let base = camera.unproject(p);
            camera.depth(base) + dot(sub(point, base), normal) / along
        })
    }
}

/// Parameter along a→b where it crosses c→d, both page segments.
fn crossing(a: P2, b: P2, c: P2, d: P2) -> Option<f64> {
    let (r, s) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-15 {
        return None;
    }
    let q = [c[0] - a[0], c[1] - a[1]];
    let t = (q[0] * s[1] - q[1] * s[0]) / den;
    let u = (q[0] * r[1] - q[1] * r[0]) / den;
    (t > 0. && t < 1. && (-1e-12..=1. + 1e-12).contains(&u)).then_some(t)
}

/// Twice the signed area of a page polygon.
fn area2(points: &[P2]) -> f64 {
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// The part of convex `subject` inside convex `clip` (Sutherland–Hodgman).
fn clip(subject: &[P2], clip: &[P2]) -> Vec<P2> {
    let orient = area2(clip).signum();
    let mut out = subject.to_vec();
    for (i, &a) in clip.iter().enumerate() {
        let b = clip[(i + 1) % clip.len()];
        let side = |p: P2| orient * ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]));
        let input = std::mem::take(&mut out);
        for (k, &q) in input.iter().enumerate() {
            let p = input[(k + input.len() - 1) % input.len()];
            let (sp, sq) = (side(p), side(q));
            if (sp >= 0.) != (sq >= 0.) {
                let t = sp / (sp - sq);
                out.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
            }
            if sq >= 0. {
                out.push(q);
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// Area centroid of a page polygon with twice-area `twice`.
fn page_centroid(points: &[P2], twice: f64) -> P2 {
    let mut c = [0.; 2];
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        let w = a[0] * b[1] - b[0] * a[1];
        c = [c[0] + (a[0] + b[0]) * w, c[1] + (a[1] + b[1]) * w];
    }
    [c[0] / (3. * twice), c[1] / (3. * twice)]
}

/// Smallest overlap (page mm²) that orders two faces.
const MIN_OVERLAP: f64 = 1e-6;

/// Which of two faces is nearer where their page outlines overlap: their
/// planes are compared at the centroid of the overlap. None when they do not
/// overlap or meet there.
fn nearer(a: &Flat, b: &Flat, camera: &Camera) -> Option<std::cmp::Ordering> {
    if a.low[0] >= b.high[0]
        || b.low[0] >= a.high[0]
        || a.low[1] >= b.high[1]
        || b.low[1] >= a.high[1]
    {
        return None;
    }
    let shared = clip(&a.points, &b.points);
    if shared.len() < 3 {
        return None;
    }
    let twice = area2(&shared);
    if twice.abs() < 2. * MIN_OVERLAP {
        return None;
    }
    let at = page_centroid(&shared, twice);
    let (da, db) = (a.depth_at(camera, at)?, b.depth_at(camera, at)?);
    ((da - db).abs() > 1e-7 * (1. + da.abs())).then(|| da.total_cmp(&db))
}

/// A face to paint, possibly a piece of a solid's face split for ordering.
struct Piece {
    points: Vec<V3>,
    outline: Vec<bool>,
    normal: V3,
    color: Rgb,
    alpha: f64,
    part: String,
    /// Table and holes, painted first.
    ground: bool,
    holes: bool,
}

/// Plane-distance tolerance (bench mm) for splitting faces.
const SPLIT_EPS: f64 = 1e-6;

/// Signed distances of `points` from the plane through `q` with normal `n`.
fn distances(points: &[V3], (q, n): (V3, V3)) -> impl Iterator<Item = f64> + '_ {
    points.iter().map(move |&p| dot(n, sub(p, q)))
}

/// Whether a face has vertices on both sides of a plane.
fn straddles(points: &[V3], plane: (V3, V3)) -> bool {
    let (low, high) =
        distances(points, plane).fold((0_f64, 0_f64), |(l, h), d| (l.min(d), h.max(d)));
    low < -SPLIT_EPS && high > SPLIT_EPS
}

/// The parts of a convex face on either side of a plane, each with its
/// outline flags: pieces of an outline edge stay outline, the new edges along
/// the cut do not. A face that does not cross the plane comes back whole.
fn split_face(points: &[V3], outline: &[bool], plane: (V3, V3)) -> Vec<(Vec<V3>, Vec<bool>)> {
    if !straddles(points, plane) {
        return vec![(points.to_vec(), outline.to_vec())];
    }
    let d: Vec<f64> = distances(points, plane).collect();
    let (mut front, mut back) = ((Vec::new(), Vec::new()), (Vec::new(), Vec::new()));
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        let (p, dp, dq, edge) = (points[i], d[i], d[j], outline[i]);
        // A vertex's outgoing edge follows edge i unless it runs along the cut.
        if dp >= -SPLIT_EPS {
            front.0.push(p);
            front.1.push(edge && (dp > SPLIT_EPS || dq >= -SPLIT_EPS));
        }
        if dp <= SPLIT_EPS {
            back.0.push(p);
            back.1.push(edge && (dp < -SPLIT_EPS || dq <= SPLIT_EPS));
        }
        if (dp > SPLIT_EPS && dq < -SPLIT_EPS) || (dp < -SPLIT_EPS && dq > SPLIT_EPS) {
            let x = add(p, scale(sub(points[j], p), dp / (dp - dq)));
            front.0.push(x);
            front.1.push(edge && dq > 0.);
            back.0.push(x);
            back.1.push(edge && dq < 0.);
        }
    }
    [front, back]
        .into_iter()
        .filter(|(f, _)| f.len() >= 3 && dot(newell(f), newell(f)) > 1e-18)
        .collect()
}

/// The page outline of a piece, for overlap and depth queries.
fn flat_of(piece: &Piece, camera: &Camera) -> Flat {
    let points: Vec<P2> = piece.points.iter().map(|&p| camera.page(p)).collect();
    let low = points
        .iter()
        .fold([f64::INFINITY; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
    let high = points.iter().fold([f64::NEG_INFINITY; 2], |m, p| {
        [m[0].max(p[0]), m[1].max(p[1])]
    });
    Flat {
        points,
        low,
        high,
        plane: (piece.points[0], piece.normal),
        alpha: piece.alpha,
    }
}

/// Whether two page outlines share more than `MIN_OVERLAP`.
fn overlaps(a: &Flat, b: &Flat) -> bool {
    if a.low[0] >= b.high[0]
        || b.low[0] >= a.high[0]
        || a.low[1] >= b.high[1]
        || b.low[1] >= a.high[1]
    {
        return false;
    }
    let shared = clip(&a.points, &b.points);
    shared.len() >= 3 && area2(&shared).abs() >= 2. * MIN_OVERLAP
}

/// Pairs (i, j), i before j in `flats`, whose page outlines overlap.
fn overlapping_pairs(flats: &[&Flat]) -> Vec<(usize, usize)> {
    let mut by_low: Vec<usize> = (0..flats.len()).collect();
    by_low.sort_by(|&i, &j| flats[i].low[0].total_cmp(&flats[j].low[0]));
    let mut pairs = Vec::new();
    for (k, &i) in by_low.iter().enumerate() {
        for &j in &by_low[k + 1..] {
            if flats[j].low[0] >= flats[i].high[0] {
                break;
            }
            if overlaps(flats[i], flats[j]) {
                pairs.push((i.min(j), i.max(j)));
            }
        }
    }
    pairs
}

/// Strongly connected components of a directed graph with more than one node.
fn cycles(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    // Kosaraju: finish order on the graph, then components on its reverse.
    let n = edges.len();
    let mut reverse = vec![Vec::new(); n];
    for (i, out) in edges.iter().enumerate() {
        for &j in out {
            reverse[j].push(i);
        }
    }
    let (mut seen, mut finished) = (vec![false; n], Vec::with_capacity(n));
    for start in 0..n {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![(start, 0)];
        while let Some((node, next)) = stack.pop() {
            if let Some(&j) = edges[node].get(next) {
                stack.push((node, next + 1));
                if !seen[j] {
                    seen[j] = true;
                    stack.push((j, 0));
                }
            } else {
                finished.push(node);
            }
        }
    }
    let mut component = vec![usize::MAX; n];
    let mut found = Vec::new();
    for &start in finished.iter().rev() {
        if component[start] != usize::MAX {
            continue;
        }
        let id = found.len();
        let (mut members, mut stack) = (Vec::new(), vec![start]);
        component[start] = id;
        while let Some(node) = stack.pop() {
            members.push(node);
            for &j in &reverse[node] {
                if component[j] == usize::MAX {
                    component[j] = id;
                    stack.push(j);
                }
            }
        }
        found.push(members);
    }
    found.retain(|c| c.len() > 1);
    found
}

/// Splits pieces until a painter's order exists: a face crossing the plane of
/// a face it overlaps on the page (they interpenetrate, so neither full face
/// is in front everywhere) is cut along that plane, and faces left in an
/// ordering cycle are cut along each other's planes. Once no face crosses the
/// plane of another, each one keeps the others wholly on one side, so the
/// depth relations are acyclic. Splitting stops at a budget; `order` breaks
/// any cycle left.
fn refine(mut pieces: Vec<Piece>, camera: &Camera) -> Vec<Piece> {
    let budget = 4 * pieces.len() + 256;
    for _ in 0..6 {
        let solid: Vec<usize> = (0..pieces.len()).filter(|&i| !pieces[i].ground).collect();
        let flats: Vec<Flat> = solid.iter().map(|&i| flat_of(&pieces[i], camera)).collect();
        let pairs = overlapping_pairs(&flats.iter().collect::<Vec<_>>());
        // Planes to cut each piece along.
        let mut cuts: Vec<Vec<(V3, V3)>> = vec![Vec::new(); pieces.len()];
        let mut edges = vec![Vec::new(); flats.len()];
        for &(i, j) in &pairs {
            let (a, b) = (&pieces[solid[i]], &pieces[solid[j]]);
            let (pa, pb) = ((a.points[0], a.normal), (b.points[0], b.normal));
            if straddles(&a.points, pb) && straddles(&b.points, pa) {
                cuts[solid[i]].push(pb);
                continue;
            }
            match nearer(&flats[i], &flats[j], camera) {
                Some(std::cmp::Ordering::Less) => edges[i].push(j),
                Some(_) => edges[j].push(i),
                None => {}
            }
        }
        for members in cycles(&edges) {
            for &i in &members {
                for &j in &members {
                    let other = &pieces[solid[j]];
                    if i != j
                        && straddles(&pieces[solid[i]].points, (other.points[0], other.normal))
                    {
                        cuts[solid[i]].push((other.points[0], other.normal));
                    }
                }
            }
        }
        if cuts.iter().all(Vec::is_empty) {
            break;
        }
        let mut next = Vec::with_capacity(pieces.len());
        for (piece, planes) in pieces.into_iter().zip(cuts) {
            let mut parts = vec![(piece.points.clone(), piece.outline.clone())];
            for plane in planes {
                parts = parts
                    .iter()
                    .flat_map(|(p, o)| split_face(p, o, plane))
                    .collect();
            }
            next.extend(parts.into_iter().map(|(points, outline)| Piece {
                points,
                outline,
                part: piece.part.clone(),
                ..piece
            }));
        }
        pieces = next;
        if pieces.len() > budget {
            break;
        }
    }
    pieces
}

/// Shaded faces and ray pieces in back-to-front order. Faces are first split
/// where they interpenetrate or overlap cyclically (`refine`); faces whose
/// page outlines overlap are then ordered by their depth inside the overlap.
/// Ray pieces are split where they cross a face outline on the page or pierce
/// its plane, so every piece lies wholly in front of or behind each face it
/// overlaps: a piece behind an opaque face is dropped, and the others follow
/// the faces behind them and precede the translucent faces in front. The
/// order is a topological sort of these relations, ties and any remaining
/// cycle going to the farther centre first.
fn paint(solids: &[Solid], rays: &[Ray], camera: &Camera, under_table: bool) -> Vec<Mark> {
    let light = unit(add(
        add(scale(camera.ex, -0.35), scale(camera.ey, 0.6)),
        scale(camera.ez, 0.72),
    ))
    .unwrap_or(camera.ez);
    let mut pieces = Vec::new();
    for solid in solids {
        let ground = !under_table && solid.ground != Ground::Part;
        for face in &solid.faces {
            if face.alpha >= OPAQUE && dot(face.normal, camera.ez) <= 1e-9 {
                continue;
            }
            pieces.push(Piece {
                points: face.points.clone(),
                outline: face.outline.clone(),
                normal: face.normal,
                color: face.color,
                alpha: face.alpha,
                part: face.part.clone(),
                ground,
                holes: solid.ground == Ground::Holes,
            });
        }
    }
    // Every mark with its sort key and, for faces off the ground, its flat.
    let mut marks: Vec<((f64, u8), Mark)> = Vec::new();
    let mut flats: Vec<(usize, Flat)> = Vec::new();
    for piece in refine(pieces, camera) {
        let lambert = if piece.alpha >= OPAQUE {
            dot(piece.normal, light).max(0.)
        } else {
            dot(piece.normal, light).abs()
        };
        let shade = 0.32 + 0.68 * lambert;
        let flat = flat_of(&piece, camera);
        let key = if piece.ground {
            f64::NEG_INFINITY
        } else {
            camera.depth(centroid(&piece.points))
        };
        let mark = Mark::Face {
            points: flat.points.clone(),
            outline: piece.outline,
            fill: hex(piece.color.map(|c| c * shade)),
            alpha: piece.alpha,
            part: piece.part,
        };
        if !piece.ground {
            flats.push((marks.len(), flat));
        }
        marks.push(((key, u8::from(piece.holes)), mark));
    }
    // `after[i]` lists the marks painted after mark i.
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); marks.len()];
    for (i, j) in overlapping_pairs(&flats.iter().map(|(_, f)| f).collect::<Vec<_>>()) {
        match nearer(&flats[i].1, &flats[j].1, camera) {
            Some(std::cmp::Ordering::Less) => after[flats[i].0].push(flats[j].0),
            Some(_) => after[flats[j].0].push(flats[i].0),
            None => {}
        }
    }
    for ray in rays {
        let (a, b) = (camera.page(ray.from), camera.page(ray.to));
        let (da, db) = (camera.depth(ray.from), camera.depth(ray.to));
        let low = [a[0].min(b[0]), a[1].min(b[1])];
        let high = [a[0].max(b[0]), a[1].max(b[1])];
        let near: Vec<&(usize, Flat)> = flats
            .iter()
            .filter(|(_, f)| {
                f.low[0] <= high[0]
                    && f.high[0] >= low[0]
                    && f.low[1] <= high[1]
                    && f.high[1] >= low[1]
            })
            .collect();
        let mut cuts = vec![0., 1.];
        let direction = sub(ray.to, ray.from);
        for (_, f) in &near {
            for (i, &c) in f.points.iter().enumerate() {
                cuts.extend(crossing(a, b, c, f.points[(i + 1) % f.points.len()]));
            }
            let (point, normal) = f.plane;
            let along = dot(normal, direction);
            if along.abs() > 1e-12 {
                let t = dot(normal, sub(point, ray.from)) / along;
                if t > 0. && t < 1. {
                    cuts.push(t);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        for pair in cuts.windows(2) {
            let (t0, t1) = (pair[0], pair[1]);
            let tm = (t0 + t1) / 2.;
            let lerp = |t: f64| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
            let (mid, depth) = (lerp(tm), da + tm * (db - da));
            let (mut behind, mut front, mut hidden) = (Vec::new(), Vec::new(), false);
            for (index, f) in &near {
                if !f.contains(mid) {
                    continue;
                }
                let Some(face_depth) = f.depth_at(camera, mid) else {
                    continue;
                };
                if face_depth > depth + 1e-6 {
                    if f.alpha >= OPAQUE {
                        hidden = true;
                        break;
                    }
                    front.push(*index);
                } else {
                    behind.push(*index);
                }
            }
            if hidden {
                continue;
            }
            let me = marks.len();
            for i in behind {
                after[i].push(me);
            }
            after.push(front);
            marks.push((
                (depth, 2),
                Mark::Ray {
                    from: lerp(t0),
                    to: lerp(t1),
                    color: ray.color.clone(),
                    stray: ray.stray,
                },
            ));
        }
    }
    order(marks, &after)
}

/// Kahn's topological sort of `marks` under `after`, taking the farthest
/// ready mark first; a cycle is broken at its farthest remaining mark.
fn order(marks: Vec<((f64, u8), Mark)>, after: &[Vec<usize>]) -> Vec<Mark> {
    use std::{cmp::Reverse, collections::BinaryHeap};
    #[derive(PartialEq)]
    struct Key(f64, u8, usize);
    impl Eq for Key {}
    impl PartialOrd for Key {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for Key {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.0
                .total_cmp(&other.0)
                .then(self.1.cmp(&other.1))
                .then(self.2.cmp(&other.2))
        }
    }
    let n = marks.len();
    let key = |i: usize| Key(marks[i].0.0, marks[i].0.1, i);
    let mut waiting = vec![0_usize; n];
    for &j in after.iter().flatten() {
        waiting[j] += 1;
    }
    let mut ready: BinaryHeap<Reverse<Key>> = (0..n)
        .filter(|&i| waiting[i] == 0)
        .map(|i| Reverse(key(i)))
        .collect();
    let mut done = vec![false; n];
    let mut sequence = Vec::with_capacity(n);
    while sequence.len() < n {
        let next = match ready.pop() {
            Some(Reverse(Key(.., i))) if !done[i] => i,
            Some(_) => continue,
            None => (0..n)
                .filter(|&i| !done[i])
                .min_by(|&i, &j| key(i).cmp(&key(j)))
                .expect("marks remain"),
        };
        done[next] = true;
        sequence.push(next);
        for &j in &after[next] {
            waiting[j] = waiting[j].saturating_sub(1);
            if waiting[j] == 0 && !done[j] {
                ready.push(Reverse(key(j)));
            }
        }
    }
    let mut slots: Vec<Option<Mark>> = marks.into_iter().map(|(_, m)| Some(m)).collect();
    sequence
        .into_iter()
        .map(|i| slots[i].take().expect("each mark once"))
        .collect()
}

/// A round scale-bar length (1, 2 or 5 × 10ⁿ mm, at least 1 mm so every
/// label is a whole millimetre) no longer than `room` page mm when possible.
fn bar_length(scale: f64, room: f64) -> f64 {
    let target = room / scale;
    let decade = 10_f64.powf(target.log10().floor()).max(1.);
    [5., 2., 1.]
        .into_iter()
        .map(|m| m * decade)
        .find(|&l| l <= target)
        .unwrap_or(decade)
}

/// A projected, depth-ordered layout ready to write as SVG.
pub(crate) struct Layout {
    pub camera: Camera,
    pub marks: Vec<Mark>,
    /// Scale bar: bench length (mm) and its page length (mm).
    pub bar: (f64, f64),
    /// Ray colours and their meaning.
    pub legend: Vec<(String, String)>,
    pub title: String,
    pub caption: String,
    pub background: Background,
    pub stroke: bool,
}

fn number(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn path(points: &[P2], close: bool) -> String {
    let mut d = String::new();
    for (i, [x, y]) in points.iter().enumerate() {
        write!(
            d,
            "{}{},{}",
            if i == 0 { "M" } else { "L" },
            number(*x),
            number(*y)
        )
        .unwrap();
    }
    if close {
        d.push('Z');
    }
    d
}

pub(crate) fn render(
    bench: &Bench,
    system: &System,
    options: &Layout3dOptions,
    version: Option<BlueprintRevision>,
) -> Result<Layout, String> {
    let scene = bench.resolved_stages()?;
    let bench = scene.as_ref();
    bench.validate()?;
    if let Some(bad) = options.thickness_mm.iter().flatten().find(|(id, mm)| {
        !bench.elements.iter().any(|e| &e.id == *id) || !(mm.is_finite() && **mm > 0.)
    }) {
        return Err(format!(
            "layout3d thickness_mm needs an element id and a positive thickness, got {:?}: {}",
            bad.0, bad.1
        ));
    }
    for e in bench.elements.iter().filter(|e| {
        e.substrate.is_some()
            && matches!(
                e.kind,
                Kind::Aperture
                    | Kind::Splitter
                    | Kind::Pbs
                    | Kind::Waveplate
                    | Kind::Polarizer
                    | Kind::Lens
            )
    }) {
        let substrate = e.substrate.as_ref().expect("filtered");
        // A lens solid always spans its traced substrate.
        let drawn = options
            .thickness_mm
            .as_ref()
            .and_then(|m| m.get(&e.id))
            .copied()
            .filter(|_| e.kind != Kind::Lens);
        let t = drawn.unwrap_or(substrate.thickness_mm);
        let thinnest =
            t - substrate.wedge_deg.unwrap_or(0.).to_radians().tan().abs() * e.body_radius;
        if thinnest <= 0. {
            return Err(format!(
                "layout3d substrate of {} is {} mm thick, too thin for its wedge across the {} mm body radius",
                e.id,
                number(t),
                number(e.body_radius)
            ));
        }
    }
    for (id, shape) in options.lens_shape.iter().flatten() {
        let Some(lens) = bench
            .elements
            .iter()
            .find(|e| &e.id == id && e.kind == Kind::Lens)
        else {
            return Err(format!("layout3d lens_shape names no lens: {id:?}"));
        };
        let rim = lens.body_radius;
        for radius in [shape.front_radius_mm, shape.back_radius_mm]
            .into_iter()
            .flatten()
        {
            if !(radius.is_finite() && (radius == 0. || radius.abs() > rim)) {
                return Err(format!(
                    "layout3d lens_shape radii of {id} must be 0 (flat) or exceed its body radius {rim} mm, got {radius}"
                ));
            }
        }
        if shape
            .center_thickness_mm
            .is_some_and(|c| !(c.is_finite() && c > 0.))
        {
            return Err(format!(
                "layout3d lens_shape center_thickness_mm of {id} must be positive"
            ));
        }
        let (_, edge, _) = lens_profile(
            lens,
            options
                .thickness_mm
                .as_ref()
                .and_then(|m| m.get(id))
                .copied(),
            Some(shape),
        );
        if edge < 0. {
            return Err(format!(
                "layout3d lens_shape faces of {id} cross before its rim: edge thickness {} mm",
                number(edge)
            ));
        }
    }
    if options
        .rays_per_source
        .is_some_and(|n| !(1..=101).contains(&n))
    {
        return Err("layout3d rays_per_source must be 1 to 101".into());
    }
    let (azimuth, elevation, zoom, pan) = view(options)?;
    let trace = bundle_trace(bench, system, options)?;
    let solids = scene_solids(bench, &trace, options)?;
    let (rays, legend) = rays(bench, &trace, options);
    let [w, h] = bench.table_size;
    let mut camera = Camera::new(azimuth, elevation, add(bench.origin, [w / 2., h / 2., 0.]));
    // Fit everything drawn into the drawing area, then zoom and pan.
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for p in solids
        .iter()
        .flat_map(|s| s.faces.iter().flat_map(|f| f.points.iter()))
        .chain(rays.iter().flat_map(|r| [&r.from, &r.to]))
    {
        let q = camera.page(*p);
        low = [low[0].min(q[0]), low[1].min(q[1])];
        high = [high[0].max(q[0]), high[1].max(q[1])];
    }
    let size = [(high[0] - low[0]).max(1.), (high[1] - low[1]).max(1.)];
    let room = [AREA_MAX[0] - AREA_MIN[0], AREA_MAX[1] - AREA_MIN[1]];
    camera.scale = (room[0] / size[0]).min(room[1] / size[1]) * zoom;
    let middle = [(low[0] + high[0]) / 2., (low[1] + high[1]) / 2.];
    camera.offset = [
        (AREA_MIN[0] + AREA_MAX[0]) / 2. - camera.scale * middle[0] + pan[0],
        (AREA_MIN[1] + AREA_MAX[1]) / 2. - camera.scale * middle[1] + pan[1],
    ];
    let marks = paint(&solids, &rays, &camera, camera.ez[2] < 0.);
    let length = bar_length(camera.scale, 60.);
    let revision = version.map_or(String::new(), |v| {
        format!(
            " · revision {}{}",
            v.revision,
            if v.saved { "" } else { " (unsaved)" }
        )
    });
    Ok(Layout {
        camera,
        marks,
        bar: (length, length * camera.scale),
        legend,
        title: format!("{} — 3D layout{revision}", bench.name),
        caption: format!(
            "azimuth {}° · elevation {}° · {} rays per source · scale bar true for lengths parallel to the page",
            number(azimuth),
            number(elevation),
            options.rays()
        ),
        background: options.background.unwrap_or_default(),
        stroke: options.stroke.unwrap_or_default(),
    })
}

impl Layout {
    /// The A3 page: shaded scene, then title, axis triad, legend and scale bar.
    pub(crate) fn svg(&self) -> String {
        let mut svg = String::new();
        let [pw, ph] = PAGE;
        writeln!(
            svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{pw}mm" height="{ph}mm" viewBox="0 0 {pw} {ph}" font-family="Helvetica, Arial, sans-serif">"##
        )
        .unwrap();
        writeln!(
            svg,
            r##"<defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#5d83bd"/><stop offset="1" stop-color="#ffffff"/></linearGradient><clipPath id="area"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath></defs>"##,
            AREA_MIN[0], AREA_MIN[1], AREA_MAX[0] - AREA_MIN[0], AREA_MAX[1] - AREA_MIN[1]
        )
        .unwrap();
        let ground = match self.background {
            Background::Gradient => "url(#sky)",
            Background::White => "#ffffff",
        };
        writeln!(svg, r#"<rect width="{pw}" height="{ph}" fill="{ground}"/>"#).unwrap();
        svg.push_str(
            "<g clip-path=\"url(#area)\" stroke-linejoin=\"round\" stroke-linecap=\"round\">\n",
        );
        for mark in &self.marks {
            match mark {
                Mark::Face {
                    points,
                    outline,
                    fill,
                    alpha,
                    part,
                } => {
                    // Opaque faces get a hairline of their own colour so
                    // antialiased shared edges do not show seams.
                    let edge = if *alpha >= OPAQUE {
                        format!(r#" stroke="{fill}" stroke-width="0.12""#)
                    } else {
                        String::new()
                    };
                    let opacity = if *alpha < OPAQUE {
                        format!(r#" fill-opacity="{}""#, number(*alpha))
                    } else {
                        String::new()
                    };
                    writeln!(
                        svg,
                        r#"<path data-part="{}" d="{}" fill="{fill}"{opacity}{edge}/>"#,
                        xml(part),
                        path(points, true)
                    )
                    .unwrap();
                    // Outline mode strokes only the polygon outlines, not the
                    // triangle diagonals or the painter's cuts.
                    let lines: String = (0..points.len())
                        .filter(|&i| outline[i])
                        .map(|i| path(&[points[i], points[(i + 1) % points.len()]], false))
                        .collect();
                    if self.stroke && !lines.is_empty() {
                        writeln!(svg, r##"<path d="{lines}" stroke="#1d2129" stroke-width="0.08" fill="none"/>"##).unwrap();
                    }
                }
                Mark::Ray {
                    from,
                    to,
                    color,
                    stray,
                } => {
                    let dash = if *stray {
                        r#" stroke-dasharray="1.2 0.8""#
                    } else {
                        ""
                    };
                    writeln!(svg, r#"<path class="ray" d="{}" stroke="{color}" stroke-width="0.3" fill="none"{dash}/>"#, path(&[*from, *to], false)).unwrap();
                }
            }
        }
        svg.push_str("</g>\n");

        // Footer: title, axis triad, legend and scale bar.
        let base = PAGE[1] - 12.;
        writeln!(
            svg,
            r##"<text x="{}" y="{}" font-size="4" fill="#1d2129">{}</text>"##,
            AREA_MIN[0] + 34.,
            base - 22.,
            xml(&self.title)
        )
        .unwrap();
        writeln!(
            svg,
            r##"<text x="{}" y="{}" font-size="2.6" fill="#3b4250">{}</text>"##,
            AREA_MIN[0] + 34.,
            base - 17.,
            xml(&self.caption)
        )
        .unwrap();
        let triad = [AREA_MIN[0] + 14., base - 10.];
        for (axis, label, color) in [
            ([1., 0., 0.], "X", "#c62828"),
            ([0., 1., 0.], "Y", "#2e7d32"),
            ([0., 0., 1.], "Z", "#1565c0"),
        ] {
            let tip = [
                triad[0] + 10. * dot(axis, self.camera.ex),
                triad[1] - 10. * dot(axis, self.camera.ey),
            ];
            writeln!(
                svg,
                r#"<path d="{}" stroke="{color}" stroke-width="0.6" fill="none"/>"#,
                path(&[triad, tip], false)
            )
            .unwrap();
            if (tip[0] - triad[0]).hypot(tip[1] - triad[1]) > 1. {
                writeln!(svg, r#"<text x="{}" y="{}" font-size="3" fill="{color}" text-anchor="middle">{label}</text>"#, number(triad[0] + 13. * dot(axis, self.camera.ex)), number(triad[1] - 13. * dot(axis, self.camera.ey) + 1.)).unwrap();
            }
        }
        let mut x = AREA_MIN[0] + 34.;
        for (color, label) in &self.legend {
            if x > 280. {
                break;
            }
            writeln!(
                svg,
                r#"<path d="{}" stroke="{color}" stroke-width="0.8" fill="none"/>"#,
                path(&[[x, base - 6.], [x + 6., base - 6.]], false)
            )
            .unwrap();
            writeln!(
                svg,
                r##"<text x="{}" y="{}" font-size="2.6" fill="#1d2129">{}</text>"##,
                number(x + 7.5),
                number(base - 5.1),
                xml(label)
            )
            .unwrap();
            x += 12. + 1.5 * label.chars().count() as f64;
        }
        let (length, page_length) = self.bar;
        let (bx, by) = (AREA_MAX[0] - page_length, base - 8.);
        for i in 0..4 {
            let fill = if i % 2 == 0 { "#1d2129" } else { "#ffffff" };
            writeln!(svg, r##"<rect x="{}" y="{by}" width="{}" height="1.6" fill="{fill}" stroke="#1d2129" stroke-width="0.15"/>"##, number(bx + page_length * i as f64 / 4.), number(page_length / 4.)).unwrap();
        }
        // Whole-millimetre labels: the midpoint only when it is one.
        let ticks: &[f64] = if (length / 2.).fract() == 0. {
            &[0., 0.5, 1.]
        } else {
            &[0., 1.]
        };
        for &at in ticks {
            writeln!(svg, r##"<text x="{}" y="{}" font-size="2.6" fill="#1d2129" text-anchor="middle">{}</text>"##, number(bx + page_length * at), by - 1., number(length * at)).unwrap();
        }
        writeln!(svg, r##"<text x="{}" y="{}" font-size="2.6" fill="#1d2129" text-anchor="end">mm (bench)</text>"##, number(AREA_MAX[0]), by + 5.).unwrap();
        svg.push_str("</svg>\n");
        svg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optics::bench_lab::{export_blueprint_file, template};

    fn element(kind: Kind, position: V3, rotation: V3, body_radius: f64) -> Element {
        let mut e = template("periscope_routing").unwrap().elements[0].clone();
        e.id = format!("{kind:?}");
        e.kind = kind;
        e.position = position;
        e.rotation = Some(rotation);
        e.body_radius = body_radius;
        e.aperture = body_radius / 2.;
        e.focal_length = 100.;
        e.mount = None;
        e.substrate = None;
        e.action = None;
        e.lens_form = None;
        e.index = None;
        e
    }

    fn bounds(solid: &Solid) -> (V3, V3) {
        solid.faces.iter().flat_map(|f| &f.points).fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(lo, hi), p| {
                (
                    std::array::from_fn(|k| lo[k].min(p[k])),
                    std::array::from_fn(|k| hi[k].max(p[k])),
                )
            },
        )
    }

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    /// The polygons a set of triangles was fanned from: each polygon's first
    /// triangle is the one whose first edge lies on its outline.
    fn polygons<'a>(faces: impl IntoIterator<Item = &'a Face>) -> usize {
        faces.into_iter().filter(|f| f.outline[0]).count()
    }

    /// The corners of a solid's polygons (a fan of k triangles has k + 2).
    fn vertices(solid: &Solid) -> usize {
        assert!(
            solid
                .faces
                .iter()
                .all(|f| f.points.len() == 3 && f.outline.len() == 3),
            "{}",
            solid.part
        );
        solid.faces.len() + 2 * polygons(&solid.faces)
    }

    /// Area vector of a face along its stored normal.
    fn outward_area(f: &Face) -> V3 {
        let n = scale(newell(&f.points), 0.5);
        if dot(n, f.normal) < 0. {
            scale(n, -1.)
        } else {
            n
        }
    }

    #[test]
    fn a_lens_with_a_substrate_fills_the_traced_slab() {
        let at = [100., 50., 80.];
        let mut lens = element(Kind::Lens, at, [0.; 3], 12.5);
        lens.focal_length = 100.;
        lens.substrate = Some(bench::Substrate {
            material: "N-BK7".into(),
            thickness_mm: 10.,
            wedge_deg: None,
            coated_face: None,
        });
        let (lo, hi) = bounds(&element_solid(&lens, None, 1.));
        assert!(
            close(lo[0], at[0] - 10., 1e-9) && close(hi[0], at[0], 1e-9),
            "{lo:?} {hi:?}"
        );
        // A plano lens keeps its flat face on the slab's back and its vertex on the front.
        lens.lens_form = Some(LensForm::Plano);
        let (lo, hi) = bounds(&element_solid(&lens, None, 1.));
        assert!(
            close(lo[0], at[0] - 10., 1e-9) && close(hi[0], at[0], 1e-9),
            "{lo:?} {hi:?}"
        );
        // A substrate thinner than the sags keeps a positive edge, still within the slab.
        lens.lens_form = None;
        lens.substrate.as_mut().unwrap().thickness_mm = 1.;
        let solid = element_solid(&lens, None, 1.);
        let (lo, hi) = bounds(&solid);
        assert!(
            close(lo[0], at[0] - 1., 1e-9) && close(hi[0], at[0], 1e-9),
            "{lo:?} {hi:?}"
        );
        let rim: Vec<f64> = solid
            .faces
            .iter()
            .flat_map(|f| &f.points)
            .filter(|p| {
                close(
                    ((p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2)).sqrt(),
                    12.5,
                    1e-6,
                )
            })
            .map(|p| p[0] - at[0])
            .collect();
        let edge = rim.iter().cloned().fold(f64::MIN, f64::max)
            - rim.iter().cloned().fold(f64::MAX, f64::min);
        assert!(close(edge, 0.1, 1e-6), "edge {edge}");
        // A wedged plano lens tilts its flat back face onto the traced one, x = −t − y·tan(wedge).
        lens.lens_form = Some(LensForm::Plano);
        lens.substrate = Some(bench::Substrate {
            material: "N-BK7".into(),
            thickness_mm: 10.,
            wedge_deg: Some(5.),
            coated_face: None,
        });
        let solid = element_solid(&lens, None, 1.);
        let tan = 5_f64.to_radians().tan();
        for p in solid.faces.iter().flat_map(|f| &f.points) {
            assert!(p[0] - at[0] >= -10. - (p[1] - at[1]) * tan - 1e-9, "{p:?}");
        }
        let (lo, hi) = bounds(&solid);
        assert!(
            close(lo[0], at[0] - 10. - 12.5 * tan, 1e-6) && close(hi[0], at[0], 1e-9),
            "{lo:?} {hi:?}"
        );
        // Diverging and meniscus lenses stay within the slab and keep its axial
        // thickness: concave faces are drawn flat on the slab planes.
        for (focal, shape) in [
            (-100., None),
            (
                100.,
                Some(LensShape {
                    front_radius_mm: Some(40.),
                    back_radius_mm: Some(-60.),
                    center_thickness_mm: None,
                }),
            ),
        ] {
            lens.focal_length = focal;
            lens.lens_form = None;
            lens.substrate.as_mut().unwrap().wedge_deg = None;
            let solid = shaped_solid(&lens, None, shape.as_ref(), 1.);
            let (lo, hi) = bounds(&solid);
            assert!(
                close(lo[0], at[0] - 10., 1e-9) && close(hi[0], at[0], 1e-9),
                "{focal}: {lo:?} {hi:?}"
            );
            // Each face's innermost ring lies on its slab plane (flat) or within the
            // sag of the steepest, 40 mm, radius of it (convex).
            let radius = |p: &V3| (p[1] - at[1]).hypot(p[2] - at[2]);
            for (plane, side) in [(0., 1.), (-10., -1.)] {
                let face: Vec<&V3> = solid
                    .faces
                    .iter()
                    .flat_map(|f| &f.points)
                    .filter(|p| side * (p[0] - at[0] + 5.) > 0.)
                    .collect();
                let inner = face.iter().map(|p| radius(p)).fold(f64::INFINITY, f64::min);
                for p in face.iter().filter(|p| close(radius(p), inner, 1e-9)) {
                    let depth = side * (plane - (p[0] - at[0]));
                    assert!(
                        (-1e-9..=inner * inner / 60.).contains(&depth),
                        "{focal}: {p:?} {depth} below the {plane} plane at r = {inner}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_face_normal_points_out_of_its_solid() {
        let r = 20.;
        let at = [100., 50., 80.];
        let with = |kind, edit: &dyn Fn(&mut Element)| {
            let mut e = element(kind, at, [0., 0., 30.], r);
            edit(&mut e);
            e
        };
        let slm = |e: &mut Element| {
            e.action = Some(Action::Slm {
                segments: [2, 2],
                size_mm: [15.36, 8.64],
                phases_rad: vec![0.; 4],
                tilts_mrad: vec![],
            })
        };
        let substrate = |coated| bench::Substrate {
            material: "N-BK7".into(),
            thickness_mm: 5.,
            wedge_deg: Some(4.),
            coated_face: coated,
        };
        let concave = |e: &mut Element| e.focal_length = 12.5;
        let cases = [
            with(Kind::Mirror, &|_| {}),
            with(Kind::CurvedMirror, &concave),
            with(Kind::CurvedMirror, &|e| e.focal_length = -40.),
            with(Kind::Lens, &|e| e.focal_length = 60.),
            with(Kind::Lens, &|e| e.focal_length = -60.),
            with(Kind::Lens, &|e| e.lens_form = Some(LensForm::Plano)),
            with(Kind::Lens, &|e| {
                e.focal_length = 60.;
                e.substrate = Some(substrate(None));
            }),
            with(Kind::LensGroup, &|_| {}),
            with(Kind::Waveplate, &|_| {}),
            with(Kind::Polarizer, &|_| {}),
            with(Kind::Blocker, &|_| {}),
            with(Kind::Aperture, &|e| e.aperture = 8.),
            with(Kind::Aperture, &|e| {
                e.aperture = 8.;
                e.substrate = Some(substrate(None));
            }),
            with(Kind::Splitter, &|e| {
                e.substrate = Some(substrate(Some(bench::Face::Back)))
            }),
            with(Kind::Aperture, &slm),
            with(Kind::Mirror, &slm),
            with(Kind::Pbs, &|_| {}),
            with(Kind::Detector, &|_| {}),
        ];
        let mut solids: Vec<Solid> = cases
            .iter()
            .flat_map(|e| [element_solid(e, None, 1.), element_solid(e, None, -1.)])
            .collect();
        let meniscus = LensShape {
            front_radius_mm: Some(40.),
            back_radius_mm: Some(-60.),
            center_thickness_mm: Some(5.),
        };
        solids.push(shaped_solid(&cases[3], None, Some(&meniscus), 1.));
        solids.push(source_solid(&template("michelson").unwrap().sources[0]).unwrap());
        for solid in &solids {
            // Markings laid on a body (sensor, emitter, reflective SLM area,
            // PBS hypotenuse) are open faces; every other face closes a body,
            // so the outward area vectors cancel and the volume is positive.
            let decal = |f: &Face| {
                [palette::SENSOR, palette::EMITTER, palette::COATING].contains(&f.color)
                    && f.alpha >= 0.6
                    || f.color == palette::SLM && f.alpha >= OPAQUE
            };
            let closed: Vec<&Face> = solid.faces.iter().filter(|f| !decal(f)).collect();
            let total = closed.iter().fold([0.; 3], |s, f| add(s, outward_area(f)));
            let area: f64 = closed.iter().map(|f| norm(outward_area(f))).sum();
            assert!(
                norm(total) < 1e-9 * area,
                "{}: open by {total:?} of {area}",
                solid.part
            );
            let volume: f64 = closed
                .iter()
                .map(|f| dot(centroid(&f.points), outward_area(f)))
                .sum::<f64>()
                / 3.;
            assert!(volume > 0., "{}: volume {volume}", solid.part);
        }
        // Concave mirror, f = 12.5 (radius 25): its front faces all face +x.
        let mirror = element_solid(&element(Kind::CurvedMirror, at, [0.; 3], r), None, 1.);
        let mut dish = element(Kind::CurvedMirror, at, [0.; 3], r);
        concave(&mut dish);
        let dish = element_solid(&dish, None, 1.);
        let fronts: Vec<&Face> = dish
            .faces
            .iter()
            .filter(|f| f.color == palette::SILVER)
            .collect();
        assert_eq!(polygons(fronts.iter().copied()), SEGMENTS * RINGS);
        assert!(
            fronts.iter().all(|f| f.normal[0] > 0.),
            "{:?}",
            fronts.iter().map(|f| f.normal).collect::<Vec<_>>()
        );
        assert!(
            mirror
                .faces
                .iter()
                .filter(|f| f.color == palette::SILVER)
                .all(|f| f.normal[0] > 0.)
        );
        // Transmissive SLM: the inner walls of the frame face the window.
        let mut window = element(Kind::Aperture, at, [0.; 3], r);
        slm(&mut window);
        let window = element_solid(&window, None, 1.);
        let wall = |y: f64| {
            window
                .faces
                .iter()
                .find(|f| {
                    f.color == palette::BODY
                        && f.points.iter().all(|p| close(p[1] - at[1], y, 1e-9))
                })
                .map(|f| f.normal)
        };
        assert_eq!(wall(7.68), Some([0., -1., 0.]));
        assert_eq!(wall(-7.68), Some([0., 1., 0.]));
    }

    #[test]
    fn outlines_skip_triangle_diagonals_and_cuts() {
        // A box's quads become two triangles each; only the quads' own
        // edges are outline, four per quad.
        let mut solid = Solid::new("box", [0.; 3]);
        solid.cuboid(
            &Frame::along([0.; 3], [1., 0., 0.]).unwrap(),
            [-1.; 3],
            [1.; 3],
            palette::BODY,
            OPAQUE,
        );
        assert_eq!(solid.faces.len(), 12);
        assert_eq!(
            solid
                .faces
                .iter()
                .map(|f| f.outline.iter().filter(|&&o| o).count())
                .sum::<usize>(),
            24
        );
        // Cutting a square through its middle keeps three outline edges on
        // each side and leaves the cut unstroked, also when the cut runs
        // through two corners.
        let square = [[0., 0., 0.], [2., 0., 0.], [2., 2., 0.], [0., 2., 0.]];
        for (plane, sides) in [
            (([1., 0., 0.], [1., 0., 0.]), 3),
            (([0., 0., 0.], unit([1., -1., 0.]).unwrap()), 2),
        ] {
            let parts = split_face(&square, &[true; 4], plane);
            assert_eq!(parts.len(), 2);
            for (points, outline) in &parts {
                assert_eq!(
                    outline.iter().filter(|&&o| o).count(),
                    sides,
                    "{points:?} {outline:?}"
                );
                for (i, &o) in outline.iter().enumerate() {
                    let (p, q) = (points[i], points[(i + 1) % points.len()]);
                    let on_cut = [p, q]
                        .iter()
                        .all(|&x| dot(sub(x, plane.0), plane.1).abs() < 1e-9);
                    assert_eq!(o, !on_cut, "{points:?} edge {i}");
                }
            }
        }
        // Outline mode strokes those edges only.
        let bench = template("michelson").unwrap();
        let options = Layout3dOptions {
            stroke: Some(true),
            ..Default::default()
        };
        let svg = render(&bench, &System::default(), &options, None)
            .unwrap()
            .svg();
        assert!(svg.contains(r##"stroke="#1d2129" stroke-width="0.08" fill="none""##));
    }

    #[test]
    fn each_kind_gets_its_parametric_solid() {
        let r = 12.7;
        let at = [100., 50., 80.];
        let (n, rings) = (SEGMENTS, RINGS);
        // Face and vertex counts follow from the construction: a flat disk is
        // two n-gons and n side quads (n + 2 faces, 6n vertices); a ring adds
        // a bore (4n quads); a curved face is a fan of n triangles plus
        // (RINGS − 1)·n quads; a box is six quads.
        let check = |solid: &Solid, faces: usize, corners: usize, lo: V3, hi: V3| {
            assert_eq!(
                (polygons(&solid.faces), vertices(solid)),
                (faces, corners),
                "{}",
                solid.part
            );
            let (a, b) = bounds(solid);
            for k in 0..3 {
                assert!(
                    close(a[k], at[k] + lo[k], 1e-9) && close(b[k], at[k] + hi[k], 1e-9),
                    "{} axis {k}: {a:?} {b:?}",
                    solid.part
                );
            }
        };
        let curved = 3 * n + 4 * n * (rings - 1);
        // Plane mirror: a 1" disk 6.35 mm thick behind its face.
        let mirror = element_solid(&element(Kind::Mirror, at, [0.; 3], r), None, 1.);
        check(&mirror, n + 2, 6 * n, [-6.35, -r, -r], [0., r, r]);
        assert!(mirror.faces.iter().all(|f| f.alpha == OPAQUE));
        // Reversed side and an override: the body moves in front of the plane.
        check(
            &element_solid(&element(Kind::Mirror, at, [0.; 3], r), Some(10.), -1.),
            n + 2,
            6 * n,
            [0., -r, -r],
            [10., r, r],
        );
        // Concave mirror of focal length 100 (radius 200): the rim rises by its sag.
        let sag200 = 200. - (200_f64 * 200. - r * r).sqrt();
        check(
            &element_solid(&element(Kind::CurvedMirror, at, [0.; 3], r), None, 1.),
            (rings + 1) * n + 1,
            curved + 4 * n + n,
            [-6.35, -r, -r],
            [sag200, r, r],
        );
        // Symmetric N-BK7 lens, f = 100: R = 2(n − 1)f on both faces, 2 mm edge.
        let radius = 2. * (GLASS_INDEX - 1.) * 100.;
        let s = radius - (radius * radius - r * r).sqrt();
        let lens = element_solid(&element(Kind::Lens, at, [0.; 3], r), None, 1.);
        check(
            &lens,
            (2 * rings + 1) * n,
            2 * curved + 4 * n,
            [-1. - s, -r, -r],
            [1. + s, r, r],
        );
        assert!(lens.faces.iter().all(|f| f.alpha < OPAQUE));
        // Plano-convex: R = (n − 1)f on the front only; the flat back face is
        // RINGS − 1 flat bands and a centre n-gon.
        let mut plano = element(Kind::Lens, at, [0.; 3], r);
        plano.lens_form = Some(LensForm::Plano);
        let radius = (GLASS_INDEX - 1.) * 100.;
        let s = radius - (radius * radius - r * r).sqrt();
        let plano_corners = curved + 4 * n + 4 * n * (rings - 1) + n;
        check(
            &element_solid(&plano, None, 1.),
            2 * rings * n + 1,
            plano_corners,
            [-1., -r, -r],
            [1. + s, r, r],
        );
        // Its flat face stays on the element's −X side whichever way the light runs.
        check(
            &element_solid(&plano, None, -1.),
            2 * rings * n + 1,
            plano_corners,
            [-1., -r, -r],
            [1. + s, r, r],
        );
        // Turned round (yaw 180°), the flat face is on world +X.
        let mut turned = plano.clone();
        turned.rotation = Some([0., 0., 180.]);
        check(
            &element_solid(&turned, None, 1.),
            2 * rings * n + 1,
            plano_corners,
            [-1. - s, -r, -r],
            [1., r, r],
        );
        // Explicit surfaces: R1 = 50 convex, flat back, 6 mm on the axis.
        let shape = LensShape {
            front_radius_mm: Some(50.),
            back_radius_mm: Some(0.),
            center_thickness_mm: Some(6.),
        };
        let s50 = 50. - (50_f64 * 50. - r * r).sqrt();
        let edge = 6. - s50;
        check(
            &shaped_solid(&plano, None, Some(&shape), 1.),
            2 * rings * n + 1,
            plano_corners,
            [-edge / 2., -r, -r],
            [edge / 2. + s50, r, r],
        );
        // A diverging lens thickens its edge so that 1 mm remains at the centre.
        let mut negative = element(Kind::Lens, at, [0.; 3], r);
        negative.focal_length = -100.;
        let (lo, hi) = bounds(&element_solid(&negative, None, 1.));
        let radius = 2. * (GLASS_INDEX - 1.) * 100.;
        let s = radius - (radius * radius - r * r).sqrt();
        assert!(close(hi[0] - lo[0], 1. + 2. * s, 1e-9), "{lo:?} {hi:?}");
        // Cube beam splitter: diagonal coating plane spans the body diameter.
        let h = r / 2_f64.sqrt();
        let pbs = element_solid(&element(Kind::Pbs, at, [0.; 3], r), None, 1.);
        check(&pbs, 7, 28, [-r, -r, -h], [r, r, h]);
        assert_eq!(
            polygons(pbs.faces.iter().filter(|f| f.color == palette::COATING)),
            1
        );
        // Camera: body box behind the face with the sensor on it.
        let camera = element_solid(&element(Kind::Detector, at, [0.; 3], r), None, 1.);
        check(
            &camera,
            7,
            28,
            [-r, -0.9 * r, -0.9 * r],
            [0.05, 0.9 * r, 0.9 * r],
        );
        assert_eq!(
            polygons(camera.faces.iter().filter(|f| f.color == palette::SENSOR)),
            1
        );
        // SLM: body box sized from its active area, with the active face marked.
        let mut slm = element(Kind::Mirror, at, [0.; 3], r);
        slm.action = Some(Action::Slm {
            segments: [2, 2],
            size_mm: [15.36, 8.64],
            phases_rad: vec![0.; 4],
            tilts_mrad: vec![],
        });
        let slm = element_solid(&slm, None, 1.);
        check(&slm, 7, 28, [-15., -11.68, -8.32], [0.05, 11.68, 8.32]);
        assert_eq!(
            polygons(slm.faces.iter().filter(|f| f.color == palette::SLM)),
            1
        );
        // Transmissive SLM: a frame of four bars around a translucent active panel.
        let mut window = element(Kind::Aperture, at, [0.; 3], r);
        window.action = Some(Action::Slm {
            segments: [2, 2],
            size_mm: [15.36, 8.64],
            phases_rad: vec![0.; 4],
            tilts_mrad: vec![],
        });
        let window = element_solid(&window, None, 1.);
        check(
            &window,
            5 * 6,
            5 * 24,
            [-7.5, -11.68, -8.32],
            [7.5, 11.68, 8.32],
        );
        let panel: Vec<&Face> = window
            .faces
            .iter()
            .filter(|f| f.color == palette::SLM)
            .collect();
        assert_eq!(polygons(panel.iter().copied()), 6);
        assert!(
            panel.iter().all(|f| f.alpha < OPAQUE
                && f.points.iter().all(|p| (p[1] - at[1]).abs() <= 7.68 + 1e-9))
        );
        assert_eq!(
            polygons(window.faces.iter().filter(|f| f.color == palette::BODY)),
            24
        );
        // Thin plates are centred slabs: translucent waveplate and polarizer, opaque blocker.
        let plate = element_solid(&element(Kind::Waveplate, at, [0.; 3], r), None, 1.);
        check(&plate, n + 2, 6 * n, [-0.5, -r, -r], [0.5, r, r]);
        assert!(plate.faces.iter().all(|f| f.alpha < OPAQUE));
        let polarizer = element_solid(&element(Kind::Polarizer, at, [0.; 3], r), None, 1.);
        check(&polarizer, n + 2, 6 * n, [-1., -r, -r], [1., r, r]);
        assert!(
            polarizer
                .faces
                .iter()
                .all(|f| f.alpha < OPAQUE && f.color == palette::POLARIZER)
        );
        let blocker = element_solid(&element(Kind::Blocker, at, [0.; 3], r), None, -1.);
        check(&blocker, n + 2, 6 * n, [-1., -r, -r], [1., r, r]);
        assert!(blocker.faces.iter().all(|f| f.alpha == OPAQUE));
        // A substrate is the traced slab from the element plane back to −t,
        // whichever way the light runs; a wedge tilts its back face.
        let substrate = |thickness_mm, wedge_deg| bench::Substrate {
            material: "N-BK7".into(),
            thickness_mm,
            wedge_deg,
            coated_face: None,
        };
        let mut splitter = element(Kind::Splitter, at, [0.; 3], r);
        splitter.substrate = Some(substrate(5., None));
        check(
            &element_solid(&splitter, None, -1.),
            n + 2,
            6 * n,
            [-5., -r, -r],
            [0., r, r],
        );
        splitter.substrate = Some(bench::Substrate {
            coated_face: Some(bench::Face::Back),
            ..substrate(5., Some(10.))
        });
        let wedged = element_solid(&splitter, None, 1.);
        let lean = 10_f64.to_radians().tan() * r;
        check(&wedged, n + 2, 6 * n, [-5. - lean, -r, -r], [0., r, r]);
        let coating: Vec<&Face> = wedged
            .faces
            .iter()
            .filter(|f| f.color == palette::COATING)
            .collect();
        assert_eq!(polygons(coating.iter().copied()), 1);
        let back = [
            -(10_f64.to_radians().cos()),
            -(10_f64.to_radians().sin()),
            0.,
        ];
        assert!(
            close(dot(coating[0].normal, back), 1., 1e-9),
            "{:?}",
            coating[0].normal
        );
        // A plate PBS is drawn as its traced slab, coated on the named face.
        for (face, x, normal) in [
            (bench::Face::Front, 0., [1., 0., 0.]),
            (bench::Face::Back, -5., [-1., 0., 0.]),
        ] {
            let mut plate = element(Kind::Pbs, at, [0.; 3], r);
            plate.substrate = Some(bench::Substrate {
                coated_face: Some(face),
                ..substrate(5., None)
            });
            let plate = element_solid(&plate, None, 1.);
            check(&plate, n + 2, 6 * n, [-5., -r, -r], [0., r, r]);
            let coating: Vec<&Face> = plate
                .faces
                .iter()
                .filter(|f| f.color == palette::COATING)
                .collect();
            assert_eq!(polygons(coating.iter().copied()), 1);
            assert!(
                coating
                    .iter()
                    .all(|f| close(dot(f.normal, normal), 1., 1e-9)
                        && f.points.iter().all(|p| close(p[0] - at[0], x, 1e-9))),
                "{face:?}"
            );
        }
        // A thin substrate keeps its traced thickness.
        splitter.substrate = Some(substrate(0.1, None));
        check(
            &element_solid(&splitter, None, 1.),
            n + 2,
            6 * n,
            [-0.1, -r, -r],
            [0., r, r],
        );
        // A compensator: glass across the opening in an opaque holder ring.
        let mut compensator = element(Kind::Aperture, at, [0.; 3], r);
        compensator.substrate = Some(substrate(5., None));
        let compensator = element_solid(&compensator, None, -1.);
        check(
            &compensator,
            (n + 2) + 4 * n,
            6 * n + 16 * n,
            [-5., -r, -r],
            [0., r, r],
        );
        let glass = compensator
            .faces
            .iter()
            .filter(|f| f.color == palette::GLASS);
        assert!(polygons(glass.clone()) == n + 2 && glass.clone().all(|f| f.alpha < OPAQUE));
        let (lo, hi) = glass.flat_map(|f| &f.points).fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(lo, hi), p| {
                (
                    std::array::from_fn(|k| lo[k].min(p[k])),
                    std::array::from_fn(|k| hi[k].max(p[k])),
                )
            },
        );
        assert!(
            close(hi[1] - lo[1], r, 1e-9) && close(hi[2] - lo[2], r, 1e-9),
            "glass spans the clear aperture: {lo:?} {hi:?}"
        );
        // Iris: an annulus around the clear aperture with its bore facing the axis.
        let iris = element_solid(&element(Kind::Aperture, at, [0.; 3], r), None, 1.);
        check(&iris, 4 * n, 16 * n, [-1., -r, -r], [1., r, r]);
        let bore = |f: &&Face| {
            f.points
                .iter()
                .all(|p| close((p[1] - at[1]).hypot(p[2] - at[2]), r / 2., 1e-9))
        };
        assert!(
            iris.faces
                .iter()
                .filter(bore)
                .all(|f| dot(f.normal, sub(at, centroid(&f.points))) > 0.)
        );
        // Lens group: a barrel.
        check(
            &element_solid(&element(Kind::LensGroup, at, [0.; 3], r), None, 1.),
            n + 2,
            6 * n,
            [-r, -r, -r],
            [r, r, r],
        );
        // Source: a 60 mm laser head behind the beam origin with the emitter on its face.
        let mut source = template("michelson").unwrap().sources[0].clone();
        (source.position, source.direction, source.beam_radius) = (at, [1., 0., 0.], 1.);
        let laser = source_solid(&source).unwrap();
        check(&laser, 7, 24 + 16, [-60., -12., -12.], [0.05, 12., 12.]);
        assert_eq!(
            polygons(laser.faces.iter().filter(|f| f.color == palette::EMITTER)),
            1
        );
    }

    #[test]
    fn mirror_faces_follow_the_solved_pose_toward_the_light() {
        let bench = template("periscope_routing").unwrap();
        let system = System::default();
        let trace = bundle_trace(&bench, &system, &Layout3dOptions::default()).unwrap();
        for e in bench.elements.iter().filter(|e| e.kind == Kind::Mirror) {
            let normal = bench::rotate([1., 0., 0.], e.rotation());
            assert_eq!(lit_side(e, &trace), 1., "{}", e.id);
            let solid = element_solid(e, None, lit_side(e, &trace));
            let silver: Vec<&Face> = solid
                .faces
                .iter()
                .filter(|f| f.color == palette::SILVER)
                .collect();
            assert_eq!(polygons(silver.iter().copied()), 1);
            assert!(
                silver.iter().all(|f| dot(f.normal, normal) > 1. - 1e-9),
                "{}",
                e.id
            );
        }
        // Turning M1 round keeps its silvered face toward the incoming beam.
        let mut turned = bench.clone();
        let m1 = turned.elements.iter_mut().find(|e| e.id == "M1").unwrap();
        let [roll, pitch, yaw] = m1.rotation();
        let normal = bench::rotate([1., 0., 0.], [roll, pitch, yaw]);
        m1.rotation = Some(crate::optics::bench_lab::normal_rotation(scale(
            normal, -1.,
        )));
        let m1 = turned.elements.iter().find(|e| e.id == "M1").unwrap();
        let reversed = bench::rotate([1., 0., 0.], m1.rotation());
        assert!(dot(reversed, normal) < -1. + 1e-9);
        let trace = bundle_trace(&turned, &system, &Layout3dOptions::default()).unwrap();
        assert_eq!(lit_side(m1, &trace), -1.);
        let solid = element_solid(m1, None, -1.);
        assert!(
            solid
                .faces
                .iter()
                .filter(|f| f.color == palette::SILVER)
                .all(|f| dot(f.normal, normal) > 1. - 1e-9)
        );
    }

    fn horizontal_rays(layout: &Layout) -> Vec<f64> {
        let mut ys: Vec<f64> = layout
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Ray { from, to, .. }
                    if (from[1] - to[1]).abs() < 1e-9 && (from[0] - to[0]).abs() > 1. =>
                {
                    Some(from[1])
                }
                _ => None,
            })
            .collect();
        ys.sort_by(f64::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        ys
    }

    #[test]
    fn periscope_layers_rise_on_the_page_by_their_height_difference() {
        let bench = template("periscope_routing").unwrap();
        let options = Layout3dOptions {
            preset: Some(Preset::Side),
            rays_per_source: Some(1),
            ..Default::default()
        };
        let layout = render(&bench, &System::default(), &options, None).unwrap();
        let ys = horizontal_rays(&layout);
        assert_eq!(ys.len(), 2, "{ys:?}");
        let rise = 125. - bench.sources[0].position[2];
        assert!(rise > 10.);
        assert!(
            close(ys[1] - ys[0], rise * layout.camera.scale, 1e-6),
            "{ys:?} vs {}",
            rise * layout.camera.scale
        );
        // The default iso view shows the rise too.
        let iso = render(
            &bench,
            &System::default(),
            &Layout3dOptions::default(),
            None,
        )
        .unwrap();
        assert!(iso.camera.ey[2] > 0.8 && iso.camera.ey[2] < 0.82);
    }

    #[test]
    fn scale_bar_matches_projected_spacing() {
        let bench = template("periscope_routing").unwrap();
        for zoom in [1., 2.5, 50.] {
            let options = Layout3dOptions {
                view: Some(View3d {
                    azimuth_deg: 0.,
                    elevation_deg: 90.,
                    zoom,
                    pan_mm: [3., -4.],
                }),
                ..Default::default()
            };
            let layout = render(&bench, &System::default(), &options, None).unwrap();
            let at = |id: &str| {
                layout
                    .camera
                    .page(bench.elements.iter().find(|e| e.id == id).unwrap().position)
            };
            let (a, b) = (at("M2"), at("D"));
            let per_mm = (a[0] - b[0]).hypot(a[1] - b[1]) / 250.;
            let (length, page) = layout.bar;
            assert!(
                [1., 2., 5.]
                    .iter()
                    .any(|m| close((length / m).log10().fract().abs(), 0., 1e-9)
                        || close((length / m).log10().fract().abs(), 1., 1e-9)),
                "{length}"
            );
            assert!(
                ((page / length) / per_mm - 1.).abs() < 0.01,
                "{page} / {length} vs {per_mm}"
            );
            assert!(page > 10. && page < 70., "{page}");
            let svg = layout.svg();
            assert!(
                svg.contains(&format!(">{}</text>", number(length))),
                "{svg}"
            );
            // Every scale label is a whole number of millimetres.
            let bar = &svg[svg.rfind("<rect").unwrap()..];
            for label in bar.split(r#"text-anchor="middle">"#).skip(1) {
                let value: f64 = label[..label.find('<').unwrap()].parse().unwrap();
                assert_eq!(value.fract(), 0., "zoom {zoom}: {value}");
            }
        }
    }

    #[test]
    fn rays_hide_behind_opaque_parts_and_show_through_translucent_ones() {
        // Side view from −y: a part at y = 100 is in front of the beam at y = 200.
        let camera = Camera {
            scale: 1.,
            ..Camera::new(0., 0., [0.; 3])
        };
        assert_eq!(camera.ez, [0., -1., 0.]);
        let ray = Ray {
            from: [0., 200., 0.],
            to: [500., 200., 0.],
            color: "#ff0000".into(),
            stray: false,
        };
        let toward_y = [0., 0., 90.];
        let pieces = |marks: &[Mark]| -> Vec<(usize, P2, P2)> {
            marks
                .iter()
                .enumerate()
                .filter_map(|(i, m)| match m {
                    Mark::Ray { from, to, .. } => Some((i, *from, *to)),
                    _ => None,
                })
                .collect()
        };
        let covered = |pieces: &[(usize, P2, P2)]| {
            pieces
                .iter()
                .map(|(_, a, b)| (b[0] - a[0]).abs())
                .sum::<f64>()
        };
        let faces = |marks: &[Mark]| {
            marks
                .iter()
                .enumerate()
                .filter(|(_, m)| matches!(m, Mark::Face { .. }))
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };

        let front = element_solid(
            &element(Kind::Blocker, [250., 100., 0.], toward_y, 20.),
            None,
            1.,
        );
        let marks = paint(&[front], std::slice::from_ref(&ray), &camera, false);
        let shown = pieces(&marks);
        assert!(close(covered(&shown), 460., 1e-6), "{shown:?}");
        assert!(
            shown
                .iter()
                .all(|(_, a, b)| (a[0] + b[0]) / 2. < 230. || (a[0] + b[0]) / 2. > 270.)
        );

        let glass = element_solid(
            &element(Kind::Waveplate, [250., 100., 0.], toward_y, 20.),
            None,
            1.,
        );
        let marks = paint(&[glass], std::slice::from_ref(&ray), &camera, false);
        let shown = pieces(&marks);
        assert!(close(covered(&shown), 500., 1e-6));
        let first_face = faces(&marks)[0];
        assert!(
            shown
                .iter()
                .filter(|(_, a, b)| ((a[0] + b[0]) / 2. - 250.).abs() < 20.)
                .all(|(i, ..)| *i < first_face)
        );

        let behind = element_solid(
            &element(Kind::Blocker, [250., 300., 0.], toward_y, 20.),
            None,
            1.,
        );
        let marks = paint(&[behind], std::slice::from_ref(&ray), &camera, false);
        let shown = pieces(&marks);
        assert!(close(covered(&shown), 500., 1e-6));
        let last_face = *faces(&marks).last().unwrap();
        assert!(shown.iter().all(|(i, ..)| *i > last_face));
    }

    fn named(kind: Kind, id: &str, position: V3, rotation: V3, radius: f64) -> Element {
        Element {
            id: id.into(),
            ..element(kind, position, rotation, radius)
        }
    }

    /// Paint positions of the faces of part `id`.
    fn face_slots(marks: &[Mark], id: &str) -> Vec<usize> {
        marks
            .iter()
            .enumerate()
            .filter(|(_, m)| matches!(m, Mark::Face { part, .. } if part == id))
            .map(|(i, _)| i)
            .collect()
    }

    fn ray_slots(marks: &[Mark]) -> Vec<usize> {
        marks
            .iter()
            .enumerate()
            .filter(|(_, m)| matches!(m, Mark::Ray { .. }))
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn a_part_in_front_paints_over_the_part_behind() {
        // Element ids that name the internal table and hole solids are still parts.
        for front in ["A", "table", "holes"] {
            part_in_front_paints_over_the_part_behind(front);
        }
    }

    fn part_in_front_paints_over_the_part_behind(front: &str) {
        // Side view from −y; A at y = 100 hides B at y = 300 and the beam between.
        let camera = Camera {
            scale: 1.,
            ..Camera::new(0., 0., [0.; 3])
        };
        let toward_y = [0., 0., 90.];
        let ray = Ray {
            from: [0., 200., 0.],
            to: [500., 200., 0.],
            color: "#ff0000".into(),
            stray: false,
        };
        // B is listed first and is larger, so its rim shows round A.
        let solids = [
            element_solid(
                &named(Kind::Blocker, "B", [250., 300., 0.], toward_y, 30.),
                None,
                1.,
            ),
            element_solid(
                &named(Kind::Blocker, front, [250., 100., 0.], toward_y, 20.),
                None,
                1.,
            ),
        ];
        let marks = paint(&solids, std::slice::from_ref(&ray), &camera, false);
        let (a, b) = (face_slots(&marks, front), face_slots(&marks, "B"));
        assert!(!a.is_empty() && !b.is_empty());
        assert!(b.iter().max() < a.iter().min(), "B {b:?} before A {a:?}");
        // The beam shows only outside B's outline (|x − 250| > 30) and is
        // painted over B where it crosses in front of it.
        let mut shown = 0.;
        for (i, m) in marks.iter().enumerate() {
            if let Mark::Ray { from, to, .. } = m {
                let mid = (from[0] + to[0]) / 2.;
                assert!((mid - 250.).abs() > 20., "hidden behind A: {from:?} {to:?}");
                if (mid - 250.).abs() < 30. {
                    assert!(b.iter().all(|&j| j < i));
                }
                shown += (to[0] - from[0]).abs();
            }
        }
        assert!(close(shown, 460., 1e-6), "{front}: {shown}");
    }

    #[test]
    fn tilted_plates_order_by_depth_where_they_overlap() {
        // Two R40 plates in the planes y = x + 100 (A) and y = x + 110 (B),
        // seen from −y: along any page column A is the nearer (depth −y), yet
        // B's centre (80, 190) is nearer than A's (100, 200). A vertical beam
        // at x = 90, y = 195 runs between them.
        let camera = Camera {
            scale: 1.,
            ..Camera::new(0., 0., [0.; 3])
        };
        let turn = [0., 0., 135.];
        let solids = [
            element_solid(
                &named(Kind::Waveplate, "A", [100., 200., 0.], turn, 40.),
                None,
                1.,
            ),
            element_solid(
                &named(Kind::Waveplate, "B", [80., 190., 0.], turn, 40.),
                None,
                1.,
            ),
        ];
        let ray = Ray {
            from: [90., 195., -30.],
            to: [90., 195., 30.],
            color: "#ff0000".into(),
            stray: false,
        };
        let marks = paint(&solids, std::slice::from_ref(&ray), &camera, false);
        // The triangles of the large front and back discs of each plate,
        // which share the shade of the largest triangle (the two discs are
        // antiparallel and translucent faces shade by |n·l|).
        let discs = |id: &str| -> Vec<usize> {
            let slots = face_slots(&marks, id);
            let area = |i: usize| match &marks[i] {
                Mark::Face { points, .. } => (0..points.len())
                    .map(|k| {
                        let (p, q) = (points[k], points[(k + 1) % points.len()]);
                        p[0] * q[1] - q[0] * p[1]
                    })
                    .sum::<f64>()
                    .abs(),
                _ => 0.,
            };
            let fill_of = |i: usize| match &marks[i] {
                Mark::Face { fill, .. } => fill.clone(),
                _ => String::new(),
            };
            let largest = slots
                .iter()
                .copied()
                .max_by(|&i, &j| area(i).total_cmp(&area(j)))
                .unwrap();
            slots
                .into_iter()
                .filter(|&i| fill_of(i) == fill_of(largest))
                .collect()
        };
        let (a, b, rays) = (discs("A"), discs("B"), ray_slots(&marks));
        assert!(
            a.len() >= 2 * (SEGMENTS - 2) && b.len() >= 2 * (SEGMENTS - 2),
            "{} {}",
            a.len(),
            b.len()
        );
        assert!(!rays.is_empty());
        // Wherever a ray piece crosses a disc triangle on the page, B's
        // triangle comes before the ray and A's after it.
        let covers = |i: usize, p: P2| match &marks[i] {
            Mark::Face { points, .. } => {
                let side = |k: usize| {
                    let (a, b) = (points[k], points[(k + 1) % points.len()]);
                    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
                };
                let sides: Vec<f64> = (0..points.len()).map(side).collect();
                sides.iter().all(|&d| d > 1e-9) || sides.iter().all(|&d| d < -1e-9)
            }
            _ => false,
        };
        let mut crossings = [0; 2];
        for &r in &rays {
            let Mark::Ray { from, to, .. } = &marks[r] else {
                unreachable!()
            };
            let mid = [(from[0] + to[0]) / 2., (from[1] + to[1]) / 2.];
            for &i in a.iter().filter(|&&i| covers(i, mid)) {
                assert!(r < i, "ray {r} after A {i}");
                crossings[0] += 1;
            }
            for &i in b.iter().filter(|&&i| covers(i, mid)) {
                assert!(i < r, "ray {r} before B {i}");
                crossings[1] += 1;
            }
        }
        assert!(crossings[0] > 0 && crossings[1] > 0, "{crossings:?}");
    }

    #[test]
    fn cyclic_overlaps_still_paint_every_mark_once() {
        let face = |i: usize| {
            (
                (i as f64, 0),
                Mark::Face {
                    points: vec![],
                    outline: vec![],
                    fill: String::new(),
                    alpha: 1.,
                    part: format!("{i}"),
                },
            )
        };
        // 0 → 1 → 2 → 0 is a cycle; 3 waits on 2.
        let after = vec![vec![1], vec![2], vec![0, 3], vec![]];
        let marks = order((0..4).map(face).collect(), &after);
        let parts: Vec<String> = marks
            .iter()
            .map(|m| match m {
                Mark::Face { part, .. } => part.clone(),
                Mark::Ray { .. } => unreachable!(),
            })
            .collect();
        // The cycle breaks at its farthest mark, 0; the rest follow the edges.
        assert_eq!(parts, ["0", "1", "2", "3"]);
        // Interpenetrating plates: a cross in the top view seen from the side.
        let camera = Camera {
            scale: 1.,
            ..Camera::new(0., 0., [0.; 3])
        };
        let solids = [
            element_solid(
                &named(Kind::Blocker, "X", [0., 0., 0.], [0., 0., 45.], 20.),
                None,
                1.,
            ),
            element_solid(
                &named(Kind::Blocker, "Y", [0., 0., 0.], [0., 0., -45.], 20.),
                None,
                1.,
            ),
        ];
        let marks = paint(&solids, &[], &camera, false);
        // Every visible face is drawn, the crossing faces in pieces.
        let drawn = solids
            .iter()
            .flat_map(|s| &s.faces)
            .filter(|f| dot(f.normal, camera.ez) > 1e-9)
            .count();
        assert!(
            marks.len() > drawn,
            "{} pieces of {drawn} faces",
            marks.len()
        );
        // On either side of the crossing a different plate is in front, and
        // the topmost painted face there is the nearest surface.
        let mut fronts = std::collections::BTreeSet::new();
        for x in [-9., -5., -2., 2., 5., 9.] {
            for z in [-12., 0., 7.] {
                let at = camera.page([x, 0., z]);
                let nearest = solids
                    .iter()
                    .flat_map(|s| &s.faces)
                    .filter(|f| dot(f.normal, camera.ez) > 1e-9)
                    .filter_map(|f| {
                        let flat = Flat {
                            points: f.points.iter().map(|&p| camera.page(p)).collect(),
                            low: [f64::NEG_INFINITY; 2],
                            high: [f64::INFINITY; 2],
                            plane: (f.points[0], f.normal),
                            alpha: 1.,
                        };
                        flat.contains(at)
                            .then(|| (flat.depth_at(&camera, at).unwrap(), f.part.clone()))
                    })
                    .max_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, part)| part);
                let top = marks.iter().rev().find_map(|m| match m {
                    Mark::Face { points, part, .. } => {
                        let flat = Flat {
                            points: points.clone(),
                            low: [f64::NEG_INFINITY; 2],
                            high: [f64::INFINITY; 2],
                            plane: ([0.; 3], [0.; 3]),
                            alpha: 1.,
                        };
                        flat.contains(at).then(|| part.clone())
                    }
                    Mark::Ray { .. } => None,
                });
                assert_eq!(top, nearest, "at x {x}, z {z}");
                fronts.extend(top);
            }
        }
        assert_eq!(fronts.len(), 2, "{fronts:?}");
    }

    #[test]
    fn colours_bundles_and_presets() {
        assert_eq!(
            [1, 6, 7, 18, 19, 37, 101].map(rings_for),
            [0, 1, 1, 2, 2, 3, 6]
        );
        for rays in 1..=101 {
            let rings = rings_for(rays);
            assert_eq!(
                (0..1 + 3 * rings * (rings + 1))
                    .filter(|&i| drawn_sample(i, rays))
                    .count(),
                rays,
                "{rays}"
            );
        }
        assert_eq!(sample_index("L1", "L1:12/M1:out"), Some(12));
        assert_eq!(sample_index("L1", "L10:3"), None);
        let green = wavelength_rgb(532.);
        assert!(green[1] > 0.9 && green[0] < green[1] && green[2] == 0.);
        let blue = wavelength_rgb(450.);
        assert!(blue[2] > 0.9 && blue[0] == 0.);
        let red = wavelength_rgb(650.);
        assert!(red[0] > 0.9 && red[1] == 0.);
        assert_eq!(hex(wavelength_rgb(1064.)), hex([0.75, 0.08, 0.35]));
        assert_eq!(hex(wavelength_rgb(355.)), hex([0.45, 0.10, 0.75]));

        let bench = template("michelson").unwrap();
        let system = System::default();
        let by = |color_by| {
            let options = Layout3dOptions {
                color_by: Some(color_by),
                ..Default::default()
            };
            render(&bench, &system, &options, None).unwrap().legend
        };
        let nm = bench.sources[0].wavelength;
        assert!(
            by(ColorBy::Wavelength)
                .iter()
                .any(|(c, l)| *l == format!("{nm} nm") && *c == hex(wavelength_rgb(nm)))
        );
        assert!(
            by(ColorBy::Field)
                .iter()
                .any(|(c, l)| *l == bench.sources[0].id && c == drawing::PALETTE[0])
        );
        // Named branches: each gets its plan colour in the legend and on its rays.
        let named_bench = template("mach_zehnder").unwrap();
        assert!(named_bench.branches.len() >= 2);
        let options = Layout3dOptions {
            color_by: Some(ColorBy::Branch),
            ..Default::default()
        };
        let layout = render(&named_bench, &system, &options, None).unwrap();
        let colours: std::collections::BTreeSet<String> = named_bench
            .branches
            .iter()
            .map(|b| drawing::branch_style(&named_bench, &b.id).0.to_string())
            .collect();
        assert_eq!(
            colours.len(),
            named_bench.branches.len(),
            "distinct branch colours"
        );
        for b in &named_bench.branches {
            let colour = drawing::branch_style(&named_bench, &b.id).0;
            assert!(
                layout
                    .legend
                    .iter()
                    .any(|(c, l)| *l == b.id && *c == colour),
                "{:?}",
                layout.legend
            );
            assert!(
                layout.marks.iter().any(
                    |m| matches!(m, Mark::Ray { color, stray: false, .. } if *color == colour)
                ),
                "no {} ray drawn in {colour}",
                b.id
            );
        }
        // Bundles: 19 rays per source trace more pieces than the chief ray alone.
        let count = |rays| {
            let options = Layout3dOptions {
                rays_per_source: Some(rays),
                ..Default::default()
            };
            render(&bench, &system, &options, None)
                .unwrap()
                .marks
                .iter()
                .filter(|m| matches!(m, Mark::Ray { .. }))
                .count()
        };
        assert!(count(19) > 5 * count(1));
        // Exactly the requested rays leave each source.
        for wanted in [1, 6, 8, 101] {
            let options = Layout3dOptions {
                rays_per_source: Some(wanted),
                ..Default::default()
            };
            let trace = bundle_trace(&bench, &system, &options).unwrap();
            let (rays, _) = rays(&bench, &trace, &options);
            for source in &bench.sources {
                let d = unit(source.direction).unwrap();
                let launched = rays.iter().filter(|r| {
                    let offset = sub(r.from, source.position);
                    close(dot(offset, d), 0., 1e-9) && norm(offset) <= source.beam_radius + 1e-9
                });
                assert_eq!(launched.count(), wanted, "{} with {wanted}", source.id);
            }
        }

        let views = [
            (Preset::Iso, -45., 35.264),
            (Preset::Top, 0., 90.),
            (Preset::Side, 0., 0.),
        ];
        for (preset, a, p) in views {
            let (azimuth, elevation, ..) = view(&Layout3dOptions {
                preset: Some(preset),
                ..Default::default()
            })
            .unwrap();
            assert!(
                close(azimuth, a, 1e-3) && close(elevation, p, 1e-3),
                "{preset:?}"
            );
        }
        // The camera follows the GUI 3D projection: page up = sin a sin p x + cos a sin p y + cos p z.
        let camera = Camera::new(-25., 35., [0.; 3]);
        let (sa, ca, sp, cp) = (
            (-25_f64).to_radians().sin(),
            (-25_f64).to_radians().cos(),
            35_f64.to_radians().sin(),
            35_f64.to_radians().cos(),
        );
        assert_eq!(camera.ex, [ca, -sa, 0.]);
        assert_eq!(camera.ey, [sa * sp, ca * sp, cp]);
        assert!(
            close(dot(camera.ex, camera.ey), 0., 1e-12)
                && close(
                    dot(bench::cross(camera.ex, camera.ey), camera.ez),
                    1.,
                    1e-12
                )
        );
    }

    #[test]
    fn layout3d_exports_svg_pdf_and_png_and_refuses_bad_options() {
        let bench = template("periscope_routing").unwrap();
        let system = System::default();
        let svg = export_blueprint_file(&bench, &system, "layout3d", None, None, None).unwrap();
        let json = serde_json::to_value(&svg).unwrap();
        assert_eq!(json["filename"], "hissabeth-layout3d.svg");
        assert_eq!(json["mime"], "image/svg+xml");
        let content = json["content"].as_str().unwrap();
        assert!(content.starts_with("<svg") && content.contains(r#"width="420mm" height="297mm""#));
        assert!(
            content.contains("url(#sky)")
                && content.contains(r#"class="ray""#)
                && content.contains(r#"data-part="M1""#)
        );

        let options = Layout3dOptions {
            background: Some(Background::White),
            preset: Some(Preset::Top),
            ..Default::default()
        };
        let png = serde_json::to_value(
            export_blueprint_file(&bench, &system, "png", Some(1.), Some(&options), None).unwrap(),
        )
        .unwrap();
        assert_eq!(png["filename"], "hissabeth-layout3d.png");
        let pdf = serde_json::to_value(
            export_blueprint_file(&bench, &system, "pdf", None, Some(&options), None).unwrap(),
        )
        .unwrap();
        assert_eq!(pdf["filename"], "hissabeth-layout3d.pdf");
        assert!(pdf["content"].as_str().unwrap().starts_with("JVBERi0"));
        let plan = serde_json::to_value(
            export_blueprint_file(&bench, &system, "png", Some(1.), None, None).unwrap(),
        )
        .unwrap();
        assert_eq!(plan["filename"], "hissabeth-bench.png");
        assert_ne!(plan["content"], png["content"]);

        let refused = export_blueprint_file(&bench, &system, "dxf", None, Some(&options), None)
            .err()
            .unwrap();
        assert!(refused.contains("layout3d options apply only"), "{refused}");
        let both = Layout3dOptions {
            preset: Some(Preset::Top),
            view: Some(View3d {
                azimuth_deg: 0.,
                elevation_deg: 0.,
                zoom: 1.,
                pan_mm: [0.; 2],
            }),
            ..Default::default()
        };
        assert!(
            render(&bench, &system, &both, None)
                .err()
                .unwrap()
                .contains("either view or preset")
        );
        // A wedge whose back face would cross the front inside the rim is refused.
        let mut wedged = template("mach_zehnder").unwrap();
        let plate = wedged
            .elements
            .iter_mut()
            .find(|e| e.kind == Kind::Splitter)
            .expect("a splitter");
        plate.substrate = Some(bench::Substrate {
            material: "N-BK7".into(),
            thickness_mm: 1.,
            wedge_deg: Some(10.),
            coated_face: Some(bench::Face::Front),
        });
        let plate_id = plate.id.clone();
        let refusal = render(&wedged, &system, &Layout3dOptions::default(), None)
            .err()
            .unwrap();
        assert!(refusal.contains("too thin for its wedge"), "{refusal}");
        let thicker = Layout3dOptions {
            thickness_mm: Some([(plate_id.clone(), 8.)].into()),
            ..Default::default()
        };
        render(&wedged, &system, &thicker, None).unwrap();
        // A plate PBS on the same wedged substrate is held to the same limit.
        wedged
            .elements
            .iter_mut()
            .find(|e| e.id == plate_id)
            .unwrap()
            .kind = Kind::Pbs;
        assert!(
            render(&wedged, &system, &Layout3dOptions::default(), None)
                .err()
                .unwrap()
                .contains("too thin for its wedge")
        );
        render(&wedged, &system, &thicker, None).unwrap();
        let thick = Layout3dOptions {
            thickness_mm: Some([("nope".to_owned(), 3.)].into()),
            ..Default::default()
        };
        assert!(
            render(&bench, &system, &thick, None)
                .err()
                .unwrap()
                .contains("thickness_mm")
        );
        let bad: Result<LensShape, _> = serde_json::from_value(serde_json::json!({"r1":50}));
        assert!(bad.is_err());
        // Lens shapes need a lens, radii beyond its rim and faces apart at the rim.
        let mut lensed = bench.clone();
        let mut lens = element(Kind::Lens, [300., 200., 125.], [0.; 3], 12.7);
        lens.id = "L".into();
        lensed.elements.push(lens);
        let shaped = |id: &str, shape: LensShape| Layout3dOptions {
            lens_shape: Some([(id.to_owned(), shape)].into()),
            ..Default::default()
        };
        let refusal = |options| {
            render(&lensed, &system, &options, None)
                .err()
                .unwrap_or_default()
        };
        assert!(refusal(shaped("M1", LensShape::default())).contains("names no lens"));
        assert!(
            refusal(shaped(
                "L",
                LensShape {
                    front_radius_mm: Some(10.),
                    ..Default::default()
                }
            ))
            .contains("exceed its body radius")
        );
        assert!(
            refusal(shaped(
                "L",
                LensShape {
                    front_radius_mm: Some(15.),
                    back_radius_mm: Some(15.),
                    center_thickness_mm: Some(2.)
                }
            ))
            .contains("cross before its rim")
        );
        assert!(
            render(
                &lensed,
                &system,
                &shaped(
                    "L",
                    LensShape {
                        front_radius_mm: Some(50.),
                        back_radius_mm: Some(-80.),
                        center_thickness_mm: Some(4.)
                    }
                ),
                None
            )
            .is_ok()
        );
        // A thin traced substrate draws its flattened faces with or without an empty shape.
        let mut thin = lensed.clone();
        let l = thin.elements.iter_mut().find(|e| e.id == "L").unwrap();
        l.focal_length = 100.;
        l.substrate = Some(bench::Substrate {
            material: "N-BK7".into(),
            thickness_mm: 1.,
            wedge_deg: None,
            coated_face: None,
        });
        render(&thin, &system, &Layout3dOptions::default(), None).unwrap();
        render(&thin, &system, &shaped("L", LensShape::default()), None).unwrap();
        // A wedge the thin substrate cannot span is refused, as for plates.
        thin.elements
            .iter_mut()
            .find(|e| e.id == "L")
            .unwrap()
            .substrate
            .as_mut()
            .unwrap()
            .wedge_deg = Some(5.);
        let refusal = render(&thin, &system, &Layout3dOptions::default(), None)
            .err()
            .unwrap();
        assert!(refusal.contains("too thin for its wedge"), "{refusal}");
        // Options ask for the 3-D layout when given, or with layout3d true.
        assert!(Layout3dOptions::default().requested().is_none());
        assert!(
            Layout3dOptions {
                layout3d: Some(true),
                ..Default::default()
            }
            .requested()
            .is_some()
        );
        assert!(
            Layout3dOptions {
                stroke: Some(false),
                ..Default::default()
            }
            .requested()
            .is_some()
        );
        assert!(
            Layout3dOptions {
                layout3d: Some(false),
                preset: Some(Preset::Top),
                ..Default::default()
            }
            .requested()
            .is_none()
        );
    }

    #[test]
    fn writes_samples_for_inspection() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/layout3d-samples");
        std::fs::create_dir_all(&dir).unwrap();
        let system = System::default();
        for name in ["michelson", "periscope_routing"] {
            let mut bench = template(name).unwrap();
            if name == "periscope_routing" {
                bench.periscopes.push(bench::Periscope {
                    id: "P1".into(),
                    lower: "M1".into(),
                    upper: "M2".into(),
                });
            }
            for (tag, options) in [
                (
                    "iso",
                    Layout3dOptions {
                        rays_per_source: Some(19),
                        ..Default::default()
                    },
                ),
                (
                    "hardware",
                    Layout3dOptions {
                        show: Some(Show3d {
                            table: true,
                            holes: true,
                            mounts: true,
                        }),
                        stroke: Some(true),
                        background: Some(Background::White),
                        ..Default::default()
                    },
                ),
            ] {
                let svg = render(&bench, &system, &options, None).unwrap().svg();
                std::fs::write(dir.join(format!("{name}-{tag}.svg")), svg).unwrap();
            }
        }
    }
}
