import { tr } from './i18n';
import { benchProblems, type Bench } from './bench';
import { paletteColor } from './color';
import type {
    Field,
    FieldType,
    EndpointSurface,
    GapSolve,
    Glass,
    Interaction,
    OpticalSystem,
    PupilKind,
    PupilMapping,
    Surface,
    Vignetting,
    RGBA,
    Wavelength,
} from './types';

const isRecord = (v: unknown): v is Record<string, unknown> =>
    typeof v === 'object' && v !== null && !Array.isArray(v);
const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const isNumArray = (v: unknown): v is number[] => Array.isArray(v) && v.every(isNum);
const isTuple = (v: unknown, n: number): v is number[] => isNumArray(v) && v.length === n;

export const FIELD_TYPES: readonly FieldType[] = ['angle', 'object_height', 'image_height'];
export const PUPIL_KINDS: readonly PupilKind[] = [
    'entrance_pupil_diameter',
    'f_number',
    'object_na',
    'image_na',
];
export const PUPIL_MAPPINGS: readonly PupilMapping[] = ['reference_entrance_pupil', 'real_stop'];
export const INTERACTIONS: readonly Interaction[] = ['refract', 'reflect', 'dummy'];

/**
 * Canonical JSON text: object keys sorted recursively, array order and every primitive kept exactly.
 * Two values that differ only in object property order produce the same text.
 */
export function canonicalJson(value: unknown): string {
    if (Array.isArray(value)) return `[${value.map((v) => canonicalJson(v ?? null)).join(',')}]`;
    if (typeof value === 'object' && value !== null) {
        const o = value as Record<string, unknown>;
        const parts = Object.keys(o)
            .sort()
            .filter((k) => o[k] !== undefined)
            .map((k) => `${JSON.stringify(k)}:${canonicalJson(o[k])}`);
        return `{${parts.join(',')}}`;
    }
    return JSON.stringify(value) ?? 'null';
}

export const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

export function newSurface(): Surface {
    return {
        label: '',
        radius: 0,
        thickness: 10,
        gap_solve: { kind: 'fixed' },
        material: 'AIR',
        aperture: 12.5,
        clear_aperture: true,
        semi_diameter: 0,
        conic: 0,
        even: [],
        odd: [],
        interaction: 'refract',
    };
}
export const newField = (n = 0): Field => ({
    x: 0,
    y: 0,
    weight: 1,
    vignetting: [0, 0, 0, 0],
    color: paletteColor(n),
});
export const newWavelength = (n = 0): Wavelength => ({
    value: 587.5618,
    weight: 1,
    color: paletteColor(n),
});

const isRgba = (v: unknown): v is RGBA => isTuple(v, 4) && v.every((c) => c >= 0 && c <= 1);

/** Plane endpoint with a unit clear radius, clipping off and a zero derived envelope. */
export const newEndpoint = (): EndpointSurface => ({
    radius: 0,
    conic: 0,
    even: [],
    odd: [],
    aperture: 1,
    clear_aperture: false,
    semi_diameter: 0,
});

/** Why an object at infinity cannot have this profile, or '' when it is valid. A conic alone has no shape at radius 0. */
export function infiniteObjectProblem(s: OpticalSystem): string {
    if (s.object_distance !== null) return '';
    const o = s.object_surface;
    const planar = o.radius === 0 && o.even.every((c) => c === 0) && o.odd.every((c) => c === 0);
    if (!planar) return tr('validation.m1');
    if (o.clear_aperture) return tr('validation.m2');
    return '';
}

/** Custom glasses are addressed as NAME_CATALOG, matching the backend's Glass::key. */
export const glassKey = (g: Glass): string => `${g.name}_${g.catalog}`;

/** Reject missing and unknown keys: the backend deserializes with deny_unknown_fields. */
function exactKeys(
    o: Record<string, unknown>,
    keys: readonly string[],
    where: string,
    optional: readonly string[] = [],
): string | null {
    for (const k of keys) if (!(k in o)) return tr('validation.m3', { p0: where, p1: k });
    for (const k of Object.keys(o))
        if (!keys.includes(k) && !optional.includes(k))
            return tr('validation.m4', { p0: where, p1: k });
    return null;
}

