export type Interaction = 'refract' | 'reflect' | 'dummy';
export type FieldType = 'angle' | 'object_height' | 'image_height';
export type PupilMapping = 'reference_entrance_pupil' | 'real_stop';
export type PupilKind = 'entrance_pupil_diameter' | 'f_number' | 'object_na' | 'image_na';

/**
 * Relation controlling the thickness AFTER a surface. Every index is zero-based.
 * `overall_length` sums the half-open gap interval [first, last).
 */
export type GapSolve =
    | { kind: 'fixed' }
    | { kind: 'pickup'; source: number; scale: number; offset: number }
    | { kind: 'edge_thickness'; height: number; value: number }
    | { kind: 'overall_length'; first: number; last: number; value: number }
    | { kind: 'marginal_height'; value: number; zone: number };

export interface Surface {
    label: string;
    radius: number;
    /** Distance to the next surface; the resolved value when `gap_solve` is active. */
    thickness: number;
    gap_solve: GapSolve;
    material: string;
    /** Physical circular clear radius in mm; blocks rays only when `clear_aperture` is true. */
    aperture: number;
    /** true: `aperture` is a physical circular aperture that blocks rays; false: it never clips. */
    clear_aperture: boolean;
    /**
     * Derived, read-only drawing radius in mm (>= 0): chief plus four field-local pupil rim rays at the
     * reference wavelength across all stored fields. Recomputed by the backend on every model update.
     */
    semi_diameter: number;
    conic: number;
    even: number[];
    odd: number[];
    /**
     * Optional toroidal profile (ZEMAX TOROIDAL): the YZ conic section swept about an axis parallel
     * to Y at distance `revolution`; `even_y` holds y^4, y^6, ... coefficients on that section.
     * revolution = 0 gives a flat X section (cylinder). Absent on radial surfaces.
     */
    toroid?: { revolution: number; even_y: number[] } | null;
    /**
     * Optional Zernike Fringe sag (ZEMAX ZERNFRIN): Σ c_j·Z_j(x/normalize, y/normalize) in mm,
     * Z_j in Fringe order (at most 37 terms), added to any base profile.
     */
    zernike?: { normalize: number; coefficients: number[] } | null;
    /** Optional 3D coordinate break / surface tilt & decenter transform. */
    transform?: CoordinateTransform | null;
    interaction: Interaction;
}

export interface CoordinateTransform {
    decenter: [number, number];
    tilt: [number, number, number];
    tilt_first: boolean;
    return_after: boolean;
}

/** Four finite values in 0..1. Plots use RGB; alpha is stored metadata. */
export type RGBA = [number, number, number, number];

/** Profile and aperture of the object or image endpoint: same conventions as an internal surface, without gap, material or interaction. */
export interface EndpointSurface {
    radius: number;
    conic: number;
    even: number[];
    odd: number[];
    /** Physical circular clear radius in mm; blocks rays only when `clear_aperture` is true. */
    aperture: number;
    clear_aperture: boolean;
    /** Derived, read-only drawing radius in mm (>= 0); 0 for an object at infinity. */
    semi_diameter: number;
}

export interface Wavelength {
    value: number;
    weight: number;
    color: RGBA;
}

/** vignetting order: [upper y, lower y, upper x, lower x]; any finite value < 1 (positive trims, negative enlarges, 0 nominal). */
export type Vignetting = [number, number, number, number];

export interface Field {
    x: number;
    y: number;
    weight: number;
    vignetting: Vignetting;
    color: RGBA;
}

export interface Pupil {
    kind: PupilKind;
    value: number;
}

/** Wavelength range is in micrometers; thermal is D0,D1,D2,E0,E1,Ltk,Tref. */
export interface Glass {
    name: string;
    catalog: string;
    formula: number;
    coefficients: number[];
    nd: number;
    vd: number;
    wavelength_range: [number, number] | null;
    thermal: [number, number, number, number, number, number, number] | null;
    transmission: [number, number, number][];
    comment: string;
}

/** One multi-configuration operand overriding a shared base value. Surface indexes are zero-based internal surfaces. */
export type ConfigurationOverride =
    | { kind: 'thickness'; surface: number; value: number }
    | { kind: 'radius'; surface: number; value: number }
    | { kind: 'conic'; surface: number; value: number }
    | { kind: 'aperture'; surface: number; value: number }
    | { kind: 'material'; surface: number; material: string }
    | { kind: 'object_distance'; value: number | null };

