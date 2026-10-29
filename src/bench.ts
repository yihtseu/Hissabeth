import { tr } from './i18n';
export type Vec3 = [number, number, number];
export const BENCH_KINDS = [
    'mirror',
    'lens',
    'splitter',
    'detector',
    'aperture',
    'blocker',
] as const;
export type BenchKind = (typeof BENCH_KINDS)[number];
export interface BenchMount {
    part: string;
    base_size: [number, number];
    height_range: [number, number];
}
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
}
export interface BenchSource {
    id: string;
    position: Vec3;
    direction: Vec3;
    wavelength: number;
    power: number;
    beam_radius: number;
    rings: number;
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
    return kind === 'source' || kind === 'lens' || kind === 'aperture'
        ? ['out']
        : kind === 'mirror'
          ? ['reflected']
          : kind === 'splitter'
            ? ['reflected', 'transmitted']
            : [];
}
/** Structural input guard. The engine owns geometric and graph validation. */
export function benchProblems(value: unknown): string[] {
    if (value == null) return [];
    const vector = (v: unknown, n = 3): boolean =>
        Array.isArray(v) &&
        v.length === n &&
        v.every((x) => typeof x === 'number' && Number.isFinite(x));
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