const SURFACE_KEYS = [
    'label',
    'radius',
    'thickness',
    'gap_solve',
    'material',
    'aperture',
    'clear_aperture',
    'semi_diameter',
    'conic',
    'even',
    'odd',
    'interaction',
];
const ENDPOINT_KEYS = [
    'radius',
    'conic',
    'even',
    'odd',
    'aperture',
    'clear_aperture',
    'semi_diameter',
];
const FIELD_KEYS = ['x', 'y', 'weight', 'vignetting', 'color'];
const WAVELENGTH_KEYS = ['value', 'weight', 'color'];
const GLASS_KEYS = [
    'name',
    'catalog',
    'formula',
    'coefficients',
    'nd',
    'vd',
    'wavelength_range',
    'thermal',
    'transmission',
    'comment',
];
const SYSTEM_KEYS = [
    'name',
    'note',
    'surfaces',
    'wavelengths',
    'reference_wavelength',
    'fields',
    'field_type',
    'pupil',
    'pupil_mapping',
    'object_distance',
    'object_material',
    'object_label',
    'image_label',
    'object_surface',
    'image_surface',
    'stop_index',
    'temperature',
    'pressure',
    'custom_glasses',
];

const isIndex = (v: unknown): v is number => isNum(v) && Number.isInteger(v) && v >= 0;

/** Strict GapSolve union: exact keys per kind. Solvability is the engine's job. */
export function parseGapSolve(value: unknown, where: string): GapSolve | string {
    const w = `${where} gap_solve`;
    if (!isRecord(value)) return tr('validation.m5', { p0: w });
    switch (value.kind) {
        case 'fixed': {
            const bad = exactKeys(value, ['kind'], w);
            return bad ?? { kind: 'fixed' };
        }
        case 'pickup': {
            const bad = exactKeys(value, ['kind', 'source', 'scale', 'offset'], w);
            if (bad) return bad;
            if (!isIndex(value.source)) return tr('validation.m6', { p0: w });
            if (!isNum(value.scale) || !isNum(value.offset)) return tr('validation.m7', { p0: w });
            return {
                kind: 'pickup',
                source: value.source,
                scale: value.scale,
                offset: value.offset,
            };
        }
        case 'edge_thickness': {
            const bad = exactKeys(value, ['kind', 'height', 'value'], w);
            if (bad) return bad;
            if (!isNum(value.height) || !isNum(value.value)) return tr('validation.m8', { p0: w });
            return { kind: 'edge_thickness', height: value.height, value: value.value };
        }
        case 'overall_length': {
            const bad = exactKeys(value, ['kind', 'first', 'last', 'value'], w);
            if (bad) return bad;
            if (!isIndex(value.first) || !isIndex(value.last))
                return tr('validation.m9', { p0: w });
            if (!isNum(value.value)) return tr('validation.m10', { p0: w });
            return {
                kind: 'overall_length',
                first: value.first,
                last: value.last,
                value: value.value,
            };
        }
        case 'marginal_height': {
            const bad = exactKeys(value, ['kind', 'value', 'zone'], w);
            if (bad) return bad;
            if (!isNum(value.value) || !isNum(value.zone)) return tr('validation.m11', { p0: w });
            return { kind: 'marginal_height', value: value.value, zone: value.zone };
        }
        default:
            return tr('validation.m12', { p0: w });
    }
}

function parseGlass(value: unknown, i: number): Glass | string {
    const where = `Custom glass ${i + 1}`;
    if (!isRecord(value)) return tr('validation.m13', { p0: where });
    const bad = exactKeys(value, GLASS_KEYS, where);
    if (bad) return bad;
    const g = value;
    if (
        typeof g.name !== 'string' ||
        typeof g.catalog !== 'string' ||
        typeof g.comment !== 'string'
    )
        return tr('validation.m14', { p0: where });
    if (!isNum(g.formula) || !Number.isInteger(g.formula) || g.formula < 1 || g.formula > 13)
        return tr('validation.m15', { p0: where });
    if (!isNumArray(g.coefficients)) return tr('validation.m16', { p0: where });
    if (!isNum(g.nd) || !isNum(g.vd)) return tr('validation.m17', { p0: where });
    if (g.wavelength_range !== null && !isTuple(g.wavelength_range, 2))
        return tr('validation.m18', { p0: where });
    if (g.thermal !== null && !isTuple(g.thermal, 7)) return tr('validation.m19', { p0: where });
    if (!Array.isArray(g.transmission) || !g.transmission.every((t) => isTuple(t, 3)))
        return tr('validation.m20', { p0: where });
    return {
        name: g.name,
        catalog: g.catalog,
        formula: g.formula,
        coefficients: g.coefficients,
        nd: g.nd,
        vd: g.vd,
        wavelength_range: g.wavelength_range as Glass['wavelength_range'],
        thermal: g.thermal as Glass['thermal'],
        transmission: g.transmission as Glass['transmission'],
        comment: g.comment,
    };
}