export interface Configuration {
    name: string;
    overrides: ConfigurationOverride[];
}

export interface OpticalSystem {
    bench?: import('./bench').Bench | null;
    name: string;
    note: string;
    surfaces: Surface[];
    wavelengths: Wavelength[];
    reference_wavelength: number;
    fields: Field[];
    field_type: FieldType;
    pupil: Pupil;
    /** How pupil coordinates map to rays; part of the saved model, no default when omitted. */
    pupil_mapping: PupilMapping;
    object_distance: number | null;
    /** Material key of the object medium (same keys as surface materials); finite and infinite conjugates. */
    object_material: string;
    object_label: string;
    image_label: string;
    /** Source profile with its vertex at z = -object_distance; must be planar when the object is at infinity. */
    object_surface: EndpointSurface;
    /** Image profile with its vertex at the sum of the internal thicknesses (signed). */
    image_surface: EndpointSurface;
    stop_index: number;
    temperature: number;
    pressure: number;
    custom_glasses: Glass[];
    /** Multi-configuration table; absent or empty means a single unmodified prescription. */
    configurations?: Configuration[];
    /** Zero-based index into configurations; omitted when zero. */
    active_configuration?: number;
}

export interface SystemSnapshot {
    system: OpticalSystem;
    revision: number;
}

// ---- surface structure editing -------------------------------------------------

/** Draft transformation over zero-based internal surface indexes; `before` may equal the surface count (before Image). */
export type SurfaceEditOperation =
    | { kind: 'insert'; before: number; count: number }
    | { kind: 'remove'; first: number; count: number };

/**
 * Read-only result over a supplied draft: the transformed system plus the old→new row map
 * (null for removed rows), the new indexes of inserted rows, and whether removing rows moved the stop.
 */
export interface SurfaceEditResult {
    system: OpticalSystem;
    old_to_new: (number | null)[];
    inserted: number[];
    stop_reassigned: boolean;
}

// ---- workspace recovery ------------------------------------------------------

/** Ready engine state; `revision` is the workspace revision the recovery tools expect. */
export interface ReadyWorkspace {
    state: 'ready';
    revision: number;
    path: string;
}

/**
 * The saved file could not be loaded as a valid system; every optical tool is blocked.
 * `source_text` is the full original text, or null when the file is unreadable or not valid UTF-8.
 * `source_bytes` is null when unreadable and 0 when no file exists.
 */
export interface RecoveryWorkspace {
    state: 'recovery';
    revision: number;
    path: string;
    reason: string;
    source_text: string | null;
    source_bytes: number | null;
}

export type WorkspaceStatus = ReadyWorkspace | RecoveryWorkspace;

export type RecoverySource =
    | { kind: 'default' }
    | { kind: 'project'; format: ImportFormat; text: string };

/** Result of a confirmed recovery replacement; `preserved_path` is null when no original file existed. */
export interface RecoveryResult {
    revision: number;
    preserved_path: string | null;
}

export type Point3 = [number, number, number];

export interface Paraxial {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    /** Null when the system is afocal (no finite focal length). */
    efl: number | null;
    bfl: number | null;
    f_number: number | null;
    matrix: number[];
    /** Null for an afocal system with an object at infinity. */
    image_distance: number | null;
    magnification: number | null;
    entrance_pupil_distance: number;
    entrance_pupil_radius: number;
    stop_radius: number;
    exit_pupil_distance: number | null;
    exit_pupil_radius: number | null;
    image_index: number;
    object_index: number;
    object_na: number | null;
    image_na: number;
    front_focal_distance: number | null;
    /** +1 when light propagates toward +z in image space, -1 after an odd number of centered reflections. */
    image_direction: number;
    /** How the engine's first-order matrix indices are signed (descriptive text). */
    matrix_basis: string;
}

/** Terminal hit on the image surface; no refraction, so the indices before and after are equal. */
export interface Segment {
    point: Point3;
    incoming: Point3;
    direction: Point3;
    normal: Point3;
    n_before: number;
    n_after: number;
    /** Cumulative optical path length. */
    opl: number;
}

