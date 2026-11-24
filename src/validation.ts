import { z } from 'zod';
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
    Wavelength,
} from './types';

const isRecord = (v: unknown): v is Record<string, unknown> =>
    typeof v === 'object' && v !== null && !Array.isArray(v);

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
            .toSorted()
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

// ---- zod schema validation ----------------------------------------------------

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

/**
 * Missing keys first (in declared order), then unknown keys, matching the backend's
 * deny_unknown_fields diagnostics. Field-level messages come from the schemas themselves.
 */
function keyProblem(
    issues: z.core.$ZodIssue[],
    keys: readonly string[],
    where: string,
    value: unknown,
): string | null {
    // Property issues carry no input in zod v4, so missing keys are detected on the raw record.
    if (isRecord(value))
        for (const k of keys)
            if (!(k in value) && issues.some((i) => i.path.length === 1 && i.path[0] === k))
                return tr('validation.m3', { p0: where, p1: k });
    const extra = issues.find((i) => i.code === 'unrecognized_keys' && i.path.length === 0);
    if (extra && extra.code === 'unrecognized_keys')
        return tr('validation.m4', { p0: where, p1: extra.keys[0]! });
    return null;
}

// The `error` parameter of a base schema covers every check chained onto it (zod v4),
// so one localized message per field is enough.
const num = (msg: string) => z.number({ error: () => msg });
const nums = (msg: string) => z.array(num(msg), { error: () => msg });
const index = (msg: string) => num(msg).int().nonnegative();
const tuple2 = (msg: string) => z.tuple([num(msg), num(msg)], { error: () => msg });
const tuple3 = (msg: string) => z.tuple([num(msg), num(msg), num(msg)], { error: () => msg });
const tuple4 = (msg: string) =>
    z.tuple([num(msg), num(msg), num(msg), num(msg)], { error: () => msg });
const tuple7 = (msg: string) =>
    z.tuple([num(msg), num(msg), num(msg), num(msg), num(msg), num(msg), num(msg)], {
        error: () => msg,
    });
const rgba = (msg: string) => {
    const channel = num(msg).min(0).max(1);
    return z.tuple([channel, channel, channel, channel], { error: () => msg });
};

/** Strict GapSolve union: exact keys per kind. Solvability is the engine's job. */
export function parseGapSolve(value: unknown, where: string): GapSolve | string {
    const w = `${where} gap_solve`;
    if (!isRecord(value)) return tr('validation.m5', { p0: w });
    const parsed = z
        .discriminatedUnion(
            'kind',
            [
                z.strictObject({ kind: z.literal('fixed') }),
                z.strictObject({
                    kind: z.literal('pickup'),
                    source: index(tr('validation.m6', { p0: w })),
                    scale: num(tr('validation.m7', { p0: w })),
                    offset: num(tr('validation.m7', { p0: w })),
                }),
                z.strictObject({
                    kind: z.literal('edge_thickness'),
                    height: num(tr('validation.m8', { p0: w })),
                    value: num(tr('validation.m8', { p0: w })),
                }),
                z.strictObject({
                    kind: z.literal('overall_length'),
                    first: index(tr('validation.m9', { p0: w })),
                    last: index(tr('validation.m9', { p0: w })),
                    value: num(tr('validation.m10', { p0: w })),
                }),
                z.strictObject({
                    kind: z.literal('marginal_height'),
                    value: num(tr('validation.m11', { p0: w })),
                    zone: num(tr('validation.m11', { p0: w })),
                }),
            ],
            { error: () => tr('validation.m12', { p0: w }) },
        )
        .safeParse(value);
    if (parsed.success) return parsed.data;
    const variantKeys: Record<string, string[]> = {
        fixed: ['kind'],
        pickup: ['kind', 'source', 'scale', 'offset'],
        edge_thickness: ['kind', 'height', 'value'],
        overall_length: ['kind', 'first', 'last', 'value'],
        marginal_height: ['kind', 'value', 'zone'],
    };
    const keys = variantKeys[value.kind as string];
    if (!keys) return tr('validation.m12', { p0: w });
    return (
        keyProblem(parsed.error.issues, keys, w, value) ??
        parsed.error.issues[0]?.message ??
        tr('validation.m12', { p0: w })
    );
}

function glassSchema(where: string) {
    return z.strictObject(
        {
            name: z.string({ error: () => tr('validation.m14', { p0: where }) }),
            catalog: z.string({ error: () => tr('validation.m14', { p0: where }) }),
            comment: z.string({ error: () => tr('validation.m14', { p0: where }) }),
            formula: num(tr('validation.m15', { p0: where }))
                .int()
                .min(1)
                .max(13),
            coefficients: nums(tr('validation.m16', { p0: where })),
            nd: num(tr('validation.m17', { p0: where })),
            vd: num(tr('validation.m17', { p0: where })),
            wavelength_range: tuple2(tr('validation.m18', { p0: where })).nullable(),
            thermal: tuple7(tr('validation.m19', { p0: where })).nullable(),
            transmission: z.array(tuple3(tr('validation.m20', { p0: where })), {
                error: () => tr('validation.m20', { p0: where }),
            }),
        },
        {
            error: (iss) => (isRecord(iss.input) ? undefined : tr('validation.m13', { p0: where })),
        },
    );
}