/** Strict endpoint profile: exact keys, finite numbers, nonnegative derived radius. */
function parseEndpoint(value: unknown, where: string): EndpointSurface | string {
    if (!isRecord(value))
        return tr('validation.m21', { p0: where.toLowerCase().replace(' ', '_') });
    const bad = exactKeys(value, ENDPOINT_KEYS, where);
    if (bad) return bad;
    if (!isNum(value.radius) || !isNum(value.aperture) || !isNum(value.conic))
        return tr('validation.m22', { p0: where });
    if (!isNum(value.semi_diameter) || value.semi_diameter < 0)
        return tr('validation.m23', { p0: where });
    if (typeof value.clear_aperture !== 'boolean') return tr('validation.m24', { p0: where });
    if (!isNumArray(value.even) || !isNumArray(value.odd))
        return tr('validation.m25', { p0: where });
    return {
        radius: value.radius,
        conic: value.conic,
        even: value.even,
        odd: value.odd,
        aperture: value.aperture,
        clear_aperture: value.clear_aperture,
        semi_diameter: value.semi_diameter,
    };
}

/** Parse untrusted JSON into an OpticalSystem; returns an error string on failure. */
export function parseSystem(value: unknown): OpticalSystem | string {
    if (!isRecord(value)) return tr('validation.m26');
    const top = exactKeys(value, SYSTEM_KEYS, 'System', ['bench']);
    if (top) return top;
    const benchErrors = benchProblems(value.bench);
    if (benchErrors.length) return benchErrors[0]!;
    const v = value;
    if (typeof v.name !== 'string') return tr('validation.m27');
    if (typeof v.note !== 'string') return tr('validation.m28');
    if (!Array.isArray(v.surfaces)) return tr('validation.m29');
    if (!Array.isArray(v.wavelengths)) return tr('validation.m30');
    if (!Array.isArray(v.fields)) return tr('validation.m31');
    if (!Array.isArray(v.custom_glasses)) return tr('validation.m32');
    if (
        !isNum(v.reference_wavelength) ||
        !Number.isInteger(v.reference_wavelength) ||
        v.reference_wavelength < 0
    )
        return tr('validation.m33');
    if (!isNum(v.stop_index) || !Number.isInteger(v.stop_index) || v.stop_index < 0)
        return tr('validation.m34');
    if (!FIELD_TYPES.includes(v.field_type as FieldType))
        return tr('validation.m35', { p0: FIELD_TYPES.join(', ') });
    if (v.object_distance !== null && !isNum(v.object_distance)) return tr('validation.m36');
    if (typeof v.object_material !== 'string') return tr('validation.m37');
    if (typeof v.object_label !== 'string' || v.object_label.length > 256)
        return tr('validation.m38');
    if (typeof v.image_label !== 'string' || v.image_label.length > 256)
        return tr('validation.m39');
    if (!isNum(v.temperature) || !isNum(v.pressure)) return tr('validation.m40');
    if (!isRecord(v.pupil)) return tr('validation.m41');
    const pupilBad = exactKeys(v.pupil, ['kind', 'value'], 'Pupil');
    if (pupilBad) return pupilBad;
    if (!PUPIL_KINDS.includes(v.pupil.kind as PupilKind))
        return tr('validation.m42', { p0: PUPIL_KINDS.join(', ') });
    if (!isNum(v.pupil.value)) return tr('validation.m43');
    if (!PUPIL_MAPPINGS.includes(v.pupil_mapping as PupilMapping))
        return tr('validation.m44', { p0: PUPIL_MAPPINGS.join(', ') });
    const objectSurface = parseEndpoint(v.object_surface, 'Object surface');
    if (typeof objectSurface === 'string') return objectSurface;
    const imageSurface = parseEndpoint(v.image_surface, 'Image surface');
    if (typeof imageSurface === 'string') return imageSurface;

    const surfaces: Surface[] = [];
    for (const [i, s] of v.surfaces.entries()) {
        const where = `Surface ${i + 1}`;
        if (!isRecord(s)) return tr('validation.m45', { p0: where });
        const bad = exactKeys(s, SURFACE_KEYS, where);
        if (bad) return bad;
        if (!isNum(s.radius) || !isNum(s.thickness) || !isNum(s.aperture) || !isNum(s.conic))
            return tr('validation.m46', { p0: where });
        if (typeof s.material !== 'string' || typeof s.label !== 'string')
            return tr('validation.m47', { p0: where });
        if (!isNum(s.semi_diameter) || s.semi_diameter < 0)
            return tr('validation.m48', { p0: where });
        if (typeof s.clear_aperture !== 'boolean') return tr('validation.m49', { p0: where });
        if (!isNumArray(s.even) || !isNumArray(s.odd)) return tr('validation.m50', { p0: where });
        if (!INTERACTIONS.includes(s.interaction as Interaction))
            return tr('validation.m51', { p0: where, p1: INTERACTIONS.join(', ') });
        const gapSolve = parseGapSolve(s.gap_solve, where);
        if (typeof gapSolve === 'string') return gapSolve;
        surfaces.push({
            label: s.label,
            radius: s.radius,
            thickness: s.thickness,
            gap_solve: gapSolve,
            material: s.material,
            aperture: s.aperture,
            clear_aperture: s.clear_aperture,
            semi_diameter: s.semi_diameter,
            conic: s.conic,
            even: s.even,
            odd: s.odd,
            interaction: s.interaction as Interaction,
        });
    }
    const wavelengths: Wavelength[] = [];
    for (const [i, w] of v.wavelengths.entries()) {
        if (!isRecord(w)) return tr('validation.m52', { p0: i + 1 });
        const bad = exactKeys(w, WAVELENGTH_KEYS, `Wavelength ${i + 1}`);
        if (bad) return bad;
        if (!isNum(w.value) || !isNum(w.weight)) return tr('validation.m53', { p0: i + 1 });
        if (!isRgba(w.color)) return tr('validation.m54', { p0: i + 1 });
        wavelengths.push({ value: w.value, weight: w.weight, color: w.color });
    }
    const fields: Field[] = [];
    for (const [i, f] of v.fields.entries()) {
        if (!isRecord(f)) return tr('validation.m55', { p0: i + 1 });
        const bad = exactKeys(f, FIELD_KEYS, `Field ${i + 1}`);
        if (bad) return bad;
        if (!isNum(f.x) || !isNum(f.y) || !isNum(f.weight))
            return tr('validation.m56', { p0: i + 1 });
        if (!isTuple(f.vignetting, 4)) return tr('validation.m57', { p0: i + 1 });
        if (!isRgba(f.color)) return tr('validation.m58', { p0: i + 1 });
        fields.push({
            x: f.x,
            y: f.y,
            weight: f.weight,
            vignetting: f.vignetting as Vignetting,
            color: f.color,
        });
    }
    const glasses: Glass[] = [];
    for (const [i, g] of v.custom_glasses.entries()) {
        const parsed = parseGlass(g, i);
        if (typeof parsed === 'string') return parsed;
        glasses.push(parsed);
    }
    return {
        name: v.name,
        bench: v.bench as Bench | null | undefined,
        note: v.note,
        surfaces,
        wavelengths,
        reference_wavelength: v.reference_wavelength,
        fields,
        field_type: v.field_type as FieldType,
        pupil: { kind: v.pupil.kind as PupilKind, value: v.pupil.value },
        pupil_mapping: v.pupil_mapping as PupilMapping,
        object_distance: v.object_distance,
        object_material: v.object_material,
        object_label: v.object_label,
        image_label: v.image_label,
        object_surface: objectSurface,
        image_surface: imageSurface,
        stop_index: v.stop_index,
        temperature: v.temperature,
        pressure: v.pressure,
        custom_glasses: glasses,
    };
}