export interface Ray {
    /**
     * points[0] is the launch point. Pupil and layout rays launch near the first surface, not at the finite object;
     * an object-mode trace_inspect ray starts on the source profile, so points[0] equals object_point.
     * The remaining entries are the internal surface hits, then the image hit.
     */
    points: Point3[];
    /** Segments at the internal surfaces only; the image hit is `image_hit`. */
    segments: Segment[];
    /** Unit direction of the ray as launched, before the first surface. */
    launch_direction: Point3;
    /** Direction after the last traced interaction. */
    direction: Point3;
    /** Reference offset of the entrance plane; segment and ray OPL are signed from `points[0]`. */
    entrance_opl: number;
    /** Point on the source profile for a finite object; null at infinity. */
    object_point: Point3 | null;
    /** Terminal image-surface hit; null for rays that did not reach it. */
    image_hit: Segment | null;
    field: number;
    field_x: number;
    wavelength: number;
    opl: number;
}

/**
 * Paraxial row. Internal rows lie on the surface vertex planes and the image row on the image vertex plane;
 * `surface_index` is zero-based and null for the endpoints. The object row is the actual source point for a
 * finite chief ray (on the curved profile, not the vertex) and the source vertex for the axial ray.
 */
export interface ParaxialRow {
    endpoint: 'object' | 'surface' | 'image';
    surface_index: number | null;
    point: Point3;
    slope_in: [number, number];
    slope_out: [number, number];
    /** q = direction sign x physical index x slope. */
    reduced_in: [number, number];
    reduced_out: [number, number];
    signed_index_in: number;
    signed_index_out: number;
}

/** Axial and selected-field chief paraxial rays. The object row is absent for an object at infinity. */
export interface ParaxialTrace {
    revision: number;
    wavelength: number;
    field: [number, number];
    entrance_pupil_radius: number;
    axis: ParaxialRow[];
    chief: ParaxialRow[];
    model: string;
    basis: string;
}

export type InspectLaunch =
    | { kind: 'pupil'; pupil: [number, number]; field_index: number }
    | { kind: 'object'; point: [number, number]; tangent: [number, number] };

export interface TraceFailureInfo {
    kind: 'invalid' | 'intersection' | 'blocked' | 'total_internal_reflection' | 'aiming' | 'image';
    /** Zero-based internal surface, null for an endpoint or launch failure. */
    surface: number | null;
    endpoint: 'object' | 'image' | null;
    /** The blocked hit appears only here, never in the ray. */
    point: Point3 | null;
    message: string;
}

export interface TraceInspect {
    revision: number;
    launch: InspectLaunch;
    wavelength: number;
    ray: Ray | null;
    failure: TraceFailureInfo | null;
    coordinates: string;
    optical_path: string;
}

export interface RejectedRay {
    field: number;
    pupil: number;
    error: string;
}

export interface Layout {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    rays: Ray[];
    /** Saved field number of each ray, aligned with `rays`. */
    field_indices: number[];
    rejected: RejectedRay[];
}

/** Pupil ray pattern for geometric analyses; weights are normalized nominal-pupil areas. */
export type PupilSampling = { kind: 'grid'; count: number } | { kind: 'hexapolar'; rings: number };

export type Spectrum = { kind: 'monochromatic'; wavelength: number } | { kind: 'polychromatic' };

/** One positive-weight wavelength; effective_weight = weight * transmitted_fraction (sampled aperture transmission only). */
export interface SpectralComponent {
    wavelength: number;
    weight: number;
    pupil_samples: number;
    transmitted_samples: number;
    transmitted_fraction: number;
    effective_weight: number;
    color: RGBA;
}

export interface ComplexOtf {
    real: number[];
    imaginary: number[];
}

/** Captured when a request starts, so labels never follow live controls. */
export interface RunInfo {
    revision: number;
    field: string;
    wavelength: number;
    /** Saved color of the selected wavelength entry. */
    color: RGBA;
    mapping: PupilMapping;
}

export interface SpotDiagram {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    points: [number, number][];
    /** Nominal pupil area weight of each transmitted point. */
    weights: number[];
    centroid: [number, number];
    rms_radius: number;
    geometric_radius: number;
    blocked: number;
    missed: number;
    total_internal_reflection: number;
    backend: string;
    sampling: PupilSampling;
    sampled_rays: number;
    transmitted_weight: number;
    units: 'mm';
}

