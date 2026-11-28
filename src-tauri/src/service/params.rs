use super::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProjectFormat {
    Geopter,
    Hissabeth,
    Zmx,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Coordinates {
    #[schemars(default, with = "usize", range(min = 0, max = 63))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_index: Option<usize>,
    #[schemars(default, with = "f64", range(min = -1e9, max = 1e9))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<f64>,
    #[schemars(default, with = "f64", range(min = -1e9, max = 1e9))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_x: Option<f64>,
}
impl Coordinates {
    pub fn select(&self, system: &System) -> Result<Field, String> {
        select_field(system, self.field_index, self.field, self.field_x)
    }
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Empty {}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchSet {
    pub bench: bench::Bench,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchDraft {
    #[schemars(default, with = "bench::Bench")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench: Option<bench::Bench>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchCheck {
    #[schemars(default, with = "bench::Bench")]
    pub bench: Option<bench::Bench>,
    #[serde(default)]
    pub require_pass: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchField {
    #[schemars(default, with = "bench::Bench")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench: Option<bench::Bench>,
    /// Named branch whose end field is read. Give exactly one of branch or port.
    #[schemars(default, with = "String")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Input port where every named branch ending there is superposed.
    #[schemars(default, with = "bench::Port")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<bench::Port>,
    /// Keep only arms at this wavelength (nm).
    #[schemars(default, with = "f64")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
    /// Ground-glass realizations averaged, 1..256 (default 1).
    #[schemars(default, with = "u32", range(min = 1, max = 256))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realizations: Option<usize>,
    /// Include block-averaged intensity and phase arrays of at most 64 x 64.
    #[serde(default)]
    pub arrays: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchReadout {
    #[schemars(default, with = "bench::Bench")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench: Option<bench::Bench>,
    /// Detector element carrying the readout.
    pub detector: String,
    /// Ground-glass realizations averaged, 1..256 (default 1).
    #[schemars(default, with = "u32", range(min = 1, max = 256))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realizations: Option<usize>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchDrawing {
    #[schemars(default, with = "bench::Bench")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench: Option<bench::Bench>,
    /// Label text height in table millimetres, 1..50 (default 4, as in the SVG blueprint).
    #[schemars(default, with = "f64", range(min = 1, max = 50))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_mm: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchCatalog {
    #[schemars(default, with = "String")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[schemars(default, with = "String")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchTemplate {
    #[schemars(default, with = "bool")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply: Option<bool>,
    #[schemars(extend("enum" = bench_lab::TEMPLATES))]
    pub name: String,
    #[schemars(default, with = "u64", range(min = 0, max = 9_007_199_254_740_991_u64))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchAutoAim {
    #[schemars(default, with = "bool")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply: Option<bool>,
    #[schemars(default, with = "bench::Bench")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench: Option<bench::Bench>,
    #[schemars(default, with = "u64", range(min = 0, max = 9_007_199_254_740_991_u64))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[schemars(default, with = "bool")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_holes: Option<bool>,
    #[schemars(default, with = "f64", extend("maximum" = 1e6, "exclusiveMinimum" = 0.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniform_z: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchExportBlueprint {
    #[schemars(default, with = "bench::Bench")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench: Option<bench::Bench>,
    #[schemars(extend("enum" = bench_lab::FORMATS))]
    pub format: String,
    /// PNG resolution in pixels per millimetre of drawing (default 4, at most
    /// 40); each side is rounded to the nearest pixel. Only for png.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0, max = 40.0))]
    pub px_per_mm: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactStart {
    #[schemars(extend("enum" = ["blueprint"]))]
    pub kind: String,
    #[schemars(extend("enum" = bench_lab::FORMATS))]
    pub format: String,
    #[serde(default)]
    pub params: ArtifactParams,
}

#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactParams {
    #[schemars(default, with = "bench::Bench")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bench: Option<bench::Bench>,
    /// PNG resolution in pixels per millimetre of drawing (default 4, at most
    /// 40); each side is rounded to the nearest pixel. Only for png.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0.0, max = 40.0))]
    pub px_per_mm: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactId {
    pub id: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactExport {
    pub id: String,
    pub path: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Revision {
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkspaceRecover {
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    pub source: workspace::Source,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SystemSet {
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    pub system: System,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SystemDraft {
    pub system: System,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectIndex {
    #[schemars(range(min = 0, max = 63))]
    pub index: usize,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SurfaceEdit {
    pub operation: editing::Operation,
    pub system: System,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Optimize {
    #[serde(flatten)]
    pub request: optimize::Request,
    #[schemars(default, with = "bool")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply: Option<bool>,
    #[schemars(default, with = "u64", range(min = 0, max = 9_007_199_254_740_991_u64))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct HistoryCompare {
    #[schemars(default, with = "usize", range(min = 0, max = 63))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_index: Option<usize>,
    #[schemars(default, with = "usize", range(min = 0, max = 63))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_index: Option<usize>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectPath {
    pub path: String,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct MaterialSearch {
    #[schemars(default, with = "String")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    #[schemars(default, with = "usize", range(min = 1, max = 500))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[schemars(default, with = "usize", range(min = 0, max = 100000))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    #[schemars(default, with = "String")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct MaterialIndex {
    pub material: String,
    #[schemars(default, with = "f64", range(min = 0.0, max = 1e8))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure: Option<f64>,
    #[schemars(default, with = "f64", range(min = -200.0, max = 500.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[schemars(range(min = 100.0, max = 20000.0))]
    pub wavelength: f64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AirIndex {
    #[schemars(default, with = "f64", range(min = 0.0, max = 1e8))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure: Option<f64>,
    #[schemars(default, with = "f64", range(min = -200.0, max = 500.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[schemars(range(min = 100.0, max = 20000.0))]
    pub wavelength: f64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogImport {
    pub catalog: String,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    pub text: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct TraceInspect {
    pub launch: inspection::InspectionLaunch,
    #[schemars(range(min = 100.0, max = 20000.0))]
    pub wavelength: f64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ParaxialTrace {
    #[schemars(range(min = 0, max = 63))]
    pub field_index: usize,
    #[schemars(range(min = 100.0, max = 20000.0))]
    pub wavelength: f64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FieldCurvature {
    #[schemars(default, with = "[f64; 2]")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_end: Option<[f64; 2]>,
    #[schemars(range(min = 2, max = 257))]
    pub samples: usize,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PrescriptionReport {
    #[schemars(range(min = 0, max = 15))]
    pub precision: usize,
    #[schemars(length(min = 0, max = 5))]
    pub sections: Vec<report::SectionId>,
    #[schemars(range(min = 100.0, max = 20000.0))]
    pub wavelength: f64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FieldWavelength {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct TraceRay {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
    #[schemars(default, with = "f64", range(min = -1e6, max = 1e6))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[schemars(default, with = "f64", range(min = -1e6, max = 1e6))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SpotDiagram {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    pub sampling: PupilSampling,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PolychromaticSpot {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    pub sampling: PupilSampling,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GeometricMtf {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "String", extend("enum" = ["x", "y"]))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<String>,
    #[schemars(default, with = "f64", extend("maximum" = 10000.0, "exclusiveMinimum" = 0.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_frequency: Option<f64>,
    pub sampling: PupilSampling,
    pub spectrum: Spectrum,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Fan {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "usize", range(min = 3, max = 257))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samples: Option<usize>,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Wavefront {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "usize", range(min = 5, max = 129))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<usize>,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DiffractionPsf {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "usize", extend("enum" = [8, 16, 32, 64, 128]))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<usize>,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DiffractionMtf {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "usize", extend("enum" = [8, 16, 32, 64, 128]))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<usize>,
    #[schemars(default, with = "f64", extend("exclusiveMinimum" = 0.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_frequency: Option<f64>,
    pub spectrum: Spectrum,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct EncircledEnergy {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "usize", range(min = 2, max = 257))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samples: Option<usize>,
    pub sampling: PupilSampling,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WavefrontZernike {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "usize", range(min = 5, max = 129))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<usize>,
    #[schemars(default, with = "usize", range(min = 1, max = 45))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terms: Option<usize>,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct HuygensPsf {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "usize", range(min = 5, max = 129))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<usize>,
    #[schemars(default, with = "f64", extend("exclusiveMinimum" = 0.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    #[schemars(default, with = "usize", range(min = 3, max = 129))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<usize>,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct HuygensMtf {
    #[serde(flatten)]
    pub coordinates: Coordinates,
    #[schemars(default, with = "usize", range(min = 5, max = 129))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<usize>,
    #[schemars(default, with = "f64", extend("exclusiveMinimum" = 0.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_frequency: Option<f64>,
    pub spectrum: Spectrum,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GaussianBeam {
    #[schemars(range(min = 1e-06, max = 1000.0))]
    pub waist: f64,
    #[schemars(default, with = "f64", range(min = -1e9, max = 1e9))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waist_z: Option<f64>,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FiberCoupling {
    #[schemars(range(min = 1e-06, max = 1000.0))]
    pub mode_field_diameter: f64,
    #[schemars(range(min = 1e-06, max = 1000.0))]
    pub waist: f64,
    #[schemars(default, with = "f64", range(min = -1e9, max = 1e9))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waist_z: Option<f64>,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ChromaticFocus {
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lower: Option<f64>,
    #[schemars(default, with = "usize", range(min = 2, max = 257))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samples: Option<usize>,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upper: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SurfaceSag {
    #[schemars(range(min = 0.0, max = 1e6))]
    pub height: f64,
    #[schemars(range(min = 0, max = 127))]
    pub surface: usize,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Autofocus {
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    #[schemars(default, with = "f64", range(min = 100.0, max = 20000.0))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wavelength: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Solve {
    pub definition: solve::GapSolve,
    #[schemars(range(min = 0, max = 127))]
    pub gap: usize,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectImport {
    pub format: ProjectFormat,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    pub text: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectExport {
    pub format: ProjectFormat,
}