/** Strict endpoint profile: exact keys, finite numbers, nonnegative derived radius. */
const endpointSchema = (where: string) =>
    z.strictObject(
        {
            radius: num(tr('validation.m22', { p0: where })),
            aperture: num(tr('validation.m22', { p0: where })),
            conic: num(tr('validation.m22', { p0: where })),
            semi_diameter: num(tr('validation.m23', { p0: where })).nonnegative(),
            clear_aperture: z.boolean({ error: () => tr('validation.m24', { p0: where }) }),
            even: nums(tr('validation.m25', { p0: where })),
            odd: nums(tr('validation.m25', { p0: where })),
        },
        {
            error: (iss) =>
                isRecord(iss.input)
                    ? undefined
                    : tr('validation.m21', { p0: where.toLowerCase().replace(' ', '_') }),
        },
    );

function parseEndpoint(value: unknown, where: string): EndpointSurface | string {
    const parsed = endpointSchema(where).safeParse(value);
    if (parsed.success) return parsed.data;
    return (
        keyProblem(parsed.error.issues, ENDPOINT_KEYS, where, value) ??
        parsed.error.issues[0]!.message
    );
}

const surfaceSchema = (where: string) =>
    z.strictObject(
        {
            radius: num(tr('validation.m46', { p0: where })),
            thickness: num(tr('validation.m46', { p0: where })),
            aperture: num(tr('validation.m46', { p0: where })),
            conic: num(tr('validation.m46', { p0: where })),
            label: z.string({ error: () => tr('validation.m47', { p0: where }) }),
            material: z.string({ error: () => tr('validation.m47', { p0: where }) }),
            semi_diameter: num(tr('validation.m48', { p0: where })).nonnegative(),
            clear_aperture: z.boolean({ error: () => tr('validation.m49', { p0: where }) }),
            even: nums(tr('validation.m50', { p0: where })),
            odd: nums(tr('validation.m50', { p0: where })),
            toroid: z
                .strictObject({
                    revolution: num(tr('validation.m46', { p0: where })),
                    even_y: nums(tr('validation.m50', { p0: where })),
                })
                .nullable()
                .optional(),
            zernike: z
                .strictObject({
                    normalize: num(tr('validation.m46', { p0: where })),
                    coefficients: nums(tr('validation.m50', { p0: where })),
                })
                .nullable()
                .optional(),
            transform: z
                .strictObject({
                    decenter: tuple2(tr('validation.m46', { p0: where })),
                    tilt: tuple3(tr('validation.m46', { p0: where })),
                    tilt_first: z.boolean(),
                    return_after: z.boolean(),
                })
                .nullable()
                .optional(),
            interaction: z.enum(INTERACTIONS as [Interaction, ...Interaction[]], {
                error: () => tr('validation.m51', { p0: where, p1: INTERACTIONS.join(', ') }),
            }),
            gap_solve: z.unknown(),
        },
        {
            error: (iss) => (isRecord(iss.input) ? undefined : tr('validation.m45', { p0: where })),
        },
    );

const wavelengthSchema = (i: number) =>
    z.strictObject(
        {
            value: num(tr('validation.m53', { p0: i + 1 })),
            weight: num(tr('validation.m53', { p0: i + 1 })),
            color: rgba(tr('validation.m54', { p0: i + 1 })),
        },
        {
            error: (iss) => (isRecord(iss.input) ? undefined : tr('validation.m52', { p0: i + 1 })),
        },
    );

const fieldSchema = (i: number) =>
    z.strictObject(
        {
            x: num(tr('validation.m56', { p0: i + 1 })),
            y: num(tr('validation.m56', { p0: i + 1 })),
            weight: num(tr('validation.m56', { p0: i + 1 })),
            vignetting: tuple4(tr('validation.m57', { p0: i + 1 })),
            color: rgba(tr('validation.m58', { p0: i + 1 })),
        },
        {
            error: (iss) => (isRecord(iss.input) ? undefined : tr('validation.m55', { p0: i + 1 })),
        },
    );