/** All stored wavelengths with positive weight; weights are wavelength weight times nominal pupil area weight. */
export interface PolychromaticSpot {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    points: [number, number][];
    weights: number[];
    wavelengths: number[];
    /** Saved RGBA per point, aligned with `points`; duplicate wavelength values can differ. */
    colors: RGBA[];
    centroid: [number, number];
    rms_radius: number;
    transmitted_weight: number;
    sampling: PupilSampling;
    sampled_rays: number;
    failures: {
        wavelength: number;
        weight: number;
        transmitted: number;
        rejected: number;
        transmitted_fraction: number;
        effective_weight: number;
        color: RGBA;
    }[];
    units: string;
}

export type SolveKind = GapSolve['kind'];

/** Installs or replaces the relation of one gap; `{ kind: 'fixed' }` removes it and keeps the resolved thickness. */
export interface SolveArgs {
    gap: number;
    definition: GapSolve;
    revision: number;
}

export interface GapSolveEntry {
    gap: number;
    thickness: number;
    definition: GapSolve;
}

export interface SolveResult {
    revision: number;
    solve: GapSolveEntry;
}

export interface SolveRecalculateResult {
    revision: number;
    /** Active (non-fixed) definitions only. */
    solves: GapSolveEntry[];
}

export interface ApertureSurfaceReport {
    surface: number;
    semi_diameter: number;
    aperture: number;
    clear_aperture: boolean;
    reached_rays: number;
}

/** Object and image records of the aperture report (they have no internal surface index). */
export interface ApertureEndpointReport {
    semi_diameter: number;
    aperture: number;
    clear_aperture: boolean;
    reached_rays: number;
}

export type Endpoint = 'object' | 'image';

export interface ApertureFailure {
    /** Set when the failure happened at an endpoint; `surface` is then null. */
    endpoint: Endpoint | null;
    field_index: number;
    /** Field-local pupil coordinates [p_T, p_S]. */
    pupil: [number, number];
    surface: number | null;
    reached_surfaces: number;
    kind: string;
    error: string;
}

/** Read-only coverage of the derived drawing semi-diameters. Every index is zero-based. */
export interface ApertureReport {
    revision: number;
    wavelength: number;
    sampling: string;
    sampled_rays: number;
    failed_rays: number;
    object: ApertureEndpointReport;
    image: ApertureEndpointReport;
    surfaces: ApertureSurfaceReport[];
    failures: ApertureFailure[];
}

export type VignettingDirection = 'upper_y' | 'lower_y' | 'upper_x' | 'lower_x';

export interface VignettingBoundary {
    direction: VignettingDirection;
    pupil_scale: number;
    /** Null for an endpoint hit (see `limiting_endpoint`) or an unlimited boundary. */
    limiting_surface: number | null;
    limiting_endpoint: Endpoint | null;
    status: 'aperture' | 'unlimited';
}

export interface VignettingFieldResult {
    field_index: number;
    vignetting: Vignetting;
    boundaries: VignettingBoundary[];
}

export interface VignettingUpdateResult {
    revision: number;
    wavelength: number;
    iterations: number;
    tolerance: number;
    fields: VignettingFieldResult[];
}

export interface GeometricMtf {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    frequencies: number[];
    mtf: number[];
    backend: string;
    rays: number;
    spectrum: Spectrum;
    sampling: PupilSampling;
    axis: MtfAxis;
    origin: [number, number];
    components: SpectralComponent[];
    transmitted_weight: number;
    normalization: string;
    units: 'cycles/mm';
    otf: ComplexOtf;
}

export type MtfAxis = 'x' | 'y';

// ---- materials -----------------------------------------------------------

export interface GlassSummary {
    key: string;
    name: string;
    catalog: string;
    nd: number;
    vd: number;
    formula: number;
    wavelength_range: [number, number] | null;
}

/** Non-fatal catalog diagnostic; `line` is 1-based in the source catalog text. */
export interface CatalogWarning {
    catalog: string;
    glass: string;
    line: number;
    message: string;
}

