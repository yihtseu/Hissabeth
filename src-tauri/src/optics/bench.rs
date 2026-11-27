//! Free-space geometric bench. World XY is the table; Z is height (mm).
//! Elements use Rz(yaw) Ry(pitch) Rx(roll), with local +X as surface normal.
use super::math::*;
use super::{Field, Interaction, System, coating, trace::Launch};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
};

#[path = "bench_paths.rs"]
mod paths;
pub use paths::{
    Assertion, AssertionKind, AssertionUnit, Branch, BranchReport, DispersionQuantity, GlassPass, Port, Relay,
    RelayCheck, RelayCheckKind,
};
pub(super) use paths::BranchPath;
pub(crate) use paths::PathBreak;
pub(super) use paths::NominalPaths;
use paths::{Departure, Flight, PathIndex};

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bench {
    pub name: String,
    #[schemars(inner(range(min = 1e-09, max = 1e6)))]
    pub table_size: [f64; 2],
    #[schemars(range(min = 1e-09, max = 1e6))]
    pub hole_pitch: f64,
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    pub origin: V3,
    #[schemars(length(min = 0, max = 128))]
    pub elements: Vec<Element>,
    #[schemars(length(min = 1, max = 16))]
    pub sources: Vec<Source>,
    #[schemars(length(min = 0, max = 384))]
    pub connections: Vec<Connection>,
    /// Rigid translation stages, evaluated before optical or mechanical analysis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 128))]
    pub stages: Vec<Stage>,
    /// Named nominal routes through the connection graph.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 128))]
    pub branches: Vec<Branch>,
    /// Saved comparison targets and tolerances.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 128))]
    pub assertions: Vec<Assertion>,
    /// Named beam heights. An element on a layer is held at its height.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 16))]
    pub layers: Vec<HeightLayer>,
    /// Mirror pairs carrying the beam between heights through a vertical leg.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 32))]
    pub periscopes: Vec<Periscope>,
    /// Table areas grouping elements, in table-frame XY (mm, same frame as positions).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 32))]
    pub zones: Vec<Zone>,
    /// Folded sections from an output port to an input port, measured along the trace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 64))]
    pub folds: Vec<Branch>,
    /// Paraxial ABCD relays checked along branches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 32))]
    pub relays: Vec<Relay>,
    #[schemars(range(min = 1, max = 128))]
    pub max_interactions: usize,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub min_power: f64,
}
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightLayer {
    pub id: String,
    /// Beam height above the table origin plane, in table-frame Z (mm).
    #[schemars(range(min = 1e-9, max = 1e6))]
    pub height_mm: f64,
}
/// The lower mirror turns the beam vertical; the upper mirror, directly above it,
/// turns it back to horizontal toward its reflected connection.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Periscope {
    pub id: String,
    pub lower: String,
    pub upper: String,
}
/// Give exactly one of `polygon_mm` and `rect`.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// Simple polygon vertices [x, y] in table-frame mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 3, max = 64))]
    pub polygon_mm: Option<Vec<[f64; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<ZoneRect>,
    /// Elements that must lie inside. When empty, the elements centred inside.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 128))]
    pub members: Vec<String>,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneRect {
    /// Corner [x, y] with the smaller coordinates (mm).
    pub min: [f64; 2],
    /// Corner [x, y] with the larger coordinates (mm).
    pub max: [f64; 2],
}
impl Zone {
    /// Outline vertices in table-frame XY.
    pub(crate) fn outline(&self) -> Vec<[f64; 2]> {
        match (&self.polygon_mm, self.rect) {
            (Some(points), _) => points.clone(),
            (None, Some(ZoneRect { min, max })) => {
                vec![min, [max[0], min[1]], max, [min[0], max[1]]]
            }
            (None, None) => vec![],
        }
    }
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub id: String,
    #[schemars(length(min = 1, max = 128))]
    pub members: Vec<String>,
    /// Zero-displacement positions (mm) and fixed rotations [roll, pitch, yaw] (degrees).
    pub reference_pose: BTreeMap<String, StagePose>,
    /// Unit translation vector along the table's X, Y and height axes.
    pub axis: V3,
    /// Inclusive coarse limits in mm.
    pub travel: [f64; 2],
    pub position_mm: f64,
    pub pzt: Option<StagePzt>,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagePose {
    pub position: V3,
    pub rotation: V3,
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagePzt {
    /// Inclusive fine adjustment limits in µm.
    pub range_um: [f64; 2],
    pub value_um: f64,
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    pub part: String,
    #[schemars(inner(range(min = 1e-09, max = 1e5)))]
    pub base_size: [f64; 2],
    #[schemars(inner(range(min = 0.0, max = 1e6)))]
    pub height_range: [f64; 2],
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(from = "ElementInput")]
pub struct Element {
    pub id: String,
    pub label: String,
    pub kind: Kind,
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    pub position: V3,
    /// Angles in degrees [roll, pitch, yaw], or null for an unresolved rotation.
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    pub rotation: Option<V3>,
    /// Allow auto-aim to adjust a numeric rotation. Stage rotations are always fixed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aim_rotation: bool,
    #[schemars(range(min = 1e-09, max = 1e5))]
    pub aperture: f64,
    #[schemars(range(min = 1e-09, max = 1e5))]
    pub body_radius: f64,
    #[schemars(range(min = -1e6, max = 1e6))]
    pub focal_length: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub reflectance: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub transmittance: f64,
    #[schemars(required, schema_with = "schemars::SchemaGenerator::subschema_for::<Option<Mount>>")]
    pub mount: Option<Mount>,
    /// Splitter substrate index; replaces scalar R/T with Fresnel splitting.
    #[serde(default)]
    #[schemars(range(min = 1.0, max = 4.0))]
    pub index: Option<f64>,
    /// Thin-film stack on a mirror or splitter face, replacing scalar R/T.
    #[serde(default)]
    pub coating: Option<Coating>,
    /// Surface scatter model on mirrors, splitters and thin lenses.
    #[serde(default)]
    pub scatter: Option<Scatter>,
    /// Lens group: internal Fresnel ghost bounces to enumerate (0-2).
    #[serde(default)]
    #[schemars(range(min = 0, max = 2))]
    pub ghosts: usize,
    /// Detector irradiance map resolution [columns, rows].
    #[serde(default)]
    #[schemars(inner(range(min = 2, max = 128)))]
    pub pixels: Option<[usize; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0, max = 10.0))]
    pub retardance_waves: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = -360.0, max = 360.0))]
    pub fast_axis_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1.0, max = 1e9))]
    pub extinction_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 10.0, max = 500.0))]
    pub screw_tpi: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 2.0, max = 200.0))]
    pub lever_arm_mm: Option<f64>,
    /// Spatial regions on an aperture plate or mirror face. When present, each
    /// region is its own named output port and light outside every region is blocked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 16))]
    pub regions: Vec<Region>,
    /// Glass slab behind the element plane, refracting every transmitted (and
    /// internally reflected) output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substrate: Option<Substrate>,
    /// Scalar phase added to the element's transmitted outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<PhasePlate>,
    /// Height layer. Its height fixes the element's Z, except on periscope mirrors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// Parametric action on the transverse field leaving an aperture plate or plane mirror.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

#[derive(schemars::JsonSchema, Deserialize)]
#[serde(deny_unknown_fields)]
struct ElementInput {
    id: String,
    label: Option<String>,
    kind: Kind,
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    position: V3,
    /// Angles in degrees [roll, pitch, yaw], or null for an unresolved rotation.
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    rotation: Option<V3>,
    #[serde(default)]
    aim_rotation: bool,
    #[schemars(range(min = 1e-09, max = 1e5))]
    aperture: Option<f64>,
    #[schemars(range(min = 1e-09, max = 1e5))]
    body_radius: Option<f64>,
    #[schemars(range(min = -1e6, max = 1e6))]
    focal_length: Option<f64>,
    #[schemars(range(min = 0.0, max = 1.0))]
    reflectance: Option<f64>,
    #[schemars(range(min = 0.0, max = 1.0))]
    transmittance: Option<f64>,
    mount: Option<Mount>,
    /// Splitter substrate index; replaces scalar R/T with Fresnel splitting.
    #[serde(default)]
    #[schemars(range(min = 1.0, max = 4.0))]
    index: Option<f64>,
    /// Thin-film stack on a mirror or splitter face, replacing scalar R/T.
    #[serde(default)]
    coating: Option<Coating>,
    /// Surface scatter model on mirrors, splitters and thin lenses.
    #[serde(default)]
    scatter: Option<Scatter>,
    /// Lens group: internal Fresnel ghost bounces to enumerate (0-2).
    #[serde(default)]
    #[schemars(range(min = 0, max = 2))]
    ghosts: usize,
    /// Detector irradiance map resolution [columns, rows].
    #[serde(default)]
    #[schemars(inner(range(min = 2, max = 128)))]
    pixels: Option<[usize; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0, max = 10.0))]
    retardance_waves: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = -360.0, max = 360.0))]
    fast_axis_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1.0, max = 1e9))]
    extinction_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 10.0, max = 500.0))]
    screw_tpi: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 2.0, max = 200.0))]
    lever_arm_mm: Option<f64>,
    /// Spatial regions on an aperture plate or mirror face. When present, each
    /// region is its own named output port and light outside every region is blocked.
    #[serde(default)]
    #[schemars(length(max = 16))]
    regions: Vec<Region>,
    /// Glass slab behind the element plane, refracting every transmitted (and
    /// internally reflected) output.
    #[serde(default)]
    substrate: Option<Substrate>,
    /// Scalar phase added to the element's transmitted outputs.
    #[serde(default)]
    phase: Option<PhasePlate>,
    /// Height layer. Its height fixes the element's Z, except on periscope mirrors.
    #[serde(default)]
    layer: Option<String>,
    /// Parametric action on the transverse field leaving an aperture plate or plane mirror.
    #[serde(default)]
    action: Option<Action>,
}

impl From<ElementInput> for Element {
    fn from(input: ElementInput) -> Self {
        let (focal_length, reflectance, transmittance): (f64, f64, f64) = match input.kind {
            Kind::Mirror => (0., 1., 0.),
            Kind::CurvedMirror => (100., 1., 0.),
            Kind::Splitter | Kind::Pbs => (0., 0.5, 0.5),
            Kind::Lens => (100., 0., 1.),
            Kind::LensGroup | Kind::Aperture | Kind::Waveplate | Kind::Polarizer => (0., 0., 1.),
            Kind::Detector | Kind::Blocker => (0., 0., 0.),
        };
        let aperture = input.aperture.unwrap_or(12.5_f64.min(input.body_radius.unwrap_or(15.)));
        Self {
            label: input.label.unwrap_or_else(|| input.id.clone()),
            id: input.id,
            kind: input.kind,
            position: input.position,
            rotation: input.rotation,
            aim_rotation: input.aim_rotation,
            aperture,
            body_radius: input.body_radius.unwrap_or(aperture.max(15.)),
            focal_length: input.focal_length.unwrap_or(focal_length),
            reflectance: input.reflectance.unwrap_or_else(|| reflectance.min(1. - input.transmittance.unwrap_or(0.))),
            transmittance: input.transmittance.unwrap_or_else(|| transmittance.min(1. - input.reflectance.unwrap_or(0.))),
            mount: input.mount,
            index: input.index,
            coating: input.coating,
            scatter: input.scatter,
            ghosts: input.ghosts,
            pixels: input.pixels,
            retardance_waves: input.retardance_waves,
            fast_axis_deg: input.fast_axis_deg,
            extinction_ratio: input.extinction_ratio,
            screw_tpi: input.screw_tpi,
            lever_arm_mm: input.lever_arm_mm,
            regions: input.regions,
            substrate: input.substrate,
            phase: input.phase,
            layer: input.layer,
            action: input.action,
        }
    }
}

/// Region outline in element-local in-plane coordinates `[y, z]` (mm), the
/// same frame as the clear aperture.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RegionShape {
    Circle {
        #[schemars(inner(range(min = -1e5, max = 1e5)))]
        center: [f64; 2],
        #[schemars(range(min = 1e-9, max = 1e5))]
        radius: f64,
    },
    Rect {
        #[schemars(inner(range(min = -1e5, max = 1e5)))]
        center: [f64; 2],
        /// Full width along local y and height along local z.
        #[schemars(inner(range(min = 1e-9, max = 1e5)))]
        size: [f64; 2],
    },
}
impl RegionShape {
    pub(crate) fn center(&self) -> [f64; 2] {
        match self {
            Self::Circle { center, .. } | Self::Rect { center, .. } => *center,
        }
    }
    pub(crate) fn contains(&self, y: f64, z: f64) -> bool {
        match self {
            Self::Circle { center, radius } => (y - center[0]).hypot(z - center[1]) <= *radius,
            Self::Rect { center, size } => {
                (y - center[0]).abs() <= size[0] / 2. && (z - center[1]).abs() <= size[1] / 2.
            }
        }
    }
    /// Radius of the largest circle inside the region (mm).
    pub(crate) fn inscribed_radius(&self) -> f64 {
        match self {
            Self::Circle { radius, .. } => *radius,
            Self::Rect { size, .. } => size[0].min(size[1]) / 2.,
        }
    }
    fn valid(&self) -> bool {
        match self {
            Self::Circle { center, radius } => {
                center.iter().all(|v| v.is_finite()) && radius.is_finite() && *radius > 0.
            }
            Self::Rect { center, size } => {
                center.iter().all(|v| v.is_finite())
                    && size.iter().all(|v| v.is_finite() && *v > 0.)
            }
        }
    }
}
/// One sub-aperture (aperture plate) or facet (mirror) with its own output port.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Output port name, unique on the element (for example `L`, `C`, `R`).
    pub port: String,
    pub shape: RegionShape,
    /// Power fraction passed (aperture) or reflected (mirror) by this region.
    #[serde(default = "unit_fraction")]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub transmittance: f64,
    /// Mirror facets only: facet normal in element-local coordinates. The
    /// element normal `[1, 0, 0]` is used when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    pub normal: Option<V3>,
}
fn unit_fraction() -> f64 {
    1.
}

