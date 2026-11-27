import { tr } from './i18n';
export type Vec3 = [number, number, number];
export const BENCH_KINDS = [
    'mirror',
    'curved_mirror',
    'lens',
    'lens_group',
    'splitter',
    'pbs',
    'waveplate',
    'polarizer',
    'detector',
    'aperture',
    'blocker',
] as const;
export type BenchKind = (typeof BENCH_KINDS)[number];
export type BenchPolarization =
    | { kind: 'unpolarized' }
    | { kind: 'horizontal' }
    | { kind: 'vertical' }
    | { kind: 'diagonal45' }
    | { kind: 'right_circular' }
    | { kind: 'left_circular' }
    | { kind: 'linear'; angle_deg: number };
export interface BenchMount {
    part: string;
    base_size: [number, number];
    height_range: [number, number];
}
export interface BenchCoating {
    layers: { index: number; thickness: number }[];
    substrate_index: number;
}
export type BenchScatter =
    | { model: 'lambertian'; fraction: number; rays: number }
    | { model: 'abg'; a: number; b: number; g: number; rays: number };
export interface BenchElement {
    id: string;
    label: string;
    kind: BenchKind;
    position: Vec3;
    /** Angles [roll, pitch, yaw] in degrees. Null leaves the rotation unresolved. */
    rotation: Vec3 | null;
    /** Allow auto-aim to adjust numeric angles. Stage rotations are fixed. */
    aim_rotation?: boolean;
    aperture: number;
    body_radius: number;
    focal_length: number;
    reflectance: number;
    transmittance: number;
    mount: BenchMount | null;
    /** Splitter substrate index: Fresnel splitting replaces scalar R/T. */
    index?: number | null;
    /** Thin-film stack on a mirror or splitter face, replacing scalar R/T. */
    coating?: BenchCoating | null;
    /** Surface scatter on mirrors, splitters and thin lenses. */
    scatter?: BenchScatter | null;
    /** Lens group internal Fresnel ghost bounces (0-2). */
    ghosts?: number;
    /** Detector irradiance map resolution [columns, rows]. */
    pixels?: [number, number] | null;
    /** Waveplate retardance in waves (0.25 = QWP, 0.5 = HWP). */
    retardance_waves?: number | null;
    /** Waveplate fast axis or polarizer transmission axis (deg). */
    fast_axis_deg?: number | null;
    /** Extinction ratio for PBS / polarizer (default 1000). */
    extinction_ratio?: number | null;
    /** Kinematic mount screw pitch (turns per inch, e.g. 100). */
    screw_tpi?: number | null;
    /** Kinematic mount pivot lever arm (mm, e.g. 25.4). */
    lever_arm_mm?: number | null;
}
type DefaultedElementFields =
    | 'label'
    | 'rotation'
    | 'aperture'
    | 'body_radius'
    | 'focal_length'
    | 'reflectance'
    | 'transmittance'
    | 'mount';