export interface MaterialSearchResult {
    total: number;
    glasses: GlassSummary[];
    /** Diagnostics from loading the source catalogs; always present, possibly empty. */
    warnings: CatalogWarning[];
}

export interface CatalogImportResult {
    revision: number;
    glasses_imported: number;
    warnings: CatalogWarning[];
}

// ---- analysis results ----------------------------------------------------

/** Unit axes in global image/pupil x,y; +y on-axis is tangential. */
export interface FieldFrame {
    tangential: [number, number];
    sagittal: [number, number];
}

export interface RayFanPoint {
    pupil: number;
    tangential_error: number | null;
    sagittal_error: number | null;
    error: string | null;
}
export interface RayFan {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    frame: FieldFrame;
    tangential: RayFanPoint[];
    sagittal: RayFanPoint[];
    chief_image: Point3;
    wavelength: number;
    units: string;
}

export interface OpdPoint {
    pupil: number;
    opd: number | null;
    error: string | null;
}
export interface OpdFan {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    frame: FieldFrame;
    reference: string;
    tangential: OpdPoint[];
    sagittal: OpdPoint[];
    units: string;
    wavelength: number;
}

export interface ChromaticPoint {
    wavelength: number;
    focus: number | null;
    shift: number | null;
    error: string | null;
}
export interface ChromaticFocus {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    reference_wavelength: number;
    reference_focus: number;
    points: ChromaticPoint[];
    units: string;
}

export interface LongitudinalPoint {
    pupil: number;
    focus: number | null;
    shift: number | null;
    error: string | null;
}
export interface LongitudinalAberration {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    wavelength: number;
    paraxial_focus: number;
    points: LongitudinalPoint[];
    units: string;
}

export interface Astigmatism {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    frame: FieldFrame;
    sagittal_focus: number;
    tangential_focus: number;
    sagittal_shift: number;
    tangential_shift: number;
    astigmatism: number;
    chief_direction: Point3;
    units: string;
}

export interface Distortion {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    /** Definition of the ideal point (paraxial chief-ray intercept at the stored image plane). */
    reference: string;
    ideal: [number, number];
    actual: [number, number];
    distortion_percent: number | null;
    units: string;
}

export interface Wavefront {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    grid: number;
    opd: (number | null)[];
    pupils: [number, number][];
    rms: number;
    peak_to_valley: number;
    mean: number;
    wavelength: number;
    units: string;
    valid: number;
    failed: number;
    reference: string;
}

export interface DiffractionPsf {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    intensity: number[];
    grid: number;
    pixel_size: number;
    strehl: number;
    wavelength: number;
    normalization: string;
    model: string;
    failed_pupil_rays: number;
    units: string;
    /** How the image surface enters the computation, as reported by the engine. */
    image_sampling: string;
}

export interface DiffractionMtf {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    frame: FieldFrame;
    frequencies: number[];
    sagittal: number[];
    tangential: number[];
    units: string;
    failed_pupil_rays: number;
    model: string;
    spectrum: Spectrum;
    components: (SpectralComponent & {
        pixel_size: number;
        sampling_cutoff: number;
        chief_image: [number, number];
    })[];
    origin: [number, number];
    transmitted_weight: number;
    normalization: string;
    pupil_grid: number;
    sampling_limit: number;
    max_frequency: number;
    /** How the image surface enters the computation, as reported by the engine. */
    image_sampling: string;
    sagittal_otf: ComplexOtf;
    tangential_otf: ComplexOtf;
}

export interface EncircledEnergy {
    /** Saved-model revision the engine held while computing this reply. */
    revision: number;
    center: [number, number];
    points: { radius: number; fraction: number }[];
    rays: number;
    sampling: PupilSampling;
    sampled_rays: number;
    transmitted_weight: number;
    normalization: string;
    units: string;
}

/** Diagnostic over every stored wavelength, zero weights included; not a weighted spectrum. */
export interface Spherochromatism {
    revision: number;
    curves: (LongitudinalAberration & { color: RGBA; weight: number; wavelength_index: number })[];
    reference_focus: number;
    reference_wavelength: number;
    units: 'mm';
}

/** Why one focus value is missing. `surface` is zero-based; endpoint names replace it for object or image failures. */
export interface FocusFailure {
    stage: 'chief' | 'bundle' | 'focus';
    kind: string;
    surface: number | null;
    endpoint: 'object' | 'image' | null;
    point: Point3 | null;
    message: string;
}