// Field order matches the historical check order so the first reported problem stays the same.
const systemSchema = z.strictObject(
    {
        name: z.string({ error: () => tr('validation.m27') }),
        note: z.string({ error: () => tr('validation.m28') }),
        surfaces: z.array(z.unknown(), { error: () => tr('validation.m29') }),
        wavelengths: z.array(z.unknown(), { error: () => tr('validation.m30') }),
        fields: z.array(z.unknown(), { error: () => tr('validation.m31') }),
        custom_glasses: z.array(z.unknown(), { error: () => tr('validation.m32') }),
        reference_wavelength: index(tr('validation.m33')),
        stop_index: index(tr('validation.m34')),
        field_type: z.enum(FIELD_TYPES as [FieldType, ...FieldType[]], {
            error: () => tr('validation.m35', { p0: FIELD_TYPES.join(', ') }),
        }),
        object_distance: num(tr('validation.m36')).nullable(),
        object_material: z.string({ error: () => tr('validation.m37') }),
        object_label: z.string({ error: () => tr('validation.m38') }).max(256),
        image_label: z.string({ error: () => tr('validation.m39') }).max(256),
        temperature: num(tr('validation.m40')),
        pressure: num(tr('validation.m40')),
        pupil: z.strictObject(
            {
                kind: z.enum(PUPIL_KINDS as [PupilKind, ...PupilKind[]], {
                    error: () => tr('validation.m42', { p0: PUPIL_KINDS.join(', ') }),
                }),
                value: num(tr('validation.m43')),
            },
            {
                error: (iss) => (isRecord(iss.input) ? undefined : tr('validation.m41')),
            },
        ),
        pupil_mapping: z.enum(PUPIL_MAPPINGS as [PupilMapping, ...PupilMapping[]], {
            error: () => tr('validation.m44', { p0: PUPIL_MAPPINGS.join(', ') }),
        }),
        object_surface: z.unknown(),
        image_surface: z.unknown(),
        bench: z.unknown().optional(),
        configurations: z.unknown().optional(),
        active_configuration: z.unknown().optional(),
    },
    { error: (iss) => (isRecord(iss.input) ? undefined : tr('validation.m26')) },
);

/** Parse untrusted JSON into an OpticalSystem; returns an error string on failure. */
export function parseSystem(value: unknown): OpticalSystem | string {
    if (!isRecord(value)) return tr('validation.m26');
    const parsed = systemSchema.safeParse(value);
    if (!parsed.success) {
        const top = keyProblem(parsed.error.issues, SYSTEM_KEYS, 'System', value);
        if (top) return top;
        const benchErrors = benchProblems(value.bench);
        if (benchErrors.length) return benchErrors[0]!;
        const first = parsed.error.issues[0]!;
        if (first.path[0] === 'pupil') {
            // Report pupil key errors with the Pupil context, like a standalone strict check.
            const sub = parsed.error.issues
                .filter((i) => i.path[0] === 'pupil')
                .map((i) => Object.assign({}, i, { path: i.path.slice(1) }));
            const bad = keyProblem(sub, ['kind', 'value'], 'Pupil', value.pupil);
            if (bad) return bad;
        }
        return first.message;
    }
    const benchErrors = benchProblems(value.bench);
    if (benchErrors.length) return benchErrors[0]!;
    const v = parsed.data;

    const objectSurface = parseEndpoint(v.object_surface, 'Object surface');
    if (typeof objectSurface === 'string') return objectSurface;
    const imageSurface = parseEndpoint(v.image_surface, 'Image surface');
    if (typeof imageSurface === 'string') return imageSurface;

    const surfaces: Surface[] = [];
    for (const [i, s] of v.surfaces.entries()) {
        const where = `Surface ${i + 1}`;
        const parsedSurface = surfaceSchema(where).safeParse(s);
        if (!parsedSurface.success)
            return (
                keyProblem(parsedSurface.error.issues, SURFACE_KEYS, where, s) ??
                parsedSurface.error.issues[0]!.message
            );
        const gapSolve = parseGapSolve(parsedSurface.data.gap_solve, where);
        if (typeof gapSolve === 'string') return gapSolve;
        surfaces.push({ ...parsedSurface.data, gap_solve: gapSolve });
    }
    const wavelengths: Wavelength[] = [];
    for (const [i, w] of v.wavelengths.entries()) {
        const parsedWavelength = wavelengthSchema(i).safeParse(w);
        if (!parsedWavelength.success)
            return (
                keyProblem(
                    parsedWavelength.error.issues,
                    WAVELENGTH_KEYS,
                    `Wavelength ${i + 1}`,
                    w,
                ) ?? parsedWavelength.error.issues[0]!.message
            );
        wavelengths.push(parsedWavelength.data);
    }
    const fields: Field[] = [];
    for (const [i, f] of v.fields.entries()) {
        const parsedField = fieldSchema(i).safeParse(f);
        if (!parsedField.success)
            return (
                keyProblem(parsedField.error.issues, FIELD_KEYS, `Field ${i + 1}`, f) ??
                parsedField.error.issues[0]!.message
            );
        fields.push(parsedField.data);
    }
    const glasses: Glass[] = [];
    for (const [i, g] of v.custom_glasses.entries()) {
        const where = `Custom glass ${i + 1}`;
        const parsedGlass = glassSchema(where).safeParse(g);
        if (!parsedGlass.success)
            return (
                keyProblem(parsedGlass.error.issues, GLASS_KEYS, where, g) ??
                parsedGlass.error.issues[0]!.message
            );
        glasses.push(parsedGlass.data);
    }
    return {
        name: v.name,
        bench: v.bench as Bench | null | undefined,
        note: v.note,
        surfaces,
        wavelengths,
        reference_wavelength: v.reference_wavelength,
        fields,
        field_type: v.field_type,
        pupil: v.pupil,
        pupil_mapping: v.pupil_mapping,
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
    const values = parts.map(Number);
    return values.every(Number.isFinite) ? values : null;
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