export type BenchElementInput = Omit<BenchElement, DefaultedElementFields> & {
    [Field in DefaultedElementFields]?: BenchElement[Field] | null;
};
export interface BenchSource {
    id: string;
    position: Vec3;
    direction: Vec3;
    /** Explicitly permit auto-aim to adjust this launch direction. */
    aim_direction?: boolean;
    wavelength: number;
    power: number;
    beam_radius: number;
    rings: number;
    waist?: number | null;
    waist_offset?: number | null;
    m2?: number | null;
    polarization?: BenchPolarization | null;
}
export interface BenchConnection {
    from: string;
    port: string;
    to: string;
}
export interface BenchStagePose {
    position: Vec3;
    rotation: Vec3;
}
export interface BenchStage {
    id: string;
    members: string[];
    reference_pose: Record<string, BenchStagePose>;
    /** Unit direction in table X, Y and height coordinates. */
    axis: Vec3;
    travel: [number, number];
    position_mm: number;
    pzt?: { range_um: [number, number]; value_um: number } | null;
}
export interface BenchPort {
    element: string;
    port: string;
}
export interface BenchBranch {
    id: string;
    /** Output at which measurement begins. */
    from: BenchPort;
    /** Named input at which measurement ends. "in" is the generic input. */
    to: BenchPort;
    /** Ordered output selections at intermediate elements with multiple nominal continuations. */
    via?: BenchPort[];
}
export type BenchAssertionUnit = 'mm' | 'um' | 'mrad';
export interface BenchAssertion {
    id: string;
    kind: 'equal_opl' | 'colinear';
    /** Two distinct branch IDs. Equal OPL takes the first branch minus the second. */
    branches: [string, string];
    target: number;
    unit: BenchAssertionUnit;
    tolerance: number;
}
export interface Bench {
    name: string;
    table_size: [number, number];
    hole_pitch: number;
    origin: Vec3;
    elements: BenchElement[];
    sources: BenchSource[];
    connections: BenchConnection[];
    stages?: BenchStage[];
    branches?: BenchBranch[];
    assertions?: BenchAssertion[];
    max_interactions: number;
    min_power: number;
}
export type BenchInput = Omit<Bench, 'elements'> & { elements: BenchElementInput[] };