/** Global z of the focus and its shift from the stored image vertex; both null when the value failed. */
export interface FocusValue {
    focus_z: number | null;
    shift: number | null;
    error: FocusFailure | null;
}

export interface FieldCurvaturePoint {
    /** Position along the sweep, 0 at the axis and 1 at `field_end`. */
    fraction: number;
    field: [number, number];
    radius: number;
    chief_image: Point3 | null;
    frame: FieldFrame;
    sagittal: FocusValue;
    tangential: FocusValue;
}

export interface FieldCurvatureCurve {
    wavelength_index: number;
    wavelength: number;
    weight: number;
    color: RGBA;
    points: FieldCurvaturePoint[];
}

/** Radial sweep of sagittal and tangential focus for every stored wavelength, zero weights and duplicates included. */
export interface FieldCurvature {
    revision: number;
    field_end: [number, number];
    max_field: number;
    samples: number;
    field_type: 'angle' | 'object_height' | 'image_height';
    field_units: 'degrees' | 'mm';
    image_vertex: number;
    curves: FieldCurvatureCurve[];
    units: 'mm';
    model: string;
}

/** Huygens PSF shares the FFT PSF shape and adds the pupil lattice size. */
export interface HuygensPsf extends DiffractionPsf {
    pupil_grid: number;
}

export interface ZernikeTerm {
    j: number;
    n: number;
    m: number;
    name: string;
    /** Waves; unit-RMS-over-disk normalization. */
    coefficient: number;
}

export interface ZernikeFit {
    revision: number;
    terms: ZernikeTerm[];
    fit_residual_rms: number;
    valid: number;
    grid: number;
    wavelength: number;
    units: string;
    ordering: string;
    normalization: string;
}

export interface GaussianBeamSummary {
    /** 1/e² radius at the plane. */
    radius: number;
    /** Wavefront curvature radius; null when flat. */
    curvature: number | null;
    waist: number;
    /** Waist position relative to the plane, positive downstream. */
    waist_offset: number;
    rayleigh_range: number;
    divergence: number;
}

export interface GaussianBeam {
    revision: number;
    wavelength: number;
    input: { waist: number; waist_z: number; rayleigh_range: number };
    surfaces: { surface: number; z: number; radius: number; curvature: number | null }[];
    image: GaussianBeamSummary;
    units: string;
    model: string;
}

export interface FiberCoupling {
    revision: number;
    wavelength: number;
    mode_field_diameter: number;
    beam_radius: number;
    wavefront_curvature: number | null;
    efficiency: number;
    optimal_mode_field_diameter: number;
    optimal_efficiency: number;
    units: string;
    model: string;
}

export interface AnalysisResults {
    ray_fan: RayFan;
    opd_fan: OpdFan;
    chromatic_focus: ChromaticFocus;
    longitudinal_aberration: LongitudinalAberration;
    spherochromatism: Spherochromatism;
    field_curvature: FieldCurvature;
    astigmatism: Astigmatism;
    distortion: Distortion;
    wavefront: Wavefront;
    diffraction_psf: DiffractionPsf;
    diffraction_mtf: DiffractionMtf;
    encircled_energy: EncircledEnergy;
    wavefront_zernike: ZernikeFit;
    huygens_psf: HuygensPsf;
    huygens_mtf: DiffractionMtf;
    gaussian_beam: GaussianBeam;
    fiber_coupling: FiberCoupling;
}
export type AnalysisKind = keyof AnalysisResults;
/** A result tagged with the analysis that produced it, for narrowing in templates. */
export type AnalysisOutcome = {
    [K in AnalysisKind]: { kind: K; data: AnalysisResults[K] };
}[AnalysisKind];

// ---- prescription report ---------------------------------------------------

/** Saved-prescription report sections; the backend emits them in this canonical order regardless of request order. */
export type SectionId = 'general' | 'specification' | 'first_order' | 'lens_data' | 'surface_data';

/** Display text is the backend's canonical formatting; `value` keeps full precision for number cells. */
export type ReportCell =
    | { kind: 'number'; value: number; text: string }
    | { kind: 'text'; text: string }
    | { kind: 'infinite'; sign: -1 | 1; text: string }
    | { kind: 'not_applicable'; reason: string; text: string }
    | { kind: 'unavailable'; reason: string; text: string };