/**
 * Required numeric ranges and index bounds only. Physical consistency (materials,
 * ray aiming, pupil feasibility, …) is left to the backend, whose errors are shown as-is.
 */
export function validateSystem(s: OpticalSystem): string[] {
    const p: string[] = benchProblems(s.bench);
    if (s.name.length > 256) p.push(tr('validation.m59'));
    if (s.note.length > 100_000) p.push(tr('validation.m60'));
    if (s.surfaces.length < 1 || s.surfaces.length > 128) p.push(tr('validation.m61'));
    if (s.wavelengths.length < 1 || s.wavelengths.length > 32) p.push(tr('validation.m62'));
    if (s.fields.length < 1 || s.fields.length > 64) p.push(tr('validation.m63'));
    if (s.reference_wavelength >= s.wavelengths.length) p.push(tr('validation.m64'));
    if (s.stop_index >= s.surfaces.length) p.push(tr('validation.m65'));
    if (!(s.pupil.value > 0)) p.push(tr('validation.m66'));
    if (s.object_label.length > 256) p.push(tr('validation.m67'));
    if (s.image_label.length > 256) p.push(tr('validation.m68'));
    if (s.object_material.trim() === '') p.push(tr('validation.m69'));
    for (const [name, e] of [
        ['Object surface', s.object_surface],
        ['Image surface', s.image_surface],
    ] as const) {
        if (!(e.aperture > 0)) p.push(tr('validation.m70', { p0: name }));
        if (e.even.length > 16) p.push(tr('validation.m71', { p0: name }));
        if (e.odd.length > 20) p.push(tr('validation.m72', { p0: name }));
    }
    const infinite = infiniteObjectProblem(s);
    if (infinite) p.push(infinite);
    if (s.object_distance !== null && !(s.object_distance > 0)) p.push(tr('validation.m73'));
    if (s.field_type !== 'angle' && s.object_distance === null) p.push(tr('validation.m74'));
    if (s.custom_glasses.length > 10_000) p.push(tr('validation.m75'));
    const keys = new Set<string>();
    for (const g of s.custom_glasses) {
        const k = glassKey(g);
        if (keys.has(k)) p.push(tr('validation.m76', { p0: k }));
        keys.add(k);
    }
    s.wavelengths.forEach((w, i) => {
        if (w.value < 100 || w.value > 20000) p.push(tr('validation.m77', { p0: i + 1 }));
        if (w.weight < 0) p.push(tr('validation.m78', { p0: i + 1 }));
    });
    if (!s.wavelengths.some((w) => w.weight > 0)) p.push(tr('validation.m79'));
    s.fields.forEach((f, i) => {
        const n = i + 1;
        if (f.weight < 0) p.push(tr('validation.m80', { p0: n }));
        if (s.field_type === 'angle' && (Math.abs(f.x) >= 80 || Math.abs(f.y) >= 80))
            p.push(tr('validation.m81', { p0: n }));
        if (f.vignetting.some((v) => !Number.isFinite(v) || v >= 1))
            p.push(tr('validation.m82', { p0: n }));
    });
    if (!s.fields.some((f) => f.weight > 0)) p.push(tr('validation.m83'));
    s.surfaces.forEach((x, i) => {
        const n = i + 1;
        if (x.label.length > 256) p.push(tr('validation.m84', { p0: n }));
        if (!(x.aperture > 0)) p.push(tr('validation.m85', { p0: n }));
        if (x.even.length > 16) p.push(tr('validation.m86', { p0: n }));
        if (x.odd.length > 20) p.push(tr('validation.m87', { p0: n }));
        const g = x.gap_solve;
        if (g.kind === 'pickup' && g.source >= s.surfaces.length)
            p.push(tr('validation.m88', { p0: n }));
        else if (g.kind === 'pickup' && g.source === i) p.push(tr('validation.m89', { p0: n }));
        if (g.kind === 'overall_length') {
            if (g.first >= g.last || g.last > s.surfaces.length)
                p.push(tr('validation.m90', { p0: n }));
            else if (i < g.first || i >= g.last) p.push(tr('validation.m91', { p0: n }));
        }
    });
    return p;
}

/** Parse a comma/space separated list; null when any entry is not a finite number. */
export function parseNumberList(text: string): number[] | null {
    const parts = text.split(/[\s,;]+/).filter((t) => t !== '');
    const nums = parts.map(Number);
    return nums.every(Number.isFinite) ? nums : null;
}

export function fieldUnit(t: FieldType): string {
    return t === 'angle' ? '°' : 'mm';
}

/** Human label for a stored field number, e.g. "2: (0, 5)°". */
export function fieldLabel(s: OpticalSystem, i: number): string {
    const f = s.fields[i];
    if (!f) return `${i + 1}`;
    const where = f.x === 0 ? `${f.y}` : `(${f.x}, ${f.y})`;
    const kind = s.field_type === 'angle' ? '' : s.field_type === 'object_height' ? ' obj' : ' img';
    return `${i + 1}: ${where}${fieldUnit(s.field_type)}${kind}`;
}