export interface BenchFlight {
    from_element: string;
    from_port: string;
    to_element: string;
    to_port: string;
    length_mm: number;
    medium: string;
    index: number;
    opl_mm: number;
    wavelength_nm: number;
    previous_segment: number | null;
}
/** Display segments carry flight fields when they connect real endpoints. */
export interface BenchSegment extends Partial<BenchFlight> {
    source: string;
    branch: string;
    from: Vec3;
    to: Vec3;
    power: number;
    element: string | null;
    status: string;
}
export interface BenchDetectorMap {
    id: string;
    bins: [number, number];
    /** Clear half-width (mm); the map spans the inscribed square. */
    extent: number;
    /** Row-major bin power: rows are local height, columns local transverse. */
    power: number[];
    total: number;
    /** Power-weighted centroid and RMS in local (transverse, height) mm. */
    centroid: [number, number];
    rms: [number, number];
}
export interface BenchPathBreak extends BenchPort {
    reason: string;
}
export interface BenchBranchReport {
    id: string;
    paths: {
        source: string;
        wavelength_nm: number;
        start_segment: number | null;
        length_mm: number;
        opl_mm: number;
        ports: BenchPort[];
        nominal_match: boolean;
        break?: BenchPathBreak;
    }[];
    break?: BenchPathBreak;
}
export interface BenchTrace {
    revision: number;
    saved: boolean;
    segments: BenchSegment[];
    branches: BenchBranchReport[];
    stage_poses?: Record<string, BenchStagePose>;
    terminations: {
        source: string;
        branch: string;
        element: string | null;
        position: Vec3;
        power: number;
        reason: string;
        /** Index of the last real flight in segments. */
        last_segment: number | null;
    }[];
    power: Record<
        'launched' | 'detected' | 'absorbed' | 'blocked' | 'escaped' | 'truncated',
        number
    >;
    detectors: BenchDetectorMap[];
    balance_error: number;
    warnings: string[];
}
export interface BenchAssembly {
    revision: number;
    saved: boolean;
    name: string;
    warnings: string[];
    items: {
        id: string;
        label: string;
        kind: BenchKind;
        position: Vec3;
        rotation: Vec3;
        normal: Vec3;
        hole: [number, number];
        hole_offset: [number, number];
        mount: BenchMount | null;
        lens_model: string | null;
    }[];
    sources: {
        id: string;
        position: Vec3;
        direction: Vec3;
        wavelength: number;
        power: number;
        mount: null;
    }[];
    bom: [string, number][];
    connections: (BenchConnection & { distance_mm: number })[];
}
export interface BenchCatalogItem {
    part: string;
    name: string;
    category: string;
    kind: BenchKind;
    aperture: number;
    body_radius: number;
    focal_length: number;
    reflectance: number;
    transmittance: number;
    retardance_waves?: number | null;
    fast_axis_deg?: number | null;
    extinction_ratio?: number | null;
    screw_tpi?: number | null;
    lever_arm_mm?: number | null;
    clamping_style: 'direct_hole' | 'clamping_fork' | 'slotted_base';
    fork_reach_mm: number;
    mount: BenchMount;
    typical_price_usd: number;
    notes: string;
}
export interface BenchGaussianSegment {
    source: string;
    branch: string;
    from: Vec3;
    to: Vec3;
    w_from: number;
    w_to: number;
}
export interface BenchGaussianHit {
    element_id: string;
    branch: string;
    beam_radius_mm: number;
    beam_diameter_mm: number;
    aperture_radius_mm: number;
    clearance_ratio: number;
    status: 'ok' | 'tight' | 'clipped';
}
export interface BenchLabIssue {
    severity: 'error' | 'warning' | 'info';
    code: string;
    element_id?: string | null;
    branch?: string | null;
    message: string;
}
export interface BenchAssertionResult {
    assertion: string;
    kind: BenchAssertion['kind'];
    branches: [string, string];
    /** Input endpoint of the second arm, at the comparison element. */
    at: BenchPort;
    value: number | BenchSeparation | null;
    unit: BenchAssertionUnit;
    target: number;
    tolerance: number;
    residual: number | null;
    status: 'pass' | 'fail' | 'unreachable';
    break?: BenchPathBreak;
}
export interface BenchSeparation {
    position_mm: number;
    angle_mrad: number;
}
export interface BenchAimResidual {
    branch: string;
    position_mm: number | null;
    angle_mrad: number | null;
    break?: BenchPathBreak;
}
export interface BenchLabCheck {
    revision: number;
    saved: boolean;
    readiness_score: number;
    passed: boolean;
    counts: { errors: number; warnings: number; info: number };
    issues: BenchLabIssue[];
    stages: { id: string; position_mm: number; pzt_um: number | null }[];
    branches: {
        id: string;
        stage_ids: string[];
        color: string | null;
        paths: {
            source: string;
            wavelength_nm: number;
            /** Accumulated only up to the interruption for an incomplete path. */
            opl_mm: number;
            break?: BenchPathBreak;
        }[];
        break?: BenchPathBreak;
    }[];
    assertions: BenchAssertionResult[];
    gaussian: { segments: BenchGaussianSegment[]; hits: BenchGaussianHit[]; units: 'mm' };
    recommended_beam_dumps: {
        branch: string;
        source: string;
        position: Vec3;
        direction: Vec3;
        power: number;
    }[];
    clamping: {
        element_id: string;
        hole: [number, number];
        offset_mm: number;
        style: 'direct_hole' | 'clamping_fork' | 'unreachable';
        clamping_hole?: [number, number] | null;
        clamping_offset_mm?: number | null;
    }[];
}
export interface BenchAlignmentGuide {
    revision?: number;
    steps: {
        step: number;
        id: string;
        kind: string;
        label: string;
        position: Vec3;
        hole: [number, number];
        hole_offset: number;
        mount_part: string | null;
        beam_height_z: number;
        expected_beam_radius_mm: number | null;
        target_next: string | null;
        instruction: string;
    }[];
    sensitivities: {
        element_id: string;
        screw_tpi: number;
        lever_arm_mm: number;
        mrad_per_quarter_turn: number;
        targets: {
            target_id: string;
            distance_mm: number;
            spot_shift_mm_per_quarter_turn: number;
        }[];
    }[];
    safety_notes: string[];
}
export interface BenchBlueprintExport {
    revision: number;
    saved: boolean;
    format: 'svg' | 'dxf' | 'stl' | 'alignment_md';
    filename: string;
    mime: string;
    content: string;
}
/** Rotation order matches Rust: Rz(yaw) Ry(pitch) Rx(roll). */
export function benchRotate(v: Vec3, a: Vec3): Vec3 {
    const [r, p, y] = a.map((x) => (x * Math.PI) / 180) as Vec3;
    const b: Vec3 = [
        v[0],
        Math.cos(r) * v[1] - Math.sin(r) * v[2],
        Math.sin(r) * v[1] + Math.cos(r) * v[2],
    ];
    const c: Vec3 = [
        Math.cos(p) * b[0] + Math.sin(p) * b[2],
        b[1],
        -Math.sin(p) * b[0] + Math.cos(p) * b[2],
    ];
    return [Math.cos(y) * c[0] - Math.sin(y) * c[1], Math.sin(y) * c[0] + Math.cos(y) * c[1], c[2]];
}
export function benchPorts(kind: BenchKind | 'source'): string[] {
    return kind === 'source' ||
        kind === 'lens' ||
        kind === 'lens_group' ||
        kind === 'aperture' ||
        kind === 'waveplate' ||
        kind === 'polarizer'
        ? ['out']
        : kind === 'mirror' || kind === 'curved_mirror'
          ? ['reflected']
          : kind === 'splitter' || kind === 'pbs'
            ? ['reflected', 'transmitted']
            : [];
}
const vector = (v: unknown, n = 3): boolean =>
    Array.isArray(v) &&
    v.length === n &&
    v.every((x) => typeof x === 'number' && Number.isFinite(x));