export interface ReportColumn {
    id: string;
    label: string;
    unit: string | null;
}

/** `cells` aligns with the table's columns; the first cell is the row header. */
export interface ReportRow {
    id: string;
    cells: ReportCell[];
}

export interface ReportTable {
    id: string;
    title: string;
    columns: ReportColumn[];
    rows: ReportRow[];
}

export interface ReportSection {
    id: SectionId;
    title: string;
    paragraphs: string[];
    tables: ReportTable[];
}

export interface ReportDiagnostic {
    section: SectionId;
    table: string;
    row: string;
    column: string;
    message: string;
}

/** Read-only report assembly over the saved system; `text` is the canonical download payload. */
export interface PrescriptionReport {
    revision: number;
    name: string;
    wavelength: number;
    precision: number;
    sections: ReportSection[];
    diagnostics: ReportDiagnostic[];
    text: string;
}

export type ImportFormat = 'geopter' | 'hissabeth' | 'zmx';

export interface AgentInfo {
    name: string;
    installed: boolean;
    executable: string | null;
}

export interface AgentRegistration {
    name: string;
    success?: boolean;
    message?: string;
    status?: string;
}

// ---- optimization, tolerancing, history, projects --------------------------

export type ConstraintMode = 'eq' | 'lt' | 'gt';

export type OptimizeVariable =
    | { kind: 'radius'; surface: number; min?: number | null; max?: number | null }
    | { kind: 'thickness'; surface: number; min?: number | null; max?: number | null }
    | { kind: 'conic'; surface: number; min?: number | null; max?: number | null }
    | {
          kind: 'even_asphere';
          surface: number;
          index: number;
          min?: number | null;
          max?: number | null;
      }
    | {
          kind: 'odd_asphere';
          surface: number;
          index: number;
          min?: number | null;
          max?: number | null;
      }
    | { kind: 'model_glass_nd'; surface: number; min?: number | null; max?: number | null }
    | { kind: 'model_glass_vd'; surface: number; min?: number | null; max?: number | null }
    | {
          kind: 'decenter';
          surface: number;
          axis: 'x' | 'y';
          min?: number | null;
          max?: number | null;
      }
    | {
          kind: 'tilt';
          surface: number;
          axis: 'x' | 'y' | 'z';
          min?: number | null;
          max?: number | null;
      }
    | {
          kind: 'config_thickness';
          configuration: number;
          surface: number;
          min?: number | null;
          max?: number | null;
      }
    | {
          kind: 'config_radius';
          configuration: number;
          surface: number;
          min?: number | null;
          max?: number | null;
      };

export type OptimizeOperand =
    | {
          kind: 'rms_spot';
          field_index?: number | null;
          wavelength?: number | null;
          sampling?: PupilSampling | null;
          target?: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'rms_wavefront';
          field_index?: number;
          wavelength?: number | null;
          grid?: number | null;
          target?: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'efl' | 'bfl' | 'working_f_number';
          wavelength?: number | null;
          target: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'total_track';
          first?: number | null;
          last?: number | null;
          target: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'center_thickness';
          surface: number;
          target: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'edge_thickness';
          surface: number;
          height?: number | null;
          target: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'distortion' | 'chief_ray_angle';
          field_index: number;
          wavelength?: number | null;
          target?: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'axial_color';
          wave1: number;
          wave2: number;
          target?: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'lateral_color';
          field_index: number;
          wave1: number;
          wave2: number;
          target?: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      }
    | {
          kind: 'ray_intercept';
          field_index: number;
          px: number;
          py: number;
          wavelength?: number | null;
          axis: 'x' | 'y';
          target?: number;
          weight?: number;
          mode?: ConstraintMode;
          configuration?: number | null;
      };

export interface OptimizeResult {
    revision: number;
    applied: boolean;
    system: OpticalSystem;
    initial_merit: number;
    final_merit: number;
    iterations: number;
    converged: boolean;
    reason: string;
    variables: { index: number; label: string; before: number; after: number }[];
    operands: {
        index: number;
        label: string;
        target: number;
        weight: number;
        mode: ConstraintMode;
        before: number;
        after: number;
        contribution_after: number;
    }[];
    history: { iteration: number; merit: number; damping: number; accepted: boolean }[];
}

