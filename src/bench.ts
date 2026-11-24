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
    rotation: Vec3;
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
export interface BenchSource {
    id: string;
    position: Vec3;
    direction: Vec3;
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
export interface Bench {
    name: string;
    table_size: [number, number];
    hole_pitch: number;
    origin: Vec3;
    elements: BenchElement[];
    sources: BenchSource[];
    connections: BenchConnection[];
    max_interactions: number;
    min_power: number;
}
export interface BenchSegment {
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
export interface BenchTrace {
    revision: number;
    saved: boolean;
    segments: BenchSegment[];
    terminations: {
        source: string;
        branch: string;
        element: string | null;
        position: Vec3;
        power: number;
        reason: string;
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
export interface BenchLabCheck {
    revision?: number;
    readiness_score: number;
    passed: boolean;
    counts: { errors: number; warnings: number; info: number };
    issues: BenchLabIssue[];
    gaussian: { segments: BenchGaussianSegment[]; hits: BenchGaussianHit[] };
    recommended_beam_dumps: { branch: string; position: Vec3; direction: Vec3; power: number }[];
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
        const b = value as Bench;
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
                typeof e.label !== 'string' ||
                !vector(e.rotation) ||
                ![e.aperture, e.body_radius, e.focal_length, e.reflectance, e.transmittance].every(
                    Number.isFinite,
                ) ||
                e.aperture <= 0 ||
                e.body_radius < e.aperture ||
                e.reflectance < 0 ||
                e.transmittance < 0 ||
                e.reflectance + e.transmittance > 1 ||
                (e.kind === 'lens' && Math.abs(e.focal_length) < 1e-6)
            )
                throw Error();
            if (
                e.mount !== null &&
                (!e.mount ||
                    typeof e.mount.part !== 'string' ||
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
        return [];
    } catch {
        return [tr('bench.invalid')];
    }
}