/// Plane-parallel or wedged glass slab. The front face is the element plane
/// (local `x = 0`, outward normal `+x`); the back face passes through local
/// `[-thickness_mm, 0, 0]` and is tilted by `wedge_deg` about the local `z` axis.
/// Light enters on whichever face it reaches first.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Substrate {
    /// Material library name, for example `N-BK7` or a `nd:vd` model glass.
    pub material: String,
    #[schemars(range(min = 1e-6, max = 1e4))]
    pub thickness_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = -45.0, max = 45.0))]
    pub wedge_deg: Option<f64>,
    /// Required on splitters and PBS, rejected elsewhere: the face carrying the
    /// reflective coating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coated_face: Option<Face>,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Face {
    Front,
    Back,
}
/// Scalar phase response, exactly one of `radians` or `waves`, given at
/// `at_wavelength_nm` and scaled as 1/λ at other wavelengths.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhasePlate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = -1e6, max = 1e6))]
    pub radians: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = -1e5, max = 1e5))]
    pub waves: Option<f64>,
    #[schemars(range(min = 100.0, max = 20000.0))]
    pub at_wavelength_nm: f64,
}
impl PhasePlate {
    pub(crate) fn radians_at(&self, wavelength_nm: f64) -> f64 {
        let design = self
            .radians
            .unwrap_or_else(|| std::f64::consts::TAU * self.waves.unwrap_or(0.));
        design * self.at_wavelength_nm / wavelength_nm
    }
}
/// Parametric action on the transverse field leaving an aperture plate or plane
/// mirror, in element-local in-plane coordinates `[y, z]` (mm) about the element
/// centre. `nd` also scales the traced power; the spatial actions (`mask`, `slm`,
/// `tip_tilt`, `rgg`) act on bench_field only and leave the geometric trace unchanged.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Neutral density: power × 10^(−od), field amplitude × 10^(−od/2).
    Nd {
        #[schemars(range(min = 0.0, max = 10.0))]
        od: f64,
    },
    /// Intensity transmittance target: `inside` within `shape` and `outside`
    /// elsewhere, or an inline grey-level `matrix` (rows from local +z downward,
    /// columns along local +y, `pitch_um` cells centred on the element; `outside`
    /// beyond it).
    Mask {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shape: Option<RegionShape>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        matrix: Option<Vec<Vec<f64>>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 0.1, max = 1e5))]
        pitch_um: Option<f64>,
        #[serde(default = "unit_fraction")]
        #[schemars(range(min = 0.0, max = 1.0))]
        inside: f64,
        #[serde(default)]
        #[schemars(range(min = 0.0, max = 1.0))]
        outside: f64,
    },
    /// Segmented SLM over a `size_mm` rectangle split into `segments` [columns
    /// along +y, rows from +z downward]: each segment adds its piston
    /// (`phases_rad`, row-major) and an optional linear ramp (`tilts_mrad`,
    /// [y, z] about the segment centre). Light outside the panel passes unchanged.
    Slm {
        #[schemars(inner(range(min = 1, max = 64)))]
        segments: [usize; 2],
        #[schemars(inner(range(min = 1e-6, max = 1e5)))]
        size_mm: [f64; 2],
        phases_rad: Vec<f64>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tilts_mrad: Vec<[f64; 2]>,
    },
    /// Tip-tilt: linear phase k·(θy·y + θz·z), deflecting the field by the
    /// small angles [θy, θz] (mrad) about the element plane.
    TipTilt {
        #[schemars(inner(range(min = -100.0, max = 100.0)))]
        tilt_mrad: [f64; 2],
    },
    /// Rotating ground glass: a Gaussian-correlated random phase screen with
    /// `correlation_um` correlation length and `roughness_waves` RMS phase. Each
    /// realization (exposure step) draws an independent screen from `seed`; arms
    /// split after the glass share each realization.
    Rgg {
        #[schemars(range(min = 0.1, max = 1e5))]
        correlation_um: f64,
        #[schemars(range(min = 0.0, max = 100.0))]
        roughness_waves: f64,
        seed: u32,
    },
}
impl Action {
    pub(crate) fn valid(&self) -> bool {
        let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
        match self {
            Self::Nd { od } => (0. ..=10.).contains(od),
            Self::Mask { shape, matrix, pitch_um, inside, outside } => {
                (0. ..=1.).contains(inside)
                    && (0. ..=1.).contains(outside)
                    && match (shape, matrix) {
                        (Some(shape), None) => shape.valid() && pitch_um.is_none(),
                        (None, Some(rows)) => {
                            (1..=64).contains(&rows.len())
                                && rows.iter().all(|r| r.len() == rows[0].len() && (1..=64).contains(&r.len()))
                                && rows.iter().flatten().all(|v| (0. ..=1.).contains(v))
                                && pitch_um.is_some_and(|p| (0.1..=1e5).contains(&p))
                        }
                        _ => false,
                    }
            }
            Self::Slm { segments, size_mm, phases_rad, tilts_mrad } => {
                segments.iter().all(|n| (1..=64).contains(n))
                    && size_mm.iter().all(|s| (1e-6..=1e5).contains(s))
                    && phases_rad.len() == segments[0] * segments[1]
                    && finite(phases_rad)
                    && phases_rad.iter().all(|p| p.abs() <= 1e6)
                    && (tilts_mrad.is_empty() || tilts_mrad.len() == phases_rad.len())
                    && tilts_mrad.iter().flatten().all(|t| t.abs() <= 100.)
            }
            Self::TipTilt { tilt_mrad } => tilt_mrad.iter().all(|t| t.abs() <= 100.),
            Self::Rgg { correlation_um, roughness_waves, .. } => {
                (0.1..=1e5).contains(correlation_um) && (0. ..=100.).contains(roughness_waves)
            }
        }
    }
}
/// Mutual coherence of sources: sources sharing `group` (for example fibre
/// outputs of one laser) are coherent with each other; a source is always
/// coherent with itself.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coherence {
    pub group: String,
    /// Coherence length (mm) shared by every source of the group. Coherent pairs
    /// are weighted by exp(−(ΔOPL/L)²); omitted means fully coherent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1e-6, max = 1e12))]
    pub length_mm: Option<f64>,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldProfile {
    /// exp(−r²/w²) amplitude, w the 1/e² intensity radius.
    Gaussian,
    /// Uniform disk of radius w.
    Flat,
}
/// Transverse complex field sampling for bench_field: a `grid` [nx, ny] (powers
/// of two) at `pitch_um`, centred on the chief ray.
#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSampling {
    #[schemars(inner(range(min = 8, max = 256)))]
    pub grid: [usize; 2],
    #[schemars(range(min = 0.1, max = 1000.0))]
    pub pitch_um: f64,
    pub profile: FieldProfile,
    #[schemars(range(min = 0.1, max = 1e6))]
    pub waist_um: f64,
    /// Jones vector [[Ex re, im], [Ey re, im]] in the trace's horizontal/vertical
    /// basis, normalised on use. It sets the traced launch polarization, so the
    /// source must then omit `polarization`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polarization: Option<[[f64; 2]; 2]>,
}
impl FieldSampling {
    fn valid(&self) -> bool {
        self.grid.iter().all(|n| n.is_power_of_two() && (8..=256).contains(n))
            && (0.1..=1000.).contains(&self.pitch_um)
            && (0.1..=1e6).contains(&self.waist_um)
            && self.polarization.is_none_or(|j| {
                j.iter().flatten().all(|x| x.is_finite()) && j.iter().flatten().any(|x| *x != 0.)
            })
    }
}
struct SlabFace {
    point: V3,
    normal: V3,
}
fn slab_faces(e: &Element, substrate: &Substrate) -> [SlabFace; 2] {
    let wedge = substrate.wedge_deg.unwrap_or(0.).to_radians();
    [
        SlabFace {
            point: e.position,
            normal: rotate([1., 0., 0.], e.rotation()),
        },
        SlabFace {
            point: add(e.position, rotate([-substrate.thickness_mm, 0., 0.], e.rotation())),
            normal: rotate([wedge.cos(), wedge.sin(), 0.], e.rotation()),
        },
    ]
}
/// Distance along `d` from `p` to a face plane, if it lies ahead.
fn face_distance(p: V3, d: V3, face: &SlabFace) -> Option<f64> {
    let c = dot(d, face.normal);
    (c.abs() > 1e-12)
        .then(|| dot(sub(face.point, p), face.normal) / c)
        .filter(|s| *s > -1e-9)
}
/// Vector Snell refraction with relative index `eta = n_incident / n_transmitted`;
/// `None` on total internal reflection.
fn refract(d: V3, normal: V3, eta: f64) -> Option<V3> {
    let m = if dot(d, normal) > 0. { scale(normal, -1.) } else { normal };
    let cos_i = -dot(d, m);
    let k = 1. - eta * eta * (1. - cos_i * cos_i);
    (k >= 0.).then(|| add(scale(d, eta), scale(m, eta * cos_i - k.sqrt())))
}
/// Glass vertices (entry first, exit last) and exit direction of one output.
struct SlabPath {
    points: Vec<V3>,
    direction: V3,
}
/// Follows one output through a substrate entered at `entry` through face
/// `first`. Transmitted light crosses to the other face. Reflected light is
/// reflected in air when the coated face is the entry face, otherwise it is
/// reflected inside the glass and leaves through the entry face.
fn slab_path(
    faces: &[SlabFace; 2],
    first: usize,
    entry: V3,
    d: V3,
    reflected: Option<usize>,
    eta: f64,
) -> Result<SlabPath, &'static str> {
    let (enter, other) = (&faces[first], &faces[1 - first]);
    if reflected == Some(first) {
        return Ok(SlabPath { points: vec![], direction: reflect(d, enter.normal) });
    }
    let inside = refract(d, enter.normal, eta).ok_or("tir")?;
    let s = face_distance(entry, inside, other).ok_or("substrate_geometry")?;
    let at = add(entry, scale(inside, s));
    let (points, direction) = if reflected.is_some() {
        let back = reflect(inside, other.normal);
        let s = face_distance(at, back, enter).ok_or("substrate_geometry")?;
        let exit = add(at, scale(back, s));
        (vec![entry, at, exit], refract(back, enter.normal, 1. / eta).ok_or("tir")?)
    } else {
        (vec![entry, at], refract(inside, other.normal, 1. / eta).ok_or("tir")?)
    };
    Ok(SlabPath { points, direction })
}