export type ToleranceOperand =
    | { kind: 'radius'; surface: number; delta_percent?: number | null; delta_mm?: number | null }
    | { kind: 'thickness'; surface: number; delta: number }
    | { kind: 'decenter'; surface: number; delta: number }
    | { kind: 'tilt'; surface: number; delta_deg: number }
    | { kind: 'index'; surface: number; delta: number }
    | { kind: 'abbe'; surface: number; delta_percent: number }
    | { kind: 'element_decenter'; first: number; last: number; delta: number }
    | { kind: 'element_tilt'; first: number; last: number; delta_deg: number };

export type ToleranceCompensator =
    | { kind: 'none' }
    | { kind: 'autofocus'; wavelength?: number | null; max_delta?: number | null };

export type ToleranceCriterion =
    | {
          kind: 'rms_spot';
          field_index?: number | null;
          wavelength?: number | null;
          sampling?: PupilSampling | null;
      }
    | {
          kind: 'rms_wavefront';
          field_index?: number;
          wavelength?: number | null;
          grid?: number | null;
      }
    | { kind: 'efl'; wavelength?: number | null }
    | {
          kind: 'geometric_mtf';
          field_index?: number;
          frequency: number;
          axis?: 'x' | 'y' | null;
          wavelength?: number | null;
          sampling?: PupilSampling | null;
      };

export interface SensitivityEntry {
    operand_index: number;
    label: string;
    axis: string;
    plus_value: number | null;
    minus_value: number | null;
    plus_change: number | null;
    minus_change: number | null;
    worst_change: number;
    plus_error: string | null;
    minus_error: string | null;
}

export interface ToleranceSensitivityResult {
    revision: number;
    nominal: number;
    criterion_unit: string;
    entries: SensitivityEntry[];
    rss_change: number;
    estimated_performance: number;
}

export interface ToleranceMonteCarloResult {
    revision: number;
    nominal: number;
    criterion_unit: string;
    trials: number;
    succeeded: number;
    failed: number;
    mean: number | null;
    std_dev: number | null;
    min: number | null;
    max: number | null;
    p50: number | null;
    p80: number | null;
    p90: number | null;
    p95: number | null;
    p98: number | null;
    pass_rate: number | null;
    compensator_range: [number, number] | null;
    histogram: { low: number; high: number; count: number }[];
}

export interface HistoryEntrySummary {
    index: number;
    revision: number;
    timestamp_ms: number;
    action: string;
    name: string;
    surfaces: number;
    current: boolean;
}

export interface HistoryList {
    current_revision: number;
    cursor: number;
    can_undo: boolean;
    can_redo: boolean;
    entries: HistoryEntrySummary[];
}

export interface HistoryMetrics {
    name: string;
    surfaces: number;
    total_track: number;
    efl: number | null;
    bfl: number | null;
    f_number: number | null;
    rms_spot_mm: (number | null)[];
}

export interface HistoryCompareResult {
    from: {
        index: number;
        revision: number;
        action: string;
        timestamp_ms: number;
        metrics: HistoryMetrics;
    };
    to: {
        index: number;
        revision: number;
        action: string;
        timestamp_ms: number;
        metrics: HistoryMetrics;
    };
    deltas: {
        efl: number | null;
        bfl: number | null;
        f_number: number | null;
        total_track: number | null;
        rms_spot_mm: (number | null)[];
    };
    surface_changes: {
        surface: number;
        status: 'modified' | 'added' | 'removed';
        label?: string;
        changes?: { field: string; before: unknown; after: unknown }[];
    }[];
}

export interface ProjectListResult {
    active_file: string | null;
    workspace_file: string;
    projects_dir: string;
    saved_projects: string[];
    recent: string[];
}

export const TABS = [
    'Layout',
    'Spot diagram',
    'MTF',
    'Analysis',
    'Ray inspection',
    'Edit',
    'Optimize',
    'Tolerance',
    'Configurations',
    'Prescription',
    'Prescription report',
    'Materials',
    'History',
    'Import / Export',
    'Agent connections',
] as const;
export type Tab = (typeof TABS)[number];