/** Structural input guard. The engine owns geometric and graph validation. */
export function benchProblems(value: unknown): string[] {
    if (value == null) return [];
    try {
        const b = value as BenchInput;
        if (
            typeof b.name !== 'string' ||
            !vector(b.table_size, 2) ||
            !vector(b.origin) ||
            !Number.isFinite(b.hole_pitch) ||
            b.hole_pitch <= 0 ||
            !Number.isInteger(b.max_interactions) ||
            b.max_interactions < 1 ||
            b.max_interactions > 128 ||
            !Number.isFinite(b.min_power) ||
            b.min_power < 0 ||
            b.min_power > 1 ||
            !Array.isArray(b.elements) ||
            b.elements.length > 128 ||
            !Array.isArray(b.sources) ||
            !b.sources.length ||
            b.sources.length > 16 ||
            !Array.isArray(b.connections) ||
            b.connections.length > 384
        )
            throw Error();
        const ids = new Set<string>();
        for (const e of [...b.elements, ...b.sources]) {
            if (typeof e.id !== 'string' || !e.id || ids.has(e.id) || !vector(e.position))
                throw Error();
            ids.add(e.id);
        }
        for (const e of b.elements) {
            if (
                !BENCH_KINDS.includes(e.kind) ||
                (e.label != null && typeof e.label !== 'string') ||
                (e.rotation != null && !vector(e.rotation)) ||
                (e.aim_rotation !== undefined && typeof e.aim_rotation !== 'boolean') ||
                ![e.aperture, e.body_radius, e.focal_length, e.reflectance, e.transmittance].every(
                    (v) => v == null || Number.isFinite(v),
                ) ||
                (e.aperture != null && e.aperture <= 0) ||
                (e.body_radius != null && e.body_radius < (e.aperture ?? 0)) ||
                (e.reflectance != null && e.reflectance < 0) ||
                (e.transmittance != null && e.transmittance < 0) ||
                (e.reflectance ?? 0) + (e.transmittance ?? 0) > 1 ||
                ((e.kind === 'lens' || e.kind === 'curved_mirror') &&
                    e.focal_length != null &&
                    Math.abs(e.focal_length) < 1e-6)
            )
                throw Error();
            if (
                e.mount != null &&
                (typeof e.mount.part !== 'string' ||
                    !vector(e.mount.base_size, 2) ||
                    !vector(e.mount.height_range, 2))
            )
                throw Error();
            if (
                (e.index != null && !Number.isFinite(e.index)) ||
                (e.ghosts !== undefined &&
                    (!Number.isInteger(e.ghosts) || e.ghosts < 0 || e.ghosts > 2)) ||
                (e.pixels != null &&
                    (!Array.isArray(e.pixels) ||
                        e.pixels.length !== 2 ||
                        !e.pixels.every((p) => Number.isInteger(p) && p >= 2 && p <= 128))) ||
                (e.coating != null &&
                    (!Array.isArray(e.coating.layers) ||
                        e.coating.layers.length > 32 ||
                        !Number.isFinite(e.coating.substrate_index) ||
                        e.coating.layers.some(
                            (l) => !l || !Number.isFinite(l.index) || !Number.isFinite(l.thickness),
                        ))) ||
                (e.scatter != null &&
                    (typeof e.scatter !== 'object' ||
                        (e.scatter.model !== 'lambertian' && e.scatter.model !== 'abg') ||
                        !Number.isInteger(e.scatter.rays) ||
                        e.scatter.rays < 1 ||
                        e.scatter.rays > 64))
            )
                throw Error();
        }
        for (const s of b.sources)
            if (
                !vector(s.direction) ||
                (s.aim_direction !== undefined && typeof s.aim_direction !== 'boolean') ||
                Math.hypot(...s.direction) < 1e-15 ||
                ![s.wavelength, s.power, s.beam_radius].every(Number.isFinite) ||
                s.power <= 0 ||
                s.beam_radius < 0 ||
                !Number.isInteger(s.rings) ||
                s.rings < 0 ||
                s.rings > 8
            )
                throw Error();
        for (const c of b.connections)
            if (
                typeof c.from !== 'string' ||
                typeof c.to !== 'string' ||
                typeof c.port !== 'string'
            )
                throw Error();
        if (
            b.stages !== undefined &&
            (!Array.isArray(b.stages) ||
                b.stages.length > 128 ||
                b.stages.some(
                    (stage) =>
                        typeof stage.id !== 'string' ||
                        !vector(stage.axis) ||
                        !vector(stage.travel, 2) ||
                        !Number.isFinite(stage.position_mm) ||
                        !Array.isArray(stage.members) ||
                        stage.members.length < 1 ||
                        stage.members.length > 128 ||
                        stage.members.some((member) => typeof member !== 'string') ||
                        !stage.reference_pose ||
                        typeof stage.reference_pose !== 'object' ||
                        Array.isArray(stage.reference_pose) ||
                        Object.values(stage.reference_pose).some(
                            (pose) => !vector(pose.position) || !vector(pose.rotation),
                        ) ||
                        (stage.pzt != null &&
                            (!vector(stage.pzt.range_um, 2) ||
                                !Number.isFinite(stage.pzt.value_um))),
                ))
        )
            throw Error();
        const isPort = (p: BenchPort): boolean =>
            typeof p.element === 'string' && typeof p.port === 'string';
        if (
            b.branches !== undefined &&
            (!Array.isArray(b.branches) ||
                b.branches.some(
                    (branch) =>
                        typeof branch.id !== 'string' ||
                        !isPort(branch.from) ||
                        !isPort(branch.to) ||
                        (branch.via !== undefined &&
                            (!Array.isArray(branch.via) || !branch.via.every(isPort))),
                ))
        )
            throw Error();
        if (
            b.assertions !== undefined &&
            (!Array.isArray(b.assertions) ||
                b.assertions.some(
                    (a) =>
                        typeof a.id !== 'string' ||
                        !['equal_opl', 'colinear'].includes(a.kind) ||
                        !['mm', 'um', 'mrad'].includes(a.unit) ||
                        !Number.isFinite(a.target) ||
                        !Number.isFinite(a.tolerance) ||
                        a.tolerance < 0 ||
                        !Array.isArray(a.branches) ||
                        a.branches.length !== 2 ||
                        a.branches.some((branch) => typeof branch !== 'string'),
                ))
        )
            throw Error();
        return [];
    } catch {
        return [tr('bench.invalid')];
    }
}