impl Element {
    pub(crate) fn rotation(&self) -> V3 {
        self.rotation.expect("bench pose was validated")
    }
    /// World normals of the surfaces light leaving `port` can have met: the
    /// port's facet normal, the element plane and a substrate's back face.
    pub(crate) fn surface_normals(&self, port: &str) -> Vec<V3> {
        let r = self.rotation();
        let facet = self.regions.iter().find(|g| g.port == port).and_then(|g| g.normal);
        let back = self.substrate.as_ref().map(|s| slab_faces(self, s)[1].normal);
        facet
            .and_then(|n| unit(rotate(n, r)).ok())
            .into_iter()
            .chain([rotate([1., 0., 0.], r)])
            .chain(back)
            .collect()
    }
    /// Output ports: the region ports when regions are defined, else the kind's ports.
    pub(crate) fn ports(&self) -> Vec<&str> {
        if self.regions.is_empty() {
            ports(self.kind).to_vec()
        } else {
            self.regions.iter().map(|r| r.port.as_str()).collect()
        }
    }
    /// First region containing the local in-plane point `[y, z]`.
    pub(crate) fn region_at(&self, y: f64, z: f64) -> Option<&Region> {
        self.regions.iter().find(|r| r.shape.contains(y, z))
    }
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coating {
    #[schemars(length(min = 0, max = 32))]
    pub layers: Vec<Layer>,
    #[schemars(range(min = 1.0, max = 4.0))]
    pub substrate_index: f64,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    #[schemars(range(min = 1.0, max = 4.0))]
    pub index: f64,
    /// Film thickness in nm.
    #[schemars(range(min = 0.0, max = 1e5))]
    pub thickness: f64,
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "model", rename_all = "snake_case")]
#[schemars(deny_unknown_fields)]
pub enum Scatter {
    Lambertian {
        #[schemars(range(min = 0.0, max = 1.0))]
        fraction: f64,
        #[schemars(range(min = 1, max = 64))]
        rays: usize,
    },
    Abg {
        #[schemars(range(min = 0.0, max = 1000.0))]
        a: f64,
        #[schemars(extend("maximum" = 1000.0, "exclusiveMinimum" = 0.0))]
        b: f64,
        #[schemars(extend("maximum" = 10.0, "exclusiveMinimum" = 0.0))]
        g: f64,
        #[schemars(range(min = 1, max = 64))]
        rays: usize,
    },
}
#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize)]
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
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    pub position: V3,
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    pub direction: V3,
    #[schemars(range(min = 100.0, max = 20000.0))]
    pub wavelength: f64,
    #[schemars(range(min = 1e-12, max = 1e6))]
    pub power: f64,
    #[schemars(range(min = 0.0, max = 1e5))]
    pub beam_radius: f64,
    /// 1/e² intensity radius (mm) of a Gaussian profile weighting the sampled disk;
    /// omitted means a flat-top profile with equal-power samples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("exclusiveMinimum" = 0.0))]
    pub gaussian_radius: Option<f64>,
    #[schemars(range(min = 0, max = 8))]
    pub rings: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("exclusiveMinimum" = 0.0))]
    pub waist: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waist_offset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("minimum" = 1.0))]
    pub m2: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polarization: Option<Polarization>,
    /// Permit auto-aim to change the supplied launch direction.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aim_direction: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coherence: Option<Coherence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<FieldSampling>,
}
impl Source {
    /// Launch Jones vector: the field sampling's polarization, else `polarization`.
    pub(crate) fn launch_jones(&self) -> Option<Jones> {
        match self.field.as_ref().and_then(|f| f.polarization) {
            Some(j) => {
                let n = j.iter().flatten().map(|x| x * x).sum::<f64>().sqrt();
                Some(j.map(|c| c.map(|x| x / n)))
            }
            None => self.polarization.and_then(Polarization::jones),
        }
    }
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub from: String,
    /// `out`, `reflected`, `transmitted`, or a region port of the source element.
    pub port: String,
    pub to: String,
}
const GOLDEN: f64 = 2.399_963_229_728_653_5;
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
/// Output direction of a region: straight through an aperture plate, or the
/// mirror reflection off the region's facet normal.
fn region_direction(e: &Element, region: &Region, d: V3) -> V3 {
    if e.kind != Kind::Mirror {
        return d;
    }
    let local = region.normal.unwrap_or([1., 0., 0.]);
    let n = rotate(scale(local, 1. / norm(local)), e.rotation());
    reflect(d, n)
}
fn reflect(direction: V3, normal: V3) -> V3 {
    sub(direction, scale(normal, 2. * dot(direction, normal)))
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

fn check_stage_travel(stage: &str, dof: &str, value: f64, allowed: [f64; 2]) -> Result<(), String> {
    let [min, max] = allowed;
    if !min.is_finite() || !max.is_finite() || min > max {
        return Err(format!("Stage {stage:?}: invalid {dof} travel range {allowed:?}"));
    }
    if !(min..=max).contains(&value) {
        return Err(format!(
            "Stage travel exceeded: {}",
            json!({ "stage": stage, "dof": dof, "value": value, "allowed": allowed })
        ));
    }
    Ok(())
}

impl Bench {
    fn stage_poses(&self) -> Result<BTreeMap<String, StagePose>, String> {
        if self.stages.len() > 128 {
            return Err("Bench stage budget exceeded".into());
        }
        let mut ids = HashSet::new();
        let mut poses = BTreeMap::new();
        for stage in &self.stages {
            let id = &stage.id;
            if id.is_empty() || id.len() > 64 || !ids.insert(id) {
                return Err(format!("Invalid or repeated stage ID: {id:?}"));
            }
            if !valid_vec(stage.axis) || (norm(stage.axis) - 1.).abs() > 1e-9 {
                return Err(format!("Stage {id:?}: axis must be a unit vector"));
            }
            if stage.members.is_empty() || stage.members.len() > 128
                || stage.reference_pose.len() != stage.members.len()
            {
                return Err(format!("Stage {id:?}: reference_pose must match the member list"));
            }
            check_stage_travel(id, "position", stage.position_mm, stage.travel)?;
            let mut displacement = stage.position_mm;
            if let Some(pzt) = &stage.pzt {
                check_stage_travel(id, "pzt", pzt.value_um, pzt.range_um)?;
                displacement += pzt.value_um / 1000.;
            }
            let offset = scale(stage.axis, displacement);
            let mut members = HashSet::new();
            for member in &stage.members {
                if !members.insert(member) {
                    return Err(format!("Stage {id:?}: repeated member {member:?}"));
                }
                if !self.elements.iter().any(|e| &e.id == member) {
                    return Err(format!("Stage {id:?}: unknown element {member:?}"));
                }
                let reference = stage.reference_pose.get(member).ok_or_else(|| {
                    format!("Stage {id:?}: missing reference pose for {member:?}")
                })?;
                let position = add(reference.position, offset);
                if !valid_vec(reference.position)
                    || !valid_vec(reference.rotation)
                    || !valid_vec(position)
                {
                    return Err(format!("Stage {id:?}: invalid pose for {member:?}"));
                }
                let pose = StagePose { position, rotation: reference.rotation };
                if poses.insert(member.clone(), pose).is_some() {
                    return Err(format!("Stage {id:?}: {member:?} belongs to more than one stage"));
                }
            }
        }
        Ok(poses)
    }

    fn apply_stage_poses(&mut self, poses: &BTreeMap<String, StagePose>) {
        for element in &mut self.elements {
            if let Some(pose) = poses.get(&element.id) {
                element.position = pose.position;
                element.rotation = Some(pose.rotation);
            }
        }
    }

    fn with_stage_poses(&self, poses: &BTreeMap<String, StagePose>) -> Cow<'_, Self> {
        if !poses.is_empty() && self.elements.iter().any(|e| {
            poses.get(&e.id).is_some_and(|p| e.position != p.position || e.rotation != Some(p.rotation))
        }) {
            let mut resolved = self.clone();
            resolved.apply_stage_poses(poses);
            Cow::Owned(resolved)
        } else {
            Cow::Borrowed(self)
        }
    }

    pub(crate) fn resolved_stages(&self) -> Result<Cow<'_, Self>, String> {
        Ok(self.with_stage_poses(&self.stage_poses()?))
    }

    pub(crate) fn resolve_stages(&mut self) -> Result<(), String> {
        let poses = self.stage_poses()?;
        if !poses.is_empty() {
            self.apply_stage_poses(&poses);
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.resolved_stages()?.validated_paths().map(|_| ())
    }

    fn validated_paths(&self) -> Result<NominalPaths<'_>, String> {
        let paths = self.draft_paths()?;
        let unresolved: Vec<_> = self.elements.iter()
            .filter(|e| e.rotation.is_none())
            .map(|e| e.id.as_str())
            .collect();
        if !unresolved.is_empty() {
            return Err(format!("Unresolved bench rotations: {}", unresolved.join(", ")));
        }
        let periscope_mirrors: HashSet<&str> = self.periscopes.iter()
            .flat_map(|p| [p.lower.as_str(), p.upper.as_str()])
            .collect();
        for e in &self.elements {
            if let Some(height) = self.layer_height(e)
                && !periscope_mirrors.contains(e.id.as_str())
                && (e.position[2] - height).abs() > 1e-9
            {
                return Err(format!(
                    "{} is on layer {} at {height} mm but sits at Z = {} mm; run auto-aim or move it to the layer height",
                    e.id,
                    e.layer.as_deref().unwrap_or_default(),
                    e.position[2]
                ));
            }
        }
        Ok(paths)
    }

    pub(crate) fn validate_draft(&self) -> Result<(), String> {
        self.draft_paths().map(|_| ())
    }

    pub(super) fn draft_paths(&self) -> Result<NominalPaths<'_>, String> {
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
                || e.rotation.is_some_and(|rotation| !valid_vec(rotation))
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
            if let Some(m) = &e.mount
                && (m.part.len() > 256
                    || m.base_size
                        .iter()
                        .any(|x| !x.is_finite() || *x <= 0. || *x > 1e5)
                    || m.height_range
                        .iter()
                        .any(|x| !x.is_finite() || *x < 0. || *x > 1e6)
                    || m.height_range[0] > m.height_range[1])
            {
                return Err(format!("Invalid mount: {}", e.id));
            }
            if !e.regions.is_empty() {
                let mut names = HashSet::new();
                let valid = matches!(e.kind, Kind::Aperture | Kind::Mirror)
                    && e.regions.len() <= 16
                    && e.coating.is_none()
                    && e.scatter.is_none()
                    && e.regions.iter().all(|r| {
                        (1..=32).contains(&r.port.len())
                            && !["in", "mount"].contains(&r.port.as_str())
                            && names.insert(r.port.as_str())
                            && r.shape.valid()
                            && (0. ..=1.).contains(&r.transmittance)
                            && r.normal.is_none_or(|n| {
                                e.kind == Kind::Mirror
                                    && valid_vec(n)
                                    && unit(n).is_ok_and(|u| u[0] > 1e-6)
                            })
                    });
                if !valid {
                    return Err(format!(
                        "Invalid regions on {}: regions need an aperture or plane mirror without coating or scatter, at most 16 unique port names, finite shapes, transmittance in 0..1, and mirror-only facet normals facing the element front",
                        e.id
                    ));
                }
            }
            if let Some(s) = &e.substrate
                && (!matches!(
                    e.kind,
                    Kind::Aperture | Kind::Splitter | Kind::Pbs | Kind::Waveplate | Kind::Polarizer
                ) || !e.regions.is_empty()
                    || s.material.is_empty()
                    || s.material.len() > 128
                    || !(s.thickness_mm.is_finite() && s.thickness_mm > 0. && s.thickness_mm <= 1e4)
                    || s.wedge_deg.is_some_and(|w| !(-45. ..=45.).contains(&w))
                    || (s.coated_face.is_some() != matches!(e.kind, Kind::Splitter | Kind::Pbs)))
            {
                return Err(format!(
                    "Invalid substrate on {}: substrates need an aperture without regions, splitter, PBS, waveplate or polarizer, a material name, thickness in 0..1e4 mm, wedge within ±45°, and a coated face (front or back) on splitters and PBS only",
                    e.id
                ));
            }
            if let Some(p) = &e.phase
                && (!matches!(
                    e.kind,
                    Kind::Aperture | Kind::Lens | Kind::Splitter | Kind::Pbs | Kind::Waveplate | Kind::Polarizer
                ) || !e.regions.is_empty()
                    || p.radians.is_some() == p.waves.is_some()
                    || p.radians.is_some_and(|r| !(-1e6..=1e6).contains(&r))
                    || p.waves.is_some_and(|w| !(-1e5..=1e5).contains(&w))
                    || !(100. ..=20000.).contains(&p.at_wavelength_nm))
            {
                return Err(format!(
                    "Invalid phase on {}: give exactly one finite radians or waves value at 100–20000 nm on a transmissive element without regions",
                    e.id
                ));
            }
            if let Some(a) = &e.action
                && (!matches!(e.kind, Kind::Aperture | Kind::Mirror) || !a.valid())
            {
                return Err(format!(
                    "Invalid action on {}: actions need an aperture or plane mirror; nd od in 0..10; a mask with either a valid shape or a 1–64 × 1–64 matrix of 0..1 values with pitch_um; an SLM with 1–64 segments per axis, positive size_mm and one finite phase (and optional tilt within ±100 mrad) per segment; tip-tilt within ±100 mrad; RGG correlation 0.1–1e5 um and roughness 0–100 waves",
                    e.id
                ));
            }
            if let Some(n) = e.index
                && (e.kind != Kind::Splitter || !(1. ..=4.).contains(&n))
            {
                return Err(format!("Splitter substrate index outside 1..4: {}", e.id));
            }
            if let Some(c) = &e.coating
                && (![Kind::Mirror, Kind::CurvedMirror, Kind::Splitter].contains(&e.kind)
                    || c.layers.len() > 32
                    || !(1. ..=4.).contains(&c.substrate_index)
                    || c.layers.iter().any(|l| {
                        !(1. ..=4.).contains(&l.index) || !(0. ..=1e5).contains(&l.thickness)
                    }))
            {
                return Err(format!("Invalid coating stack: {}", e.id));
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
            if let Some(bins) = e.pixels
                && (e.kind != Kind::Detector || bins.iter().any(|b| !(2..=128).contains(b)))
            {
                return Err(format!(
                    "Detector pixels require 2..=128 bins per axis: {}",
                    e.id
                ));
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
                || s.gaussian_radius.is_some_and(|x| !x.is_finite() || x <= 0.)
                || s.waist.is_some_and(|x| !x.is_finite() || x <= 0.)
                || s.waist_offset.is_some_and(|x| !x.is_finite())
                || s.m2.is_some_and(|x| !x.is_finite() || x < 1.)
                || matches!(s.polarization, Some(Polarization::Linear { angle_deg }) if !angle_deg.is_finite())
                || s.field.as_ref().is_some_and(|f| !f.valid() || (f.polarization.is_some() && s.polarization.is_some()))
                || s.coherence.as_ref().is_some_and(|c| {
                    !(1..=64).contains(&c.group.len()) || c.length_mm.is_some_and(|l| !(1e-6..=1e12).contains(&l))
                })
            {
                return Err(format!("Invalid source: {}", s.id));
            }
            packets += 1 + 3 * s.rings * (s.rings + 1);
        }
        for (i, a) in self.sources.iter().enumerate() {
            let Some(ca) = &a.coherence else { continue };
            if let Some(b) = self.sources[..i]
                .iter()
                .find(|b| b.coherence.as_ref().is_some_and(|cb| cb.group == ca.group && cb.length_mm != ca.length_mm))
            {
                return Err(format!(
                    "Sources {} and {} share coherence group {} with different length_mm; a group has one coherence length",
                    b.id, a.id, ca.group
                ));
            }
        }
        if packets > 1024 || self.connections.len() > 384 {
            return Err("Bench packet or connection budget exceeded".into());
        }
        let mut outputs = HashSet::new();
        for c in &self.connections {
            let allowed = if self.sources.iter().any(|s| s.id == c.from) {
                vec!["out"]
            } else {
                self.elements
                    .iter()
                    .find(|e| e.id == c.from)
                    .map(Element::ports)
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
        self.validate_layout()?;
        self.nominal_paths()
    }

    pub(crate) fn layer_height(&self, e: &Element) -> Option<f64> {
        let id = e.layer.as_deref()?;
        self.layers.iter().find(|l| l.id == id).map(|l| l.height_mm)
    }

    /// Layers, periscopes and zones: identifiers, references and shapes.
    fn validate_layout(&self) -> Result<(), String> {
        let element = |id: &str| self.elements.iter().find(|e| e.id == id);
        let mut ids = HashSet::new();
        for l in &self.layers {
            if !(1..=64).contains(&l.id.len())
                || !ids.insert(&l.id)
                || !(l.height_mm.is_finite() && l.height_mm > 0. && l.height_mm <= 1e6)
            {
                return Err(format!("Invalid or repeated layer: {}", l.id));
            }
        }
        if self.layers.len() > 16 || self.periscopes.len() > 32 || self.zones.len() > 32 {
            return Err("Bench layer, periscope or zone budget exceeded".into());
        }
        for e in &self.elements {
            if let Some(layer) = &e.layer
                && !ids.contains(layer)
            {
                return Err(format!("{} names unknown layer {layer}", e.id));
            }
        }
        let mut ids = HashSet::new();
        let mut mirrors = HashSet::new();
        for p in &self.periscopes {
            let valid = (1..=64).contains(&p.id.len())
                && ids.insert(&p.id)
                && p.lower != p.upper
                && mirrors.insert(&p.lower)
                && mirrors.insert(&p.upper)
                && [&p.lower, &p.upper].iter().all(|id| {
                    element(id).is_some_and(|e| e.kind == Kind::Mirror && e.regions.is_empty())
                })
                && self.connections.iter().any(|c| {
                    c.from == p.lower && c.port == "reflected" && c.to == p.upper
                });
            if !valid {
                return Err(format!(
                    "Invalid periscope {}: it needs a unique ID and two distinct plane mirrors without regions, used by no other periscope, with the lower mirror's reflected output connected to the upper mirror",
                    p.id
                ));
            }
        }
        let mut ids = HashSet::new();
        for z in &self.zones {
            let outline = z.outline();
            let valid = (1..=64).contains(&z.id.len())
                && ids.insert(&z.id)
                && z.label.len() <= 256
                && z.polygon_mm.is_some() != z.rect.is_some()
                && z.rect.is_none_or(|r| r.min[0] < r.max[0] && r.min[1] < r.max[1])
                && z.members.len() <= 128
                && z.members.iter().all(|m| element(m).is_some())
                && (3..=64).contains(&outline.len())
                && outline.iter().flatten().all(|v| v.is_finite() && v.abs() <= 1e6)
                && polygon_area(&outline) > 1e-9
                && simple_polygon(&outline);
            if !valid {
                return Err(format!(
                    "Invalid zone {}: give a unique ID and exactly one of polygon_mm (3–64 vertices of a simple polygon) or rect (min below max), with known member elements",
                    z.id
                ));
            }
        }
        Ok(())
    }
}

/// Unsigned area of a polygon (shoelace).
pub(crate) fn polygon_area(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.
}

/// No two non-adjacent edges touch.
fn simple_polygon(points: &[[f64; 2]]) -> bool {
    let n = points.len();
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let on = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        p[0] >= a[0].min(b[0]) && p[0] <= a[0].max(b[0]) && p[1] >= a[1].min(b[1]) && p[1] <= a[1].max(b[1])
    };
    let touch = |a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]| {
        let (d1, d2) = (cross(c, d, a), cross(c, d, b));
        let (d3, d4) = (cross(a, b, c), cross(a, b, d));
        segments_cross(a, b, c, d)
            || (d1 == 0. && on(c, d, a))
            || (d2 == 0. && on(c, d, b))
            || (d3 == 0. && on(a, b, c))
            || (d4 == 0. && on(a, b, d))
    };
    (0..n).all(|i| {
        (i + 1..n).all(|j| {
            let adjacent = j == i + 1 || (i == 0 && j == n - 1);
            adjacent
                || !touch(points[i], points[(i + 1) % n], points[j], points[(j + 1) % n])
        })
    })
}

/// Segments ab and cd cross at a point interior to both.
fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| {
        (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])
    };
    cross(c, d, a) * cross(c, d, b) < 0. && cross(a, b, c) * cross(a, b, d) < 0.
}

/// Point inside a polygon (even-odd rule); boundary points count as inside.
pub(crate) fn inside_polygon(points: &[[f64; 2]], p: [f64; 2]) -> bool {
    let n = points.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        if cross == 0.
            && p[0] >= a[0].min(b[0])
            && p[0] <= a[0].max(b[0])
            && p[1] >= a[1].min(b[1])
            && p[1] <= a[1].max(b[1])
        {
            return true;
        }
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
            inside = !inside;
        }
    }
    inside
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct Segment {
    pub source: String,
    pub branch: String,
    pub from: V3,
    pub to: V3,
    pub power: f64,
    pub element: Option<String>,
    pub status: String,
    #[serde(flatten)]
    pub flight: Option<Flight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jones: Option<[[f64; 2]; 2]>,
    #[serde(skip)]
    pub(super) axial: bool,
    #[serde(skip)]
    pub(super) beam: Vec<Port>,
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct Termination {
    pub source: String,
    pub branch: String,
    pub element: Option<String>,
    pub position: V3,
    pub power: f64,
    pub reason: String,
    /// Last real propagation segment, if the ray reached a surface.
    pub last_segment: Option<usize>,
}
#[derive(schemars::JsonSchema, Default, Debug, Serialize)]
pub struct Ledger {
    pub launched: f64,
    pub detected: f64,
    pub absorbed: f64,
    pub blocked: f64,
    pub escaped: f64,
    pub truncated: f64,
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
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
#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct Trace {
    pub segments: Vec<Segment>,
    pub branches: Vec<BranchReport>,
    /// Bench folds, measured along the trace like branches.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub folds: Vec<BranchReport>,
    /// Table-frame poses used by this trace, keyed by stage member ID.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub stage_poses: BTreeMap<String, StagePose>,
    pub terminations: Vec<Termination>,
    pub power: Ledger,
    pub detectors: Vec<DetectorMap>,
    pub balance_error: f64,
    pub warnings: Vec<String>,
    /// Physical traces only: power leaving each region port of a spatial
    /// splitter. Unlit ports are listed with zero power.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub port_power: Vec<PortPower>,
    #[serde(skip)]
    pub(super) splitter_outputs: HashMap<usize, [Option<(V3, V3)>; 2]>,
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct PortPower {
    pub element: String,
    pub port: String,
    pub power: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TraceMode {
    Physical,
    Axial,
    Nominal,
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
    let [x, y, z] = local(direction, e.rotation());
    let local_direction = unit([
        x.signum(),
        y / x.abs() - height[1] / e.focal_length,
        z / x.abs() - height[2] / e.focal_length,
    ])?;
    Ok(rotate(local_direction, e.rotation()))
}

struct Packet {
    source: String,
    branch: String,
    p: V3,
    d: V3,
    power: f64,
    depth: usize,
    expected: Option<String>,
    departure: Departure,
    air: f64,
    wavelength: f64,
    jones: Option<Jones>,
    /// Zero-power nominal ray launched from an unlit region's centre so that
    /// every region arm keeps a geometric path; it never touches the ledger.
    probe: bool,
    /// Identity of the beam: the source port followed by every element port the
    /// packet left. A beam's samples, its chief and its region probes share it,
    /// while different sources or upstream arms differ.
    beam: Vec<Port>,
}
impl Packet {
    fn segment(&self, from: V3, to: V3, element: Option<&str>, status: &str, flight: Option<Flight>) -> Segment {
        Segment {
            source: self.source.clone(),
            branch: self.branch.clone(),
            from,
            to,
            power: self.power,
            element: element.map(str::to_owned),
            status: status.into(),
            flight,
            jones: self.jones,
            axial: self.departure.nominal,
            beam: self.beam.clone(),
        }
    }

    fn finish(self, element: Option<String>, position: V3, reason: &str, last_segment: Option<usize>) -> Termination {
        Termination {
            source: self.source,
            branch: self.branch,
            element,
            position,
            power: self.power,
            reason: reason.into(),
            last_segment,
        }
    }
}

fn group_media(system: &System) -> Vec<&str> {
    let mut medium = system.object_material.as_str();
    system.surfaces.iter().map(|surface| {
        if surface.interaction == Interaction::Refract {
            medium = &surface.material;
        }
        medium
    }).collect()
}

fn surface_port(element: &str, face: usize) -> Port {
    Port::new(element, &format!("surface:{}", face + 1))
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
#[derive(Clone)]
struct GhostHit {
    point: V3,
    face: usize,
    index: f64,
    medium_after_face: Option<usize>,
}
struct GhostPath {
    points: Vec<GhostHit>,
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
    /// `(p, v)` is the ray position and direction in prescription coordinates.
    fn run(
        &mut self,
        (mut p, mut v): (V3, V3),
        mut i: i64,
        step: i64,
        mut factor: f64,
        bounces_left: usize,
        mut points: Vec<GhostHit>,
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
            let face = i as usize;
            let medium_after_face = if step > 0 { face.checked_sub(1) } else { Some(face) };
            let n1 = medium_after_face.map_or(self.entry, |face| self.after[face]);
            points.push(GhostHit { point: hit.point, face, index: n1, medium_after_face });
            if s.clear_aperture && hit.point[0].hypot(hit.point[1]) > s.aperture + 1e-9 {
                self.paths.push(GhostPath {
                    points,
                    factor,
                    bounces: self.max_bounces - bounces_left,
                    end: GhostEnd::Blocked("aperture"),
                });
                return;
            }
            let n2 = if step > 0 { self.after[face] } else { self.before(face) };
            let ci = dot(v, hit.normal).clamp(-1., 1.);
            let reflectance = coating::fresnel(n1, n2, ci).unwrap_or(1.);
            if reflectance > 0. {
                let bounced = unit(sub(v, scale(hit.normal, 2. * ci))).unwrap_or(v);
                if bounces_left > 0 {
                    self.run(
                        (hit.point, bounced),
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
    let angles = [0., 0., e.rotation()[2]];
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

    fn surface_hit(
        element: &Element,
        system: &System,
        position: V3,
        direction: V3,
        bounded: bool,
    ) -> Option<(f64, Option<usize>)> {
        let p = local(sub(position, element.position), element.rotation());
        let d = local(direction, element.rotation());
        if element.kind == Kind::LensGroup {
            let mut vertex = 0.;
            let mut nearest = None;
            for (i, surface) in system.surfaces.iter().enumerate() {
                if let Ok(hit) =
                    surface.profile().intersect([p[1], p[2], p[0]], [d[1], d[2], d[0]], vertex)
                    && (!bounded || hit.point[0].hypot(hit.point[1]) <= element.body_radius)
                    && hit.distance > 1e-7
                    && nearest.is_none_or(|(distance, _)| hit.distance < distance)
                {
                    nearest = Some((hit.distance, Some(i)));
                }
                vertex += surface.thickness;
            }
            nearest
        } else if let Some(substrate) = &element.substrate {
            // A slab is entered on the nearest face met from outside, at a point
            // that lies on the glass side of the other face and inside the body.
            // The returned index names that entry face (0 front, 1 back).
            let faces = slab_faces(element, substrate);
            // The glass lies behind the front face and in front of the back face.
            let side = [-1., 1.];
            let mut nearest: Option<(f64, Option<usize>)> = None;
            for (i, face) in faces.iter().enumerate() {
                if side[i] * dot(direction, face.normal) <= 0. {
                    continue;
                }
                let Some(distance) = face_distance(position, direction, face) else { continue };
                let point = add(position, scale(direction, distance));
                let other = &faces[1 - i];
                let hit = local(sub(point, element.position), element.rotation());
                if distance > 1e-7
                    && side[1 - i] * dot(sub(point, other.point), other.normal) >= -1e-9
                    && (!bounded || hit[1].hypot(hit[2]) <= element.body_radius)
                    && nearest.is_none_or(|(d, _)| distance < d)
                {
                    nearest = Some((distance, Some(i)));
                }
            }
            nearest
        } else if d[0].abs() > 1e-12 {
            let distance = -p[0] / d[0];
            let hit = add(p, scale(d, distance));
            (distance > 1e-7 && (!bounded || hit[1].hypot(hit[2]) <= element.body_radius))
                .then_some((distance, None))
        } else {
            None
        }
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
            let mut take = |distance: f64, mount: bool, surface: Option<usize>| {
                if distance > 1e-7 && closest.is_none_or(|a| distance < a.0) {
                    closest = Some((distance, e, mount, surface));
                }
            };
            if let Some((distance, face)) = Self::surface_hit(e, system, position, direction, true) {
                take(distance, false, face);
            }
            // A periscope's upper mirror shares the lower mirror's post: its own
            // mount box would stand in the vertical leg.
            if !self.periscopes.iter().any(|p| p.upper == e.id)
                && let Some(distance) = mount_hit(e, position, direction, self.origin[2])
            {
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
        (out, paths): (&mut Trace, &mut PathIndex),
        queue: &mut VecDeque<Packet>,
        ray: &Packet,
        e: &Element,
        arrival: usize,
    ) -> Result<(), String> {
        let hit = out.segments[arrival].to;
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
                last_segment: Some(arrival),
            });
            paths.group_stop(ray.departure.nominal, arrival, arrival, PathBreak::new(&e.id, "in", "budget"));
            return Ok(());
        }
        let entry = ray.air;
        let media = group_media(system);
        let mut after = Vec::with_capacity(group_size);
        let mut vertices = Vec::with_capacity(group_size);
        let mut vertex = 0.;
        for (s, medium) in system.surfaces.iter().zip(&media) {
            after.push(system.material_index(medium, ray.wavelength)?);
            vertices.push(vertex);
            vertex += s.thickness;
        }
        let p = local(sub(ray.p, e.position), e.rotation());
        let d = local(ray.d, e.rotation());
        let mut walk = GhostWalk {
            system,
            entry,
            after,
            vertices,
            max_bounces: e.ghosts,
            paths: vec![],
        };
        walk.run(
            ([p[1], p[2], p[0]], [d[1], d[2], d[0]]),
            0,
            1,
            1.,
            e.ghosts,
            vec![],
        );
        out.power.absorbed += ray.power * (1. - e.transmittance);
        let world = |p: V3| add(e.position, rotate([p[2], p[0], p[1]], e.rotation()));
        for path in walk.paths {
            let power = ray.power * e.transmittance * path.factor;
            if power <= 0. {
                continue;
            }
            let main = path.bounces == 0;
            let mut previous = arrival;
            let mut last = hit;
            let mut departure = Departure {
                port: surface_port(&e.id, 0),
                previous: Some(arrival),
                arrival: Some(arrival),
                nominal: ray.departure.nominal && main,
                phase: 0.,
            };
            for point in path.points.iter().skip(1) {
                let end = world(point.point);
                let to = surface_port(&e.id, point.face);
                let medium = point.medium_after_face.map_or(system.object_material.as_str(), |i| media[i]);
                let flight = departure.flight(to.clone(), norm(sub(end, last)), medium, point.index, ray.wavelength);
                previous = out.segments.len();
                let status = if main { "group_surface" } else { "group_ghost" };
                let mut segment = ray.segment(last, end, Some(&e.id), status, Some(flight));
                segment.power = power;
                out.segments.push(segment);
                departure.port = to;
                departure.previous = Some(previous);
                last = end;
            }
            let termination = |reason: String| Termination {
                source: ray.source.clone(),
                branch: ray.branch.clone(),
                element: Some(e.id.clone()),
                position: last,
                power,
                reason,
                last_segment: Some(previous),
            };
            match path.end {
                GhostEnd::Exit(direction) => {
                    let port = if main { "out" } else { "ghost" };
                    departure.port = Port::new(&e.id, port);
                    queue.push_back(Packet {
                        source: ray.source.clone(),
                        beam: next_beam(&ray.beam, &e.id, port),
                        branch: if main {
                            format!("{}/{}:out", ray.branch, e.id)
                        } else {
                            format!("{}/{}:ghost", ray.branch, e.id)
                        },
                        p: last,
                        d: rotate([direction[2], direction[0], direction[1]], e.rotation()),
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
                        departure,
                        air: ray.air,
                        wavelength: ray.wavelength,
                        jones: ray.jones,
                        probe: ray.probe,
                    });
                }
                GhostEnd::Truncated => {
                    out.power.truncated += power;
                    out.terminations
                        .push(termination("ghost_bounce_limit".into()));
                }
                GhostEnd::Blocked(reason) => {
                    out.power.blocked += power;
                    let reason = format!("group_{reason}");
                    paths.group_stop(departure.nominal, arrival, previous, PathBreak::new(&e.id, &departure.port.port, &reason));
                    out.terminations.push(termination(reason));
                }
            }
        }
        Ok(())
    }

    pub fn trace(&self, system: &System) -> Result<Trace, String> {
        let poses = self.stage_poses()?;
        let mut trace = self.with_stage_poses(&poses).trace_resolved(system)?;
        trace.stage_poses = poses;
        Ok(trace)
    }

    /// Lists every region port's power and marks branches leaving an unlit
    /// region as unreachable: their probe path stays geometric only.
    fn report_region_power(&self, out: &mut Trace, (ports, beams): &(HashMap<Port, f64>, HashMap<Vec<Port>, f64>)) {
        let region_ports: HashSet<Port> = self
            .elements
            .iter()
            .flat_map(|e| e.regions.iter().map(move |r| Port::new(&e.id, &r.port)))
            .collect();
        for e in &self.elements {
            for r in &e.regions {
                let port = Port::new(&e.id, &r.port);
                out.port_power.push(PortPower {
                    power: ports.get(&port).copied().unwrap_or(0.),
                    element: port.element,
                    port: port.port,
                });
            }
        }
        // A path is lit only where its own incoming beam (same source and upstream
        // route) put sampled power through each region port it leaves.
        for path in out.branches.iter_mut().chain(out.folds.iter_mut()).flat_map(|b| b.paths.iter_mut()) {
            if path.interruption.is_some() {
                continue;
            }
            let unlit = path.segment_indices.iter().find_map(|&at| {
                let segment = &out.segments[at];
                let flight = segment.flight.as_ref()?;
                let port = Port::new(&flight.from_element, &flight.from_port);
                (region_ports.contains(&port)
                    && beams.get(&segment.beam).is_none_or(|p| *p <= 0.))
                .then_some(port)
            });
            if let Some(port) = unlit {
                path.interruption = Some(PathBreak::new(&port.element, &port.port, "not_illuminated"));
            }
        }
    }
    pub(super) fn trace_resolved(&self, system: &System) -> Result<Trace, String> {
        self.trace_mode(system, TraceMode::Physical)
    }

    pub(super) fn trace_alignment(&self, system: &System, nominal: bool) -> Result<Trace, String> {
        self.trace_mode(system, if nominal { TraceMode::Nominal } else { TraceMode::Axial })
    }

    fn trace_mode(&self, system: &System, mode: TraceMode) -> Result<Trace, String> {
        let nominal = self.validated_paths()?;
        self.validate_groups(system)?;
        let directed = mode == TraceMode::Nominal;
        let mut paths = PathIndex::default();
        let media = group_media(system);
        let mut out = Trace {
            segments: vec![],
            branches: vec![],
            folds: vec![],
            stage_poses: BTreeMap::new(),
            terminations: vec![],
            power: Ledger::default(),
            detectors: vec![],
            balance_error: 0.,
            warnings: vec![],
            port_power: vec![],
            splitter_outputs: HashMap::new(),
        };
        // Region port power: per port for display, and per incoming beam (see
        // `Packet::beam`) to judge which arms are lit.
        let mut region_power: (HashMap<Port, f64>, HashMap<Vec<Port>, f64>) = Default::default();
        // Incoming beams (lineage, element) whose region arms already have an anchor.
        let mut anchors: HashSet<(Vec<Port>, String)> = HashSet::new();
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
            let rings = if mode == TraceMode::Physical { s.rings } else { 0 };
            for ring in 1..=rings {
                for j in 0..6 * ring {
                    let a = std::f64::consts::TAU * j as f64 / (6 * ring) as f64;
                    offsets.push(add(
                        scale(u, s.beam_radius * ring as f64 / s.rings as f64 * a.cos()),
                        scale(v, s.beam_radius * ring as f64 / s.rings as f64 * a.sin()),
                    ));
                }
            }
            // Each sample stands for an (almost) equal-area cell, so its power is the
            // profile intensity at the sample. Ring k holds 6k samples at radius kR/n;
            // the normalisation always covers the full physical grid.
            let intensity = |r2: f64| s.gaussian_radius.map_or(1., |w| (-2. * r2 / (w * w)).exp());
            let total: f64 = (0..=s.rings)
                .map(|k| {
                    let r = s.beam_radius * k as f64 / s.rings.max(1) as f64;
                    (6 * k).max(1) as f64 * intensity(r * r)
                })
                .sum();
            let air = system.material_index("AIR", s.wavelength)?;
            out.power.launched += s.power;
            for (i, offset) in offsets.into_iter().enumerate() {
                queue.push_back(Packet {
                    source: s.id.clone(),
                    branch: format!("{}:{i}", s.id),
                    beam: vec![Port::new(&s.id, "out")],
                    p: add(s.position, offset),
                    d,
                    power: s.power * intensity(dot(offset, offset)) / total,
                    depth: 0,
                    expected: next(&s.id, "out"),
                    departure: Departure {
                        port: Port::new(&s.id, "out"),
                        previous: None,
                        arrival: None,
                        nominal: i == 0 && (mode != TraceMode::Physical || !self.branches.is_empty() || !self.folds.is_empty()),
                        phase: 0.,
                    },
                    air,
                    wavelength: s.wavelength,
                    jones: s.launch_jones(),
                    probe: false,
                });
            }
        }
        while let Some(ray) = queue.pop_front() {
            if directed && ray.expected.is_none() {
                continue;
            }
            let cutoff = (!ray.probe && ray.power < self.min_power)
                || ray.depth >= self.max_interactions
                || out.segments.len() >= 50_000;
            if cutoff {
                let departure = &ray.departure;
                paths.stop(departure, PathBreak::new(&departure.port.element, &departure.port.port, "budget"));
                if ray.probe {
                    continue;
                }
                out.power.truncated += ray.power;
                let (position, last) = (ray.p, departure.previous);
                out.terminations.push(ray.finish(None, position, "budget", last));
                continue;
            }
            let closest = if directed {
                self.elements.iter().find(|e| Some(&e.id) == ray.expected.as_ref()).and_then(|e| {
                    let (distance, face) = Self::surface_hit(e, system, ray.p, ray.d, false)?;
                    Some((distance, e, false, face))
                })
            } else {
                self.nearest(system, ray.p, ray.d, None)
            };
            let Some((distance, e, mount, face)) = closest else {
                let end = add(
                    ray.p,
                    scale(ray.d, self.table_size[0].hypot(self.table_size[1])),
                );
                let status = if ray.expected.is_some() { "missed_target" } else { "escaped" };
                // The displayed extension has no measured endpoint.
                out.segments.push(ray.segment(ray.p, end, None, status, None));
                let departure = &ray.departure;
                let (element, port) = ray.expected.as_deref().map_or(
                    (departure.port.element.as_str(), departure.port.port.as_str()),
                    |id| (id, nominal.input(&departure.port)),
                );
                paths.stop(departure, PathBreak::new(element, port, status));
                if ray.probe {
                    continue;
                }
                let (expected, last) = (ray.expected.clone(), departure.previous);
                out.power.escaped += ray.power;
                out.terminations.push(ray.finish(expected, end, "escaped", last));
                continue;
            };
            let hit = add(ray.p, scale(ray.d, distance));
            let h = local(sub(hit, e.position), e.rotation());
            let reason = if mount {
                Some("mount_obstruction")
            } else if ray.expected.as_ref().is_some_and(|id| id != &e.id) {
                Some("unexpected_hit")
            } else if !directed && h[1].hypot(h[2]) > e.aperture {
                Some("clipped")
            } else if !directed && !e.regions.is_empty() && e.region_at(h[1], h[2]).is_none() {
                // Inside the plate but between its sub-apertures or facets.
                Some("masked")
            } else if e.kind == Kind::LensGroup
                && (face != Some(0) || local(ray.d, e.rotation())[0] <= 0.)
            {
                Some("group_entry")
            } else if e.kind == Kind::Blocker {
                Some("beam_stop")
            } else {
                None
            };
            let arrival = out.segments.len();
            let input = if mount {
                "mount"
            } else if ray.expected.as_deref() == Some(&e.id) {
                nominal.input(&ray.departure.port)
            } else {
                "in"
            };
            // A substrate is entered on the face the hit test stopped at.
            let slab = e.substrate.as_ref().filter(|_| !mount && reason.is_none()).map(|substrate| {
                (substrate, slab_faces(e, substrate), face.unwrap_or(0))
            });
            let entry = hit;
            let flight = ray.departure.flight(Port::new(&e.id, input), distance, "AIR", ray.air, ray.wavelength);
            out.segments.push(ray.segment(ray.p, entry, Some(&e.id), reason.unwrap_or("hit"), Some(flight)));
            // A region element still anchors its arms when the chief sample lands on the
            // plate between regions: the arms start from region probes, and their
            // illumination is judged from the sampled port power afterwards.
            // Region arms are anchored by the first sample that reaches the plate as
            // planned: the chief sample (even when it lands between regions), or, if
            // the chief never gets there, the first sample passing a region. The anchor
            // starts every arm from region probes; whether an arm is lit is judged from
            // the sampled port power afterwards.
            let mut ray = ray;
            // Anchors are kept per source and upstream port, so two sources (or the
            // arms of an upstream region plate) each start their own arms here. A
            // nominal probe from an upstream region may take the anchor; a sampled
            // ray is promoted only when no nominal packet reached the plate first.
            let planned = !e.regions.is_empty()
                && !mount
                && ray.expected.as_deref() == Some(e.id.as_str())
                && matches!(reason, None | Some("masked" | "clipped"));
            let key = (ray.beam.clone(), e.id.clone());
            if planned
                && !directed
                && !anchors.contains(&key)
                && (ray.departure.nominal || (reason.is_none() && !ray.probe))
            {
                anchors.insert(key);
                ray.departure.nominal = true;
            } else if planned && !directed {
                // Already anchored: later arrivals are ordinary samples.
                ray.departure.nominal = false;
            }
            let anchored = planned && ray.departure.nominal;
            paths.hit(&ray.departure, arrival, &e.id, reason.is_none() || anchored);
            if anchored && reason.is_some() {
                paths.anchor_regions(arrival);
            }
            if !directed && anchored {
                // Unlit or not, every region arm keeps a geometric path from its centre.
                let lit = reason.is_none().then(|| e.region_at(h[1], h[2])).flatten();
                for region in e
                    .regions
                    .iter()
                    .filter(|r| lit.is_none_or(|l| l.port != r.port))
                {
                    let [y, z] = region.shape.center();
                    queue.push_back(Packet {
                        source: ray.source.clone(),
                        branch: format!("{}/{}:{}~probe", ray.branch, e.id, region.port),
                        beam: next_beam(&ray.beam, &e.id, &region.port),
                        p: add(e.position, rotate([0., y, z], e.rotation())),
                        d: region_direction(e, region, ray.d),
                        power: 0.,
                        depth: ray.depth + 1,
                        expected: next(&e.id, &region.port),
                        departure: Departure {
                            port: Port::new(&e.id, &region.port),
                            previous: Some(arrival),
                            arrival: Some(arrival),
                            nominal: true,
                            phase: 0.,
                        },
                        air: ray.air,
                        wavelength: ray.wavelength,
                        jones: ray.jones,
                        probe: true,
                    });
                }
            }
            if let Some(reason) = reason {
                if ray.probe {
                    continue;
                }
                out.power.blocked += ray.power;
                out.terminations.push(ray.finish(Some(e.id.clone()), hit, reason, Some(arrival)));
                continue;
            }
            if e.kind == Kind::LensGroup {
                let group_size = system.surfaces.len();
                if !directed && e.ghosts > 0 {
                    self.trace_group_ghosts(system, (&mut out, &mut paths), &mut queue, &ray, e, arrival)?;
                    continue;
                }
                if ray.depth + group_size > self.max_interactions {
                    out.power.truncated += ray.power;
                    paths.group_stop(ray.departure.nominal, arrival, arrival, PathBreak::new(&e.id, input, "budget"));
                    out.terminations.push(ray.finish(Some(e.id.clone()), hit, "budget", Some(arrival)));
                    continue;
                }
                let p = local(sub(ray.p, e.position), e.rotation());
                let d = local(ray.d, e.rotation());
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
                    Err(error) => (
                        error.ray.map(|r| *r),
                        Some((error.kind.as_str(), error.point)),
                    ),
                };
                let world = |p: V3| add(e.position, rotate([p[2], p[0], p[1]], e.rotation()));
                let mut last = hit;
                let mut blocked = None;
                let mut departure = Departure {
                    port: surface_port(&e.id, 0),
                    previous: Some(arrival),
                    arrival: Some(arrival),
                    nominal: ray.departure.nominal,
                    phase: 0.,
                };
                if let Some(path) = &traced {
                    let internal = path.segments.iter().skip(1).map(|segment| (segment.point, segment.n_before));
                    let failed_hit = failure.and_then(|(_, point)| {
                        path.segments.last().zip(point).map(|(before, point)| (point, before.n_after))
                    });
                    for (gap, (point, index)) in internal.chain(failed_hit).enumerate() {
                        let end = world(point);
                        let delta = sub(end, last);
                        let length = norm(delta);
                        let mut visit = (end, length, surface_port(&e.id, gap + 1), "group_surface");
                        if length > 1e-7 {
                            let direction = scale(delta, 1. / length);
                            if dot(direction, rotate([1., 0., 0.], e.rotation())) <= 0. {
                                blocked = Some(("group_geometry", departure.port.clone()));
                                break;
                            }
                            if !directed && let Some((distance, obstacle, mount, _)) =
                                self.nearest(system, last, direction, Some(&e.id))
                                && distance < length - 1e-7
                            {
                                let port = Port::new(&obstacle.id, if mount { "mount" } else { "in" });
                                let reason = if mount { "mount_obstruction" } else { "group_obstruction" };
                                blocked = Some((reason, port.clone()));
                                visit = (add(last, scale(direction, distance)), distance, port, "group_obstruction");
                            }
                        }
                        let (end, length, port, status) = visit;
                        let flight = departure.flight(port.clone(), length, media[gap], index, ray.wavelength);
                        departure.previous = Some(out.segments.len());
                        out.segments.push(ray.segment(last, end, Some(&port.element), status, Some(flight)));
                        departure.port = port;
                        last = end;
                        if blocked.is_some() {
                            break;
                        }
                    }
                }
                let failure_reason;
                if blocked.is_none()
                    && let Some((reason, _)) = failure
                {
                    failure_reason = format!("group_{reason}");
                    blocked = Some((&failure_reason, departure.port.clone()));
                }
                if let Some((reason, port)) = blocked {
                    out.power.blocked += ray.power;
                    let previous = departure.previous.unwrap();
                    paths.group_stop(departure.nominal, arrival, previous, PathBreak::new(&port.element, &port.port, reason));
                    out.terminations.push(ray.finish(Some(port.element), last, reason, Some(previous)));
                } else if let Some(path) = traced {
                    out.power.absorbed += ray.power * (1. - e.transmittance);
                    if e.transmittance > 0. {
                        let d = path.direction;
                        departure.port = Port::new(&e.id, "out");
                        queue.push_back(Packet {
                            source: ray.source,
                            branch: format!("{}/{}:out", ray.branch, e.id),
                            beam: next_beam(&ray.beam, &e.id, "out"),
                            p: last,
                            d: rotate([d[2], d[0], d[1]], e.rotation()),
                            power: ray.power * e.transmittance,
                            depth: ray.depth + group_size,
                            expected: next(&e.id, "out"),
                            departure,
                            air: ray.air,
                            wavelength: ray.wavelength,
                            jones: ray.jones,
                            probe: ray.probe,
                        });
                    } else {
                        paths.group_stop(departure.nominal, arrival, departure.previous.unwrap(), PathBreak::new(&e.id, "out", "no_output"));
                    }
                }
                continue;
            }
            if e.kind == Kind::Detector && ray.probe {
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
                out.terminations.push(ray.finish(Some(e.id.clone()), hit, "detected", Some(arrival)));
                continue;
            }
            let n = rotate([1., 0., 0.], e.rotation());
            let reflected = reflect(ray.d, n);
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
                Kind::Aperture | Kind::Mirror if !e.regions.is_empty() => {
                    let gain = if e.kind == Kind::Mirror {
                        e.reflectance
                    } else {
                        1.
                    };
                    let lit = e.region_at(h[1], h[2]);
                    // Directed tracing ignores clear apertures, so every arm is followed.
                    for region in e
                        .regions
                        .iter()
                        .filter(|r| directed || lit.is_some_and(|l| l.port == r.port))
                    {
                        scalar(
                            region.port.as_str(),
                            region_direction(e, region, ray.d),
                            gain * region.transmittance,
                        );
                    }
                }
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
            if let Some(Action::Nd { od }) = &e.action {
                let gain = 10f64.powf(-od);
                for child in &mut children {
                    child.2 *= gain;
                }
            }
            let transmitted: f64 = children.iter().map(|c| c.2).sum();
            out.power.absorbed += ray.power * (1. - transmitted).max(0.);
            let glass = match &slab {
                Some((substrate, ..)) => Some(
                    system
                        .material_index(&substrate.material, ray.wavelength)
                        .map_err(|err| format!("Substrate of {}: {err}", e.id))?,
                ),
                None => None,
            };
            for (port, d, fraction, jones) in children {
                if fraction <= 0. {
                    continue;
                }
                let child_beam = next_beam(&ray.beam, &e.id, port);
                let child_branch = format!("{}/{}:{}", ray.branch, e.id, port);
                let mut start = (hit, d, arrival);
                if let (Some((substrate, faces, first)), Some(n_glass)) = (&slab, glass) {
                    let reflected = (port == "reflected")
                        .then(|| usize::from(substrate.coated_face == Some(Face::Back)));
                    // Every glass vertex after the entry must stay inside the clear aperture.
                    let inside = |point: &V3| {
                        let l = local(sub(*point, e.position), e.rotation());
                        l[1].hypot(l[2]) <= e.aperture
                    };
                    let path = slab_path(faces, *first, entry, ray.d, reflected, ray.air / n_glass)
                        .and_then(|path| if path.points.iter().all(inside) { Ok(path) } else { Err("clipped") });
                    match path {
                        Ok(path) => {
                            let mut previous = arrival;
                            for pair in path.points.windows(2) {
                                let departure = Departure {
                                    port: Port::new(&e.id, "substrate"),
                                    previous: Some(previous),
                                    arrival: Some(arrival),
                                    nominal: ray.departure.nominal,
                                    phase: 0.,
                                };
                                let length = norm(sub(pair[1], pair[0]));
                                let flight = departure.flight(
                                    Port::new(&e.id, "substrate"),
                                    length,
                                    &substrate.material,
                                    n_glass,
                                    ray.wavelength,
                                );
                                previous = out.segments.len();
                                out.segments.push(Segment {
                                    source: ray.source.clone(),
                                    branch: child_branch.clone(),
                                    from: pair[0],
                                    to: pair[1],
                                    power: ray.power * fraction,
                                    element: Some(e.id.clone()),
                                    status: "substrate".into(),
                                    flight: Some(flight),
                                    jones,
                                    axial: ray.departure.nominal,
                                    beam: child_beam.clone(),
                                });
                            }
                            start = (path.points.last().copied().unwrap_or(entry), path.direction, previous);
                        }
                        Err(reason) => {
                            let departure = Departure {
                                port: Port::new(&e.id, port),
                                previous: Some(arrival),
                                arrival: Some(arrival),
                                nominal: ray.departure.nominal,
                                phase: 0.,
                            };
                            paths.stop(&departure, PathBreak::new(&e.id, port, reason));
                            if !ray.probe {
                                out.power.blocked += ray.power * fraction;
                                out.terminations.push(Termination {
                                    source: ray.source.clone(),
                                    branch: child_branch,
                                    element: Some(e.id.clone()),
                                    position: entry,
                                    power: ray.power * fraction,
                                    reason: reason.into(),
                                    last_segment: Some(arrival),
                                });
                            }
                            continue;
                        }
                    }
                }
                let (hit, d, previous) = start;
                let phase = e
                    .phase
                    .as_ref()
                    .filter(|_| port != "reflected")
                    .map_or(0., |plate| plate.radians_at(ray.wavelength));
                let mut kept = fraction;
                if !directed
                    && !ray.probe
                    && let Some(model) = &e.scatter
                {
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
                                beam: next_beam(&ray.beam, &e.id, &format!("scatter{k}")),
                                p: hit,
                                d: direction,
                                power: ray.power * fraction * weight,
                                depth: ray.depth + 1,
                                expected: None,
                                departure: Departure {
                                    port: Port::new(&e.id, &format!("scatter{k}")),
                                    previous: Some(arrival),
                                    arrival: Some(arrival),
                                    nominal: false,
                                    phase: 0.,
                                },
                                air: ray.air,
                                wavelength: ray.wavelength,
                                jones,
                                probe: false,
                            });
                        }
                    }
                }
                if kept > 0. {
                    if !e.regions.is_empty() && !ray.probe {
                        *region_power.0.entry(Port::new(&e.id, port)).or_default() += ray.power * kept;
                        *region_power
                            .1
                            .entry(next_beam(&ray.beam, &e.id, port))
                            .or_default() += ray.power * kept;
                    }
                    if ray.departure.nominal && matches!(e.kind, Kind::Splitter | Kind::Pbs) {
                        // Slots store transmitted then reflected chief-ray exit points
                        // (the glass exit for a substrate) and directions.
                        let outputs = out.splitter_outputs.entry(arrival).or_default();
                        outputs[usize::from(port == "reflected")] = Some((hit, d));
                    }
                    queue.push_back(Packet {
                        source: ray.source.clone(),
                        branch: child_branch,
                        beam: child_beam,
                        p: hit,
                        d,
                        power: ray.power * kept,
                        depth: ray.depth + 1,
                        expected: next(&e.id, port),
                        departure: Departure {
                            port: Port::new(&e.id, port),
                            previous: Some(previous),
                            arrival: Some(arrival),
                            nominal: ray.departure.nominal,
                            phase,
                        },
                        air: ray.air,
                        wavelength: ray.wavelength,
                        jones,
                        probe: ray.probe,
                    });
                }
            }
        }
        out.branches = paths.reports(self, &nominal, &out.segments);
        out.folds = paths.fold_reports(self, &nominal, &out.segments);
        if mode == TraceMode::Physical {
            self.report_region_power(&mut out, &region_power);
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
        if mode == TraceMode::Physical {
            out.warnings = self.assembly_report().warnings;
        }
        Ok(out)
    }
    pub fn assembly(&self) -> Value {
        match self.resolved_stages() {
            Ok(bench) => json!(bench.assembly_report()),
            Err(error) => json!({ "error": error }),
        }
    }
    pub(crate) fn assembly_report(&self) -> Assembly {
        let mut warnings = vec![];
        let mut bom: HashMap<String, usize> = HashMap::new();
        let items:Vec<_>=self.elements.iter().map(|e| {
            let p=sub(e.position,self.origin);
            if p[2]<e.body_radius {warnings.push(format!("{}: optical body envelope intersects table",e.id));}
            let holes=[(p[0]/self.hole_pitch).round(),(p[1]/self.hole_pitch).round()];
            if e.mount.is_none(){warnings.push(format!("{}: mount dimensions unspecified",e.id));}
            if let Some(m)=&e.mount {
                if p[2]<m.height_range[0] || p[2]>m.height_range[1] {warnings.push(format!("{}: optical height outside mount adjustment range",e.id));}
                let a=e.rotation()[2].to_radians();let w=(a.cos().abs()*m.base_size[0]+a.sin().abs()*m.base_size[1])/2.;let h=(a.sin().abs()*m.base_size[0]+a.cos().abs()*m.base_size[1])/2.;
                if p[0]-w<0. || p[1]-h<0. || p[0]+w>self.table_size[0] || p[1]+h>self.table_size[1] {warnings.push(format!("{}: mount envelope outside table",e.id));}
            }
            let part=e.mount.as_ref().map(|m|m.part.as_str()).unwrap_or("unspecified");
            *bom.entry(format!("{:?} / {}",e.kind,part)).or_default()+=1;
            AssemblyElement { id: e.id.clone(), label: e.label.clone(), kind: e.kind, position: p, rotation: e.rotation(), normal: rotate([1.,0.,0.],e.rotation()), hole: holes, hole_offset: [p[0]-holes[0]*self.hole_pitch,p[1]-holes[1]*self.hole_pitch], mount: e.mount.clone(), lens_model: match e.kind { Kind::Lens => Some("ideal thin lens; slope approximation"), Kind::LensGroup => Some("current sequential prescription; real surfaces; scalar group transmission"), _ => None } }
        }).collect();
        for (i, a) in self.elements.iter().enumerate() {
            for b in &self.elements[i + 1..] {
                let shared_post = self.periscopes.iter().any(|p| {
                    (p.lower == a.id && p.upper == b.id) || (p.lower == b.id && p.upper == a.id)
                });
                if let (Some(ma), Some(mb), false) = (&a.mount, &b.mount, shared_post) {
                    // Separating-axis test for upright, yaw-rotated rectangular bases.
                    let overlap = [
                        a.rotation()[2],
                        a.rotation()[2] + 90.,
                        b.rotation()[2],
                        b.rotation()[2] + 90.,
                    ]
                    .iter()
                    .all(|angle| {
                        let theta = angle.to_radians();
                        let axis = [theta.cos(), theta.sin(), 0.];
                        let extent = |e: &Element, m: &Mount| {
                            let da = (e.rotation()[2] - angle).to_radians();
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
        let connections: Vec<_> = self
            .connections
            .iter()
            .map(|c| AssemblyConnection {
                from: c.from.clone(),
                port: c.port.clone(),
                to: c.to.clone(),
                distance_mm: norm(sub(positions[c.from.as_str()], positions[c.to.as_str()])),
            })
            .collect();
        let sources: Vec<_> = self
            .sources
            .iter()
            .map(|s| AssemblySource {
                id: s.id.clone(),
                position: sub(s.position, self.origin),
                direction: unit(s.direction).unwrap_or([0.; 3]),
                wavelength: s.wavelength,
                power: s.power,
                mount: (),
            })
            .collect();
        Assembly {
            name: self.name.clone(),
            units: AssemblyUnits {
                length: "mm",
                angle: "degrees",
                wavelength: "nm",
                power: "relative source units",
            },
            axes: "table XY; Z height; rotations [roll,pitch,yaw], Rz Ry Rx; local normal +X",
            origin: self.origin,
            hole_pitch: self.hole_pitch,
            items,
            sources,
            bom: parts,
            connections,
            warnings,
            scope: "geometric rays; ideal thin lenses and current-prescription lens groups; scalar group/splitter power; upright mount boxes; connection distances join component centers",
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct AssemblyUnits {
    length: &'static str,
    angle: &'static str,
    wavelength: &'static str,
    power: &'static str,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct AssemblyElement {
    id: String,
    label: String,
    kind: Kind,
    position: [f64; 3],
    rotation: [f64; 3],
    normal: [f64; 3],
    hole: [f64; 2],
    hole_offset: [f64; 2],
    mount: Option<Mount>,
    lens_model: Option<&'static str>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct AssemblySource {
    id: String,
    position: [f64; 3],
    direction: [f64; 3],
    wavelength: f64,
    power: f64,
    mount: (),
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct AssemblyConnection {
    from: String,
    port: String,
    to: String,
    distance_mm: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct Assembly {
    name: String,
    units: AssemblyUnits,
    axes: &'static str,
    origin: [f64; 3],
    hole_pitch: f64,
    items: Vec<AssemblyElement>,
    sources: Vec<AssemblySource>,
    bom: Vec<(String, usize)>,
    connections: Vec<AssemblyConnection>,
    warnings: Vec<String>,
    scope: &'static str,
}

pub fn example() -> Bench {
    let element = |id: &str, kind, position, yaw| Element {
        id: id.into(),
        label: id.into(),
        kind,
        position,
        rotation: Some([0., 0., yaw]),
        aim_rotation: false,
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
        regions: vec![],
        substrate: None,
        phase: None,
        layer: None,
        action: None,
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
            gaussian_radius: None,
            aim_direction: false,
            coherence: None,
            field: None,
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
        stages: vec![],
        branches: vec![],
        assertions: vec![],
        layers: vec![],
        periscopes: vec![],
        zones: vec![],
        folds: vec![],
        relays: vec![],
        max_interactions: 32,
        min_power: 1e-8,
    }
}
/// The beam identity of a packet leaving `element` through `port`.
pub(super) fn next_beam(beam: &[Port], element: &str, port: &str) -> Vec<Port> {
    beam.iter().cloned().chain(std::iter::once(Port::new(element, port))).collect()
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
        mirror.rotation = Some([0.; 3]);
        mirror.reflectance = 0.85;
        mirror.focal_length = 100.;
        let detector = &mut scene.elements[1];
        detector.position = [250., 200., 75.];
        detector.rotation = Some([0.; 3]);
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
        group.rotation = Some([0.; 3]);
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
    fn lens_group_propagation_integrates_glass_and_air_with_linked_segments() {
        use super::super::Surface;
        let (mut system, mut bench) = group_scene();
        system.surfaces = vec![
            Surface::sphere(0., 8., "1.5:60", 12.5),
            Surface::sphere(0., 0., "AIR", 12.5),
        ];
        bench.sources[0].position[1] = 200.;
        bench.sources[0].wavelength = 587.562; // Buchdahl reference wavelength, exactly n_d.
        bench.branches.push(Branch {
            id: "through_group".into(),
            from: Port::new("S1", "out"),
            to: Port::new("D", "in"),
            via: vec![],
        });
        for ghosts in [0, 1] {
            bench.elements[0].ghosts = ghosts;
            let trace = bench.trace(&system).unwrap();
            let arm = &trace.branches[0].paths[0];
            assert!(arm.nominal_match);
            assert!((arm.length_mm - 110.).abs() <= 1e-9);
            assert!((arm.opl_mm - 114.).abs() <= 1e-9);
            let end = trace.terminations.iter().find(|end| end.reason == "detected").unwrap();
            let exit = trace.segments[end.last_segment.unwrap()].flight.as_ref().unwrap();
            let glass = trace.segments[exit.previous_segment.unwrap()].flight.as_ref().unwrap();
            assert_eq!(glass.from_port, "surface:1");
            assert_eq!(glass.to_port, "surface:2");
            assert_eq!(glass.medium, "1.5:60");
            assert!((glass.index - 1.5).abs() <= 1e-12);
            assert!((glass.length_mm - 8.).abs() <= 1e-9);
            assert!((glass.opl_mm - 12.).abs() <= 1e-9);
            assert!((exit.length_mm - 52.).abs() <= 1e-9);
            assert_eq!(exit.medium, "AIR");
        }
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
            e.rotation = Some(angles);
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
            for (e, t) in expected.iter().zip(b.to) {
                close(*e, t);
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
    fn port_fraction(t: &Trace, port: &str) -> f64 {
        t.port_power
            .iter()
            .find(|p| p.element == "AP" && p.port == port)
            .unwrap()
            .power
            / t.power.launched
    }
    fn strip(port: &str, center: f64, width: f64) -> Region {
        Region {
            port: port.into(),
            shape: RegionShape::Rect {
                center: [center, 0.],
                size: [width, 40.],
            },
            transmittance: 1.,
            normal: None,
        }
    }
    #[test]
    fn sub_apertures_split_sampled_power_by_area() {
        // Flat-top sampler: n rings of 6k equal-power samples at radius kR/n, so each
        // sample stands for an annulus cell of width δ = R/n and the sampled disk
        // reaches ρ = R(1 + 1/(2n)). Region fractions converge to the area fraction
        // of that disk; each region edge mis-assigns at most the band of cells it
        // crosses, chord·δ / (πρ²).
        let scene = super::super::bench_lab::template("three_arm").unwrap();
        let system = System::default();
        let source = &scene.sources[0];
        let (r, n) = (source.beam_radius, source.rings as f64);
        let (rho, delta) = (r * (1. + 0.5 / n), r / n);
        let disk = |b: f64| {
            let t = (b / rho).min(1.);
            2. / std::f64::consts::PI * (t.asin() + t * (1. - t * t).sqrt())
        };
        let band = |b: f64| {
            2. * (rho * rho - b * b).max(0.).sqrt() * delta / (std::f64::consts::PI * rho * rho)
        };
        for b in [2., 5., 8.] {
            let mut bench = scene.clone();
            bench.branches.clear();
            bench.assertions.clear();
            bench.connections.retain(|c| c.from != "AP");
            bench.elements.retain(|e| e.id == "AP");
            let side = (20. - b) / 2. + b;
            bench.elements[0].regions = vec![
                strip("L", side, 20. - b),
                strip("C", 0., 2. * b),
                strip("R", -side, 20. - b),
            ];
            let trace = bench.trace(&system).unwrap();
            accounted(&trace);
            let [l, c, rr] = ["L", "C", "R"].map(|p| port_fraction(&trace, p));
            let expected = disk(b);
            let tolerance = 2. * band(b);
            println!("b={b}: C {c:.4} vs {expected:.4} (tol {tolerance:.4}), L {l:.4} R {rr:.4}");
            assert!((c - expected).abs() <= tolerance);
            assert!((l - (1. - expected) / 2.).abs() <= band(b));
            close(l, rr);
            close(l + c + rr, 1.);
            close(trace.power.blocked, 0.);
        }
    }
    #[test]
    fn gaussian_source_weights_sub_aperture_power_by_the_gaussian_integral() {
        // One centred hole of radius a in a Gaussian beam (1/e² radius w) sampled over
        // the disk ρ = R(1 + 1/(2n)). Exact: the independently summed sample weights
        // exp(-2r²/w²) inside the hole. Analytic: (1 - e^{-2a²/w²}) / (1 - e^{-2ρ²/w²}),
        // within the Gaussian mass of one cell-width band δ at the hole edge.
        let mut scene = super::super::bench_lab::template("three_arm").unwrap();
        let system = System::default();
        let (w, a) = (8., 6.);
        scene.sources[0].gaussian_radius = Some(w);
        scene.branches.clear();
        scene.assertions.clear();
        scene.connections.retain(|c| c.from != "AP");
        scene.elements.retain(|e| e.id == "AP");
        let plate = &mut scene.elements[0];
        plate.regions.retain(|r| r.port == "C");
        plate.regions[0].shape = RegionShape::Circle {
            center: [0., 0.],
            radius: a,
        };
        let source = &scene.sources[0];
        let (r, n) = (source.beam_radius, source.rings);
        let g = |x: f64| (-2. * x * x / (w * w)).exp();
        let (mut inside, mut total) = (g(0.), g(0.));
        for k in 1..=n {
            let radius = r * k as f64 / n as f64;
            let weight = 6. * k as f64 * g(radius);
            total += weight;
            if radius <= a {
                inside += weight;
            }
        }
        let trace = scene.trace(&system).unwrap();
        accounted(&trace);
        let f = port_fraction(&trace, "C");
        close(f, inside / total);
        let (rho, delta) = (r * (1. + 0.5 / n as f64), r / n as f64);
        let norm = 1. - g(rho);
        let analytic = (1. - g(a)) / norm;
        let band = 2. * std::f64::consts::PI * a * delta * 2. / (std::f64::consts::PI * w * w) * g(a) / norm;
        println!("gaussian: {f:.4} vs {analytic:.4} (band {band:.4})");
        assert!((f - analytic).abs() <= band, "{f} vs {analytic}");
        // The profile really is Gaussian: the sampled power differs from flat-top.
        let mut flat = scene.clone();
        flat.sources[0].gaussian_radius = None;
        assert!(port_fraction(&flat.trace(&system).unwrap(), "C") < f - 0.1);
        close(trace.power.blocked / trace.power.launched, 1. - f);
    }
    #[test]
    fn gaussian_source_splits_three_holes_by_the_gaussian_integral() {
        // The three_arm mask (holes of radius a = 3 at y = +10, 0, -10) in a Gaussian
        // beam of 1/e² radius w. Exact: independently summed sample weights inside
        // each hole. Analytic: the Gaussian integral over each hole (fine quadrature),
        // normalised to the sampled disk ρ; tolerance is the Gaussian mass of a
        // cell-width (δ) band along the hole edge, taken at its brightest point.
        let mut scene = super::super::bench_lab::template("three_arm").unwrap();
        let system = System::default();
        let w = 12.;
        scene.sources[0].gaussian_radius = Some(w);
        let trace = scene.trace(&system).unwrap();
        accounted(&trace);
        let source = &scene.sources[0];
        let (r, n) = (source.beam_radius, source.rings);
        let (rho, delta) = (r * (1. + 0.5 / n as f64), r / n as f64);
        let intensity = |x: f64, y: f64| 2. / (std::f64::consts::PI * w * w) * (-2. * (x * x + y * y) / (w * w)).exp();
        let norm = 1. - (-2. * rho * rho / (w * w)).exp();
        let mut points = vec![([0., 0.], 1.)];
        for k in 1..=n {
            let radius = r * k as f64 / n as f64;
            for j in 0..6 * k {
                let angle = std::f64::consts::TAU * j as f64 / (6 * k) as f64;
                let weight = (-2. * radius * radius / (w * w)).exp();
                points.push(([-radius * angle.cos(), -radius * angle.sin()], weight));
            }
        }
        let total: f64 = points.iter().map(|p| p.1).sum();
        let a = 3.;
        for (port, centre) in [("L", 10.), ("C", 0.), ("R", -10.)] {
            let exact: f64 = points
                .iter()
                .filter(|(p, _)| (p[0] - centre).hypot(p[1]) <= a)
                .map(|p| p.1)
                .sum::<f64>()
                / total;
            let f = port_fraction(&trace, port);
            close(f, exact);
            let steps = 400;
            let mut analytic = 0.;
            for i in 0..steps {
                let radius = a * (i as f64 + 0.5) / steps as f64;
                for j in 0..steps {
                    let angle = std::f64::consts::TAU * (j as f64 + 0.5) / steps as f64;
                    let (x, y) = (centre + radius * angle.cos(), radius * angle.sin());
                    analytic += intensity(x, y) * radius;
                }
            }
            analytic *= a / steps as f64 * std::f64::consts::TAU / steps as f64 / norm;
            let brightest = intensity((centre.abs() - a).max(0.), 0.);
            let band = std::f64::consts::TAU * a * delta * brightest / norm;
            println!("{port}: {f:.5} vs {analytic:.5} (band {band:.5})");
            assert!((f - analytic).abs() <= band, "{port}: {f} vs {analytic}");
        }
        close(port_fraction(&trace, "L"), port_fraction(&trace, "R"));
        assert!(port_fraction(&trace, "C") > 1.5 * port_fraction(&trace, "L"));
    }
    #[test]
    fn region_arms_keep_their_beam_through_a_shared_plain_element() {
        // AP/L, C, R all pass one plain aperture A before a second mask AP2: each arm
        // is a different incoming beam at AP2 and must keep its own anchor.
        let system = System::default();
        let mut bench = super::super::bench_lab::template("three_arm").unwrap();
        let plate = bench.elements.iter().find(|e| e.id == "AP").unwrap().clone();
        let mut common = plate.clone();
        common.id = "A".into();
        common.position[0] += 50.;
        common.aperture = 15.;
        common.regions.clear();
        let mut second = plate;
        second.id = "AP2".into();
        second.position[0] += 100.;
        bench.elements.retain(|e| !e.id.starts_with("D_"));
        bench.elements.extend([common, second]);
        bench.connections = [("S1", "out", "AP"), ("AP", "L", "A"), ("AP", "C", "A"), ("AP", "R", "A"), ("A", "out", "AP2")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = ["L", "C", "R"]
            .map(|arm| Branch {
                id: arm.into(),
                from: Port::new("AP", arm),
                to: Port::new("AP2", "in"),
                via: vec![],
            })
            .into();
        let trace = bench.trace(&system).unwrap();
        for arm in ["L", "C", "R"] {
            let report = trace.branches.iter().find(|b| b.id == arm).unwrap();
            assert!(report.interruption.is_none(), "{arm}");
            assert_eq!(report.paths.len(), 1, "{arm}: {:?}", report.paths);
            assert!(report.paths[0].interruption.is_none(), "{arm}: {:?}", report.paths[0].interruption);
            assert!((report.paths[0].length_mm - 100.).abs() < 1e-9);
        }
    }
    #[test]
    fn illumination_is_judged_per_source() {
        // S2 is shifted +12 mm: its samples span y ∈ [-2, 26] and miss the R hole,
        // which S1 still lights. Only S2's R path is not illuminated.
        let system = System::default();
        let mut twin = super::super::bench_lab::template("three_arm").unwrap();
        let mut second = twin.sources[0].clone();
        second.id = "S2".into();
        second.position[1] += 12.;
        twin.sources.push(second);
        twin.connections.push(Connection {
            from: "S2".into(),
            port: "out".into(),
            to: "AP".into(),
        });
        let trace = twin.trace(&system).unwrap();
        accounted(&trace);
        for arm in ["L", "C", "R"] {
            let report = trace.branches.iter().find(|b| b.id == arm).unwrap();
            assert_eq!(report.paths.len(), 2, "{arm}");
            for path in &report.paths {
                let unlit = arm == "R" && path.source == "S2";
                let reason = path.interruption.as_ref().map(|b| (b.element.as_str(), b.port.as_str(), b.reason.as_str()));
                assert_eq!(reason, unlit.then_some(("AP", "R", "not_illuminated")), "{arm} {}", path.source);
            }
        }
        assert!(port_fraction(&trace, "R") > 0.);
    }
    #[test]
    fn region_arms_start_once_per_source_and_upstream_port() {
        let system = System::default();
        // Two sources on one mask: each arm gets one path per source.
        let mut twin = super::super::bench_lab::template("three_arm").unwrap();
        let mut second = twin.sources[0].clone();
        second.id = "S2".into();
        second.position[1] += 1.;
        twin.sources.push(second);
        twin.connections.push(Connection {
            from: "S2".into(),
            port: "out".into(),
            to: "AP".into(),
        });
        let trace = twin.trace(&system).unwrap();
        accounted(&trace);
        for (arm, length) in [("L", 200.), ("C", 250.), ("R", 300.)] {
            let report = trace.branches.iter().find(|b| b.id == arm).unwrap();
            assert!(report.interruption.is_none(), "{arm}: {:?}", report.interruption);
            let mut sources: Vec<_> = report.paths.iter().map(|p| p.source.as_str()).collect();
            sources.sort();
            assert_eq!(sources, ["S1", "S2"], "{arm}");
            for path in &report.paths {
                assert!(path.interruption.is_none());
                assert!((path.length_mm - length).abs() < 1e-9, "{arm}: {}", path.length_mm);
            }
        }
        // Cascade: the L arm (led by a zero-power probe) meets a second region plate.
        // Its arms U / V start once each from that probe, never again from samples.
        let mut cascade = super::super::bench_lab::template("three_arm").unwrap();
        let mut second = cascade.elements.iter().find(|e| e.id == "AP").unwrap().clone();
        second.id = "AP2".into();
        second.position[0] += 100.;
        second.position[1] += 10.;
        second.regions = [("U", 0., 1.), ("V", -2., 0.8)]
            .map(|(port, y, radius)| Region {
                port: port.into(),
                shape: RegionShape::Circle {
                    center: [y, 0.],
                    radius,
                },
                transmittance: 1.,
                normal: None,
            })
            .into();
        // Only the L arm continues (the second mask would block C / R anyway).
        cascade.elements.retain(|e| e.id != "D_C" && e.id != "D_R");
        let first = cascade.elements.iter_mut().find(|e| e.id == "AP").unwrap();
        first.regions.retain(|r| r.port == "L");
        cascade.elements.push(second);
        cascade.connections.retain(|c| c.from != "AP");
        for (from, port, to) in [("AP", "L", "AP2"), ("AP2", "U", "D_L"), ("AP2", "V", "D_L")] {
            cascade.connections.push(Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            });
        }
        cascade.branches.clear();
        for arm in ["U", "V"] {
            cascade.branches.push(Branch {
                id: arm.into(),
                from: Port::new("AP2", arm),
                to: Port::new("D_L", "in"),
                via: vec![],
            });
        }
        let trace = cascade.trace(&system).unwrap();
        accounted(&trace);
        for arm in ["U", "V"] {
            let report = trace.branches.iter().find(|b| b.id == arm).unwrap();
            assert!(report.interruption.is_none(), "{arm}: {:?}", report.interruption);
            assert_eq!(report.paths.len(), 1, "{arm}: {:?}", report.paths);
            assert!(report.paths[0].interruption.is_none(), "{arm}");
            assert!((report.paths[0].length_mm - 100.).abs() < 1e-9);
        }
        assert!(["U", "V"].iter().all(|p| trace.port_power.iter().any(|q| q.element == "AP2" && q.port == *p && q.power > 0.)));
    }
    #[test]
    fn unlit_sub_aperture_reports_zero_power_and_unreachable_arm() {
        let scene = super::super::bench_lab::template("three_arm").unwrap();
        let system = System::default();
        let centered = scene.trace(&system).unwrap();
        accounted(&centered);
        let source = &scene.sources[0];
        let rho = source.beam_radius * (1. + 0.5 / source.rings as f64);
        let delta = source.beam_radius / source.rings as f64;
        // Exact expectation: count the sampler's equal-power points inside each hole.
        // For a +X beam the sample offset is (y, z) = (-r cos a, -r sin a).
        let samples = |dy: f64| -> Vec<[f64; 2]> {
            let n = source.rings;
            let mut out = vec![[dy, 0.]];
            for k in 1..=n {
                let radius = source.beam_radius * k as f64 / n as f64;
                for j in 0..6 * k {
                    let a = std::f64::consts::TAU * j as f64 / (6 * k) as f64;
                    out.push([dy - radius * a.cos(), -radius * a.sin()]);
                }
            }
            out
        };
        let plate = scene.elements.iter().find(|e| e.id == "AP").unwrap();
        let dy0 = scene.sources[0].position[1] - plate.position[1];
        let expect = |pts: &[[f64; 2]], port: &str| {
            pts.iter()
                .filter(|p| p[0].hypot(p[1]) <= plate.aperture)
                .filter(|p| {
                    // Independent of Element::region_at: holes of radius 3 at y = +10, 0, -10.
                    let centre = match port {
                        "L" => 10.,
                        "C" => 0.,
                        _ => -10.,
                    };
                    (p[0] - centre).hypot(p[1]) <= 3.
                })
                .count() as f64
                / pts.len() as f64
        };
        let pts = samples(dy0);
        for port in ["L", "C", "R"] {
            // Exact sample count, and consistent with the hole area 9/ρ² within the
            // sampler's worst-case edge band 2·3·δ/ρ².
            let f = port_fraction(&centered, port);
            close(f, expect(&pts, port));
            assert!(
                (f - 9. / (rho * rho)).abs() <= 6. * delta / (rho * rho),
                "{port}: {f}"
            );
        }
        // Area consistency with a margin smaller than the expectation: one 10 mm hole,
        // expected 100/ρ², worst-case edge band 2·10·δ/ρ² (≈35 % of the value).
        let mut wide = scene.clone();
        wide.elements.iter_mut().find(|e| e.id == "AP").unwrap().regions.retain(|r| r.port == "C");
        if let RegionShape::Circle { radius, .. } = &mut wide.elements.iter_mut().find(|e| e.id == "AP").unwrap().regions[0].shape {
            *radius = 10.;
        }
        wide.connections.retain(|c| c.from != "AP" || c.port == "C");
        wide.branches.retain(|b| b.id == "C");
        wide.assertions.clear();
        let f = port_fraction(&wide.trace(&system).unwrap(), "C");
        assert!((f - 100. / (rho * rho)).abs() <= 20. * delta / (rho * rho), "{f}");
        let lit: f64 = ["L", "C", "R"]
            .iter()
            .map(|p| port_fraction(&centered, p))
            .sum();
        close(centered.power.blocked / centered.power.launched, 1. - lit);
        assert!(
            centered
                .branches
                .iter()
                .all(|b| b.interruption.is_none()
                    && b.paths.iter().all(|p| p.interruption.is_none()))
        );

        // Shift the beam 12 mm towards +Y: it spans y ∈ [-2, 26], missing the R hole at y = -10.
        let mut shifted = scene.clone();
        shifted.sources[0].position[1] += 12.;
        let trace = shifted.trace(&system).unwrap();
        accounted(&trace);
        close(port_fraction(&trace, "R"), 0.);
        let pts = samples(dy0 + 12.);
        for port in ["L", "C"] {
            close(port_fraction(&trace, port), expect(&pts, port));
        }
        // Samples beyond the plate body miss it; between aperture and body they are
        // clipped, and inside the aperture but outside every hole they are masked.
        let count = |f: &dyn Fn(f64) -> bool| {
            pts.iter().filter(|p| f(p[0].hypot(p[1]))).count() as f64 / pts.len() as f64
        };
        close(trace.power.escaped / trace.power.launched, count(&|r| r > plate.body_radius));
        let lit_now: f64 = ["L", "C"].iter().map(|p| port_fraction(&trace, p)).sum();
        close(
            trace.power.blocked / trace.power.launched,
            count(&|r| r <= plate.body_radius) - lit_now,
        );
        // Samples beyond the plate body miss it and escape; the rest outside the holes are masked.
        let lit: f64 = ["L", "C", "R"]
            .iter()
            .map(|p| port_fraction(&trace, p))
            .sum();
        close(
            (trace.power.blocked + trace.power.escaped) / trace.power.launched,
            1. - lit,
        );
        close(trace.power.detected / trace.power.launched, lit);
        let arm = trace.branches.iter().find(|b| b.id == "R").unwrap();
        let path = &arm.paths[0];
        let broken = path
            .interruption
            .as_ref()
            .or(arm.interruption.as_ref())
            .unwrap();
        assert_eq!(
            (
                broken.element.as_str(),
                broken.port.as_str(),
                broken.reason.as_str()
            ),
            ("AP", "R", "not_illuminated")
        );
        // The geometric probe still measures the arm from the hole centre to D_R.
        close(path.length_mm, 300.);
        // A fold over the dark arm is unlit too.
        let mut folded = shifted.clone();
        folded.folds = vec![folded.branches.iter().find(|b| b.id == "R").unwrap().clone()];
        let fold = &folded.trace(&system).unwrap().folds[0];
        assert_eq!(fold.paths[0].interruption.as_ref().map(|b| b.reason.as_str()), Some("not_illuminated"));
        for id in ["L", "C"] {
            let arm = trace.branches.iter().find(|b| b.id == id).unwrap();
            assert!(
                arm.interruption.is_none() && arm.paths[0].interruption.is_none(),
                "{id}"
            );
        }
        assert!(
            trace
                .terminations
                .iter()
                .all(|t| !t.branch.contains("~probe"))
        );

        // Shift 26 mm: the chief sample misses the plate body entirely, but a few
        // samples still pass L. L stays reachable; C and R are unlit.
        let mut far = scene.clone();
        far.sources[0].position[1] += 26.;
        let trace = far.trace(&system).unwrap();
        accounted(&trace);
        let pts = samples(dy0 + 26.);
        assert!((dy0 + 26.) > plate.body_radius);
        for id in ["L", "C", "R"] {
            close(port_fraction(&trace, id), expect(&pts, id));
            let arm = trace.branches.iter().find(|b| b.id == id).unwrap();
            assert_eq!(arm.paths.len(), 1, "{id}");
            let broken = arm.paths[0].interruption.as_ref().or(arm.interruption.as_ref());
            if id == "L" {
                assert!(port_fraction(&trace, id) > 0. && broken.is_none(), "{broken:?}");
            } else {
                assert_eq!(broken.map(|b| b.reason.as_str()), Some("not_illuminated"));
            }
        }

        // Shift 5 mm: the chief sample lands on the plate between holes, yet every
        // hole passes samples, so all three arms stay reachable with their power.
        let mut between = scene.clone();
        between.sources[0].position[1] += 5.;
        let trace = between.trace(&system).unwrap();
        accounted(&trace);
        let pts = samples(dy0 + 5.);
        assert!((dy0 + 5. - 0f64).abs() > 3. && (dy0 + 5. - 10f64).abs() > 3.);
        // Hole centre to detector: D_L, D_C, D_R sit 200, 250, 300 mm downstream.
        for (id, length) in [("L", 200.), ("C", 250.), ("R", 300.)] {
            let f = port_fraction(&trace, id);
            close(f, expect(&pts, id));
            assert!(f > 0., "{id}");
            let arm = trace.branches.iter().find(|b| b.id == id).unwrap();
            assert!(
                arm.interruption.is_none()
                    && !arm.paths.is_empty()
                    && arm.paths.iter().all(|p| p.interruption.is_none()),
                "{id}: {:?}",
                arm.interruption
            );
            close(arm.paths[0].length_mm, length);
        }
    }
    #[test]
    fn source_branches_cross_a_region_plate_whose_chief_is_masked() {
        // Shifted 5 mm, the representative sample lands between holes and is masked,
        // yet every hole is lit, so branches from the source through each hole reach
        // their detectors; their length adds the source-to-plate flight.
        let system = System::default();
        let mut bench = super::super::bench_lab::template("three_arm").unwrap();
        bench.sources[0].position[1] += 5.;
        let direct = bench.trace(&system).unwrap();
        bench.branches = [("L", "D_L"), ("C", "D_C"), ("R", "D_R")]
            .map(|(arm, detector)| Branch {
                id: arm.into(),
                from: Port::new("S1", "out"),
                to: Port::new(detector, "in"),
                via: vec![Port::new("AP", arm)],
            })
            .into();
        bench.assertions.clear();
        let trace = bench.trace(&system).unwrap();
        accounted(&trace);
        let chief = &trace.segments[trace.branches[0].paths[0].segment_indices[0]];
        assert_eq!(chief.status, "masked");
        let approach = super::super::math::norm(super::super::math::sub(chief.to, chief.from));
        for arm in ["L", "C", "R"] {
            let report = trace.branches.iter().find(|b| b.id == arm).unwrap();
            let arm_only = direct.branches.iter().find(|b| b.id == arm).unwrap();
            assert_eq!(report.paths.len(), 1, "{arm}");
            let path = &report.paths[0];
            assert!(path.interruption.is_none(), "{arm}: {:?}", path.interruption);
            close(path.length_mm, approach + arm_only.paths[0].length_mm);
        }

        // Moved fully off the plate's R hole the source branch through R is unlit.
        bench.sources[0].position[1] += 7.;
        let trace = bench.trace(&system).unwrap();
        let report = trace.branches.iter().find(|b| b.id == "R").unwrap();
        let reason = report.paths[0].interruption.as_ref().map(|b| (b.element.as_str(), b.port.as_str(), b.reason.as_str()));
        assert_eq!(reason, Some(("AP", "R", "not_illuminated")));
    }
    #[test]
    fn port_names_never_alias_beam_identity() {
        // A legal port name that looks like an internal marker keeps its own power,
        // whether it names the hole the chief ray lights (C) or an off-axis one (R),
        // and the Gaussian envelope and lab check stay consistent.
        let system = System::default();
        let renamed = "L~probe";
        for original in ["C", "R"] {
            let mut bench = super::super::bench_lab::template("three_arm").unwrap();
            for region in bench.elements.iter_mut().flat_map(|e| e.regions.iter_mut()) {
                if region.port == original {
                    region.port = renamed.into();
                }
            }
            for connection in &mut bench.connections {
                if connection.from == "AP" && connection.port == original {
                    connection.port = renamed.into();
                }
            }
            for branch in &mut bench.branches {
                if branch.from.port == original {
                    branch.from.port = renamed.into();
                }
            }
            let trace = bench.trace(&system).unwrap();
            accounted(&trace);
            assert!(port_fraction(&trace, renamed) > 0., "{original}");
            for report in &trace.branches {
                assert!(!report.paths.is_empty(), "{original} {}", report.id);
                for path in &report.paths {
                    assert!(path.interruption.is_none(), "{original} {}: {:?}", report.id, path.interruption);
                }
            }
            let envelope = super::super::bench_lab::gaussian_envelope(&bench, &system);
            assert!(envelope.is_ok(), "{original}: {envelope:?}");
            let check = super::super::bench_lab::lab_check(&bench, &system);
            assert!(check.is_ok(), "{original}: {check:?}");
        }
    }
    #[test]
    fn prism_facets_split_by_landing_point_and_obey_reflection() {
        let mut bench = super::super::bench_lab::template("three_arm").unwrap();
        bench.branches.clear();
        bench.assertions.clear();
        bench.connections.retain(|c| c.from != "AP");
        bench.elements.retain(|e| e.id == "AP");
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let facet = |port: &str, center: f64, ny: f64| Region {
            port: port.into(),
            shape: RegionShape::Rect {
                center: [center, 0.],
                size: [15., 30.],
            },
            transmittance: 1.,
            normal: Some([h, ny, 0.]),
        };
        let ap = &mut bench.elements[0];
        ap.kind = Kind::Mirror;
        ap.reflectance = 1.;
        ap.transmittance = 0.;
        ap.regions = vec![facet("top", 7.5, h), facet("bottom", -7.5, -h)];
        let trace = bench.trace(&System::default()).unwrap();
        accounted(&trace);
        let source = &bench.sources[0];
        let samples = (1 + 3 * source.rings * (source.rings + 1)) as f64;
        // Samples on y = 0 (centre plus the a = π/2, 3π/2 points of even rings) may land
        // on either facet: at most 2n + 1 samples, half of them per side.
        let tolerance = (2. * source.rings as f64 + 1.) / (2. * samples);
        let d = [1., 0., 0.];
        for (port, ny) in [("top", h), ("bottom", -h)] {
            let f = port_fraction(&trace, port);
            assert!((f - 0.5).abs() <= tolerance, "{port}: {f}");
            let n = [h, ny, 0.];
            let expected = sub(d, scale(n, 2. * dot(d, n)));
            let leg = trace
                .segments
                .iter()
                .find(|s| s.branch.ends_with(&format!("AP:{port}")))
                .unwrap();
            let actual = unit(sub(leg.to, leg.from)).unwrap();
            for i in 0..3 {
                close(actual[i], expected[i]);
            }
        }
        close(
            port_fraction(&trace, "top") + port_fraction(&trace, "bottom"),
            1.,
        );
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
        detector.rotation = Some([0., 0., 90.]);
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
        b.elements[0].rotation = Some([0., 45., 0.]);
        b.elements[1].position = [300., 200., 275.];
        b.elements[1].rotation = Some([0., 90., 0.]);
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
            e.rotation.as_mut().unwrap()[2] += 37.;
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
        obstacle.rotation = Some([0.; 3]);
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
            e.rotation = Some([0.; 3]);
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
            e.rotation = Some([0.; 3]);
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
