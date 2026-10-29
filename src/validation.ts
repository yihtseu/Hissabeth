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
export const newField = (n = 0): Field => ({ x: 0, y: 0, weight: 1, vignetting: [0, 0, 0, 0], color: paletteColor(n) });
export const newWavelength = (n = 0): Wavelength => ({ value: 587.5618, weight: 1, color: paletteColor(n) });

const isRgba = (v: unknown): v is RGBA => isTuple(v, 4) && v.every((c) => c >= 0 && c <= 1);

/** Plane endpoint with a unit clear radius, clipping off and a zero derived envelope. */
export const newEndpoint = (): EndpointSurface => ({
  radius: 0, conic: 0, even: [], odd: [], aperture: 1, clear_aperture: false, semi_diameter: 0,
});

/** Why an object at infinity cannot have this profile, or '' when it is valid. A conic alone has no shape at radius 0. */
export function infiniteObjectProblem(s: OpticalSystem): string {
  if (s.object_distance !== null) return '';
  const o = s.object_surface;
  const planar = o.radius === 0 && o.even.every((c) => c === 0) && o.odd.every((c) => c === 0);
  if (!planar) return 'An object at infinity needs a planar object surface (radius 0 and no nonzero asphere coefficients).';
  if (o.clear_aperture) return 'An object at infinity cannot clip rays: turn off Object surface clip rays.';
  return '';
}

/** Custom glasses are addressed as NAME_CATALOG, matching the backend's Glass::key. */
export const glassKey = (g: Glass): string => `${g.name}_${g.catalog}`;

/** Reject missing and unknown keys: the backend deserializes with deny_unknown_fields. */
function exactKeys(o: Record<string, unknown>, keys: readonly string[], where: string): string | null {
  for (const k of keys) if (!(k in o)) return `${where}: missing "${k}"`;
  for (const k of Object.keys(o)) if (!keys.includes(k)) return `${where}: unknown field "${k}"`;
  return null;
}

const SURFACE_KEYS = ['label', 'radius', 'thickness', 'gap_solve', 'material', 'aperture', 'clear_aperture', 'semi_diameter', 'conic', 'even', 'odd', 'interaction'];
const ENDPOINT_KEYS = ['radius', 'conic', 'even', 'odd', 'aperture', 'clear_aperture', 'semi_diameter'];
const FIELD_KEYS = ['x', 'y', 'weight', 'vignetting', 'color'];
const WAVELENGTH_KEYS = ['value', 'weight', 'color'];
const GLASS_KEYS = [
  'name', 'catalog', 'formula', 'coefficients', 'nd', 'vd',
  'wavelength_range', 'thermal', 'transmission', 'comment',
];
const SYSTEM_KEYS = [
  'name', 'note', 'surfaces', 'wavelengths', 'reference_wavelength', 'fields', 'field_type',
  'pupil', 'pupil_mapping', 'object_distance', 'object_material', 'object_label', 'image_label', 'object_surface', 'image_surface', 'stop_index', 'temperature', 'pressure', 'custom_glasses',
];

const isIndex = (v: unknown): v is number => isNum(v) && Number.isInteger(v) && v >= 0;

/** Strict GapSolve union: exact keys per kind. Solvability is the engine's job. */
export function parseGapSolve(value: unknown, where: string): GapSolve | string {
  const w = `${where} gap_solve`;
  if (!isRecord(value)) return `${w} must be an object`;
  switch (value.kind) {
    case 'fixed': {
      const bad = exactKeys(value, ['kind'], w);
      return bad ?? { kind: 'fixed' };
    }
    case 'pickup': {
      const bad = exactKeys(value, ['kind', 'source', 'scale', 'offset'], w);
      if (bad) return bad;
      if (!isIndex(value.source)) return `${w}: source must be a non-negative integer gap index`;
      if (!isNum(value.scale) || !isNum(value.offset)) return `${w}: scale and offset must be numbers`;
      return { kind: 'pickup', source: value.source, scale: value.scale, offset: value.offset };
    }
    case 'edge_thickness': {
      const bad = exactKeys(value, ['kind', 'height', 'value'], w);
      if (bad) return bad;
      if (!isNum(value.height) || !isNum(value.value)) return `${w}: height and value must be numbers`;
      return { kind: 'edge_thickness', height: value.height, value: value.value };
    }
    case 'overall_length': {
      const bad = exactKeys(value, ['kind', 'first', 'last', 'value'], w);
      if (bad) return bad;
      if (!isIndex(value.first) || !isIndex(value.last)) return `${w}: first and last must be non-negative integer gap indexes`;
      if (!isNum(value.value)) return `${w}: value must be a number`;
      return { kind: 'overall_length', first: value.first, last: value.last, value: value.value };
    }
    case 'marginal_height': {
      const bad = exactKeys(value, ['kind', 'value', 'zone'], w);
      if (bad) return bad;
      if (!isNum(value.value) || !isNum(value.zone)) return `${w}: value and zone must be numbers`;
      return { kind: 'marginal_height', value: value.value, zone: value.zone };
    }
    default:
      return `${w}: kind must be one of fixed, pickup, edge_thickness, overall_length, marginal_height`;
  }
}

function parseGlass(value: unknown, i: number): Glass | string {
  const where = `Custom glass ${i + 1}`;
  if (!isRecord(value)) return `${where} must be an object`;
  const bad = exactKeys(value, GLASS_KEYS, where);
  if (bad) return bad;
  const g = value;
  if (typeof g.name !== 'string' || typeof g.catalog !== 'string' || typeof g.comment !== 'string')
    return `${where}: name, catalog and comment must be strings`;
  if (!isNum(g.formula) || !Number.isInteger(g.formula) || g.formula < 1 || g.formula > 13)
    return `${where}: formula must be an integer 1–13`;
  if (!isNumArray(g.coefficients)) return `${where}: coefficients must be numbers`;
  if (!isNum(g.nd) || !isNum(g.vd)) return `${where}: nd and vd must be numbers`;
  if (g.wavelength_range !== null && !isTuple(g.wavelength_range, 2))
    return `${where}: wavelength_range must be null or [low, high] in µm`;
  if (g.thermal !== null && !isTuple(g.thermal, 7))
    return `${where}: thermal must be null or 7 numbers`;
  if (!Array.isArray(g.transmission) || !g.transmission.every((t) => isTuple(t, 3)))
    return `${where}: transmission must be [µm, transmittance, thickness] triples`;
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
  if (!isRecord(value)) return `"${where.toLowerCase().replace(' ', '_')}" must be an object`;
  const bad = exactKeys(value, ENDPOINT_KEYS, where);
  if (bad) return bad;
  if (!isNum(value.radius) || !isNum(value.aperture) || !isNum(value.conic))
    return `${where}: radius, aperture and conic must be numbers`;
  if (!isNum(value.semi_diameter) || value.semi_diameter < 0)
    return `${where}: semi_diameter must be a non-negative number`;
  if (typeof value.clear_aperture !== 'boolean') return `${where}: clear_aperture must be true or false`;
  if (!isNumArray(value.even) || !isNumArray(value.odd)) return `${where}: even and odd must be arrays of numbers`;
  return {
    radius: value.radius, conic: value.conic, even: value.even, odd: value.odd,
    aperture: value.aperture, clear_aperture: value.clear_aperture, semi_diameter: value.semi_diameter,
  };
}

/** Parse untrusted JSON into an OpticalSystem; returns an error string on failure. */
export function parseSystem(value: unknown): OpticalSystem | string {
  if (!isRecord(value)) return 'System must be a JSON object';
  const top = exactKeys(value, SYSTEM_KEYS, 'System');
  if (top) return top;
  const v = value;
  if (typeof v.name !== 'string') return '"name" must be a string';
  if (typeof v.note !== 'string') return '"note" must be a string';
  if (!Array.isArray(v.surfaces)) return '"surfaces" must be an array';
  if (!Array.isArray(v.wavelengths)) return '"wavelengths" must be an array';
  if (!Array.isArray(v.fields)) return '"fields" must be an array';
  if (!Array.isArray(v.custom_glasses)) return '"custom_glasses" must be an array';
  if (!isNum(v.reference_wavelength) || !Number.isInteger(v.reference_wavelength) || v.reference_wavelength < 0)
    return '"reference_wavelength" must be a non-negative integer';
  if (!isNum(v.stop_index) || !Number.isInteger(v.stop_index) || v.stop_index < 0)
    return '"stop_index" must be a non-negative integer';
  if (!FIELD_TYPES.includes(v.field_type as FieldType))
    return `"field_type" must be one of ${FIELD_TYPES.join(', ')}`;
  if (v.object_distance !== null && !isNum(v.object_distance))
    return '"object_distance" must be a number or null';
  if (typeof v.object_material !== 'string') return '"object_material" must be a string';
  if (typeof v.object_label !== 'string' || v.object_label.length > 256)
    return '"object_label" must be a string of at most 256 characters';
  if (typeof v.image_label !== 'string' || v.image_label.length > 256)
    return '"image_label" must be a string of at most 256 characters';
  if (!isNum(v.temperature) || !isNum(v.pressure)) return '"temperature" and "pressure" must be numbers';
  if (!isRecord(v.pupil)) return '"pupil" must be an object';
  const pupilBad = exactKeys(v.pupil, ['kind', 'value'], 'Pupil');
  if (pupilBad) return pupilBad;
  if (!PUPIL_KINDS.includes(v.pupil.kind as PupilKind))
    return `pupil.kind must be one of ${PUPIL_KINDS.join(', ')}`;
  if (!isNum(v.pupil.value)) return 'pupil.value must be a number';
  if (!PUPIL_MAPPINGS.includes(v.pupil_mapping as PupilMapping))
    return `"pupil_mapping" must be one of ${PUPIL_MAPPINGS.join(', ')}`;
  const objectSurface = parseEndpoint(v.object_surface, 'Object surface');
  if (typeof objectSurface === 'string') return objectSurface;
  const imageSurface = parseEndpoint(v.image_surface, 'Image surface');
  if (typeof imageSurface === 'string') return imageSurface;

  const surfaces: Surface[] = [];
  for (const [i, s] of v.surfaces.entries()) {
    const where = `Surface ${i + 1}`;
    if (!isRecord(s)) return `${where} must be an object`;
    const bad = exactKeys(s, SURFACE_KEYS, where);
    if (bad) return bad;
    if (!isNum(s.radius) || !isNum(s.thickness) || !isNum(s.aperture) || !isNum(s.conic))
      return `${where}: radius, thickness, aperture and conic must be numbers`;
    if (typeof s.material !== 'string' || typeof s.label !== 'string')
      return `${where}: material and label must be strings`;
    if (!isNum(s.semi_diameter) || s.semi_diameter < 0)
      return `${where}: semi_diameter must be a non-negative number`;
    if (typeof s.clear_aperture !== 'boolean') return `${where}: clear_aperture must be true or false`;
    if (!isNumArray(s.even) || !isNumArray(s.odd))
      return `${where}: even and odd must be arrays of numbers`;
    if (!INTERACTIONS.includes(s.interaction as Interaction))
      return `${where}: interaction must be one of ${INTERACTIONS.join(', ')}`;
    const gapSolve = parseGapSolve(s.gap_solve, where);
    if (typeof gapSolve === 'string') return gapSolve;
    surfaces.push({
      label: s.label, radius: s.radius, thickness: s.thickness, gap_solve: gapSolve, material: s.material,
      aperture: s.aperture, clear_aperture: s.clear_aperture, semi_diameter: s.semi_diameter, conic: s.conic, even: s.even, odd: s.odd,
      interaction: s.interaction as Interaction,
    });
  }
  const wavelengths: Wavelength[] = [];
  for (const [i, w] of v.wavelengths.entries()) {
    if (!isRecord(w)) return `Wavelength ${i + 1} must be an object`;
    const bad = exactKeys(w, WAVELENGTH_KEYS, `Wavelength ${i + 1}`);
    if (bad) return bad;
    if (!isNum(w.value) || !isNum(w.weight)) return `Wavelength ${i + 1}: value and weight must be numbers`;
    if (!isRgba(w.color)) return `Wavelength ${i + 1}: color must be 4 numbers in 0..1 [r, g, b, a]`;
    wavelengths.push({ value: w.value, weight: w.weight, color: w.color });
  }
  const fields: Field[] = [];
  for (const [i, f] of v.fields.entries()) {
    if (!isRecord(f)) return `Field ${i + 1} must be an object`;
    const bad = exactKeys(f, FIELD_KEYS, `Field ${i + 1}`);
    if (bad) return bad;
    if (!isNum(f.x) || !isNum(f.y) || !isNum(f.weight)) return `Field ${i + 1}: x, y and weight must be numbers`;
    if (!isTuple(f.vignetting, 4)) return `Field ${i + 1}: vignetting must be 4 numbers [upper y, lower y, upper x, lower x]`;
    if (!isRgba(f.color)) return `Field ${i + 1}: color must be 4 numbers in 0..1 [r, g, b, a]`;
    fields.push({ x: f.x, y: f.y, weight: f.weight, vignetting: f.vignetting as Vignetting, color: f.color });
  }
  const glasses: Glass[] = [];
  for (const [i, g] of v.custom_glasses.entries()) {
    const parsed = parseGlass(g, i);
    if (typeof parsed === 'string') return parsed;
    glasses.push(parsed);
  }
  return {
    name: v.name,
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
  const p: string[] = [];
  if (s.name.length > 256) p.push('Name is longer than 256 characters.');
  if (s.note.length > 100_000) p.push('Note is longer than 100,000 characters.');
  if (s.surfaces.length < 1 || s.surfaces.length > 128) p.push('Surface count must be 1–128.');
  if (s.wavelengths.length < 1 || s.wavelengths.length > 32) p.push('Provide 1–32 wavelengths.');
  if (s.fields.length < 1 || s.fields.length > 64) p.push('Provide 1–64 fields.');
  if (s.reference_wavelength >= s.wavelengths.length) p.push('Reference wavelength must select a listed wavelength.');
  if (s.stop_index >= s.surfaces.length) p.push('Stop must select an existing surface.');
  if (!(s.pupil.value > 0)) p.push('Pupil value must be positive.');
  if (s.object_label.length > 256) p.push('Object label is longer than 256 characters.');
  if (s.image_label.length > 256) p.push('Image label is longer than 256 characters.');
  if (s.object_material.trim() === '') p.push('Object material must not be empty.');
  for (const [name, e] of [['Object surface', s.object_surface], ['Image surface', s.image_surface]] as const) {
    if (!(e.aperture > 0)) p.push(`${name}: clear radius must be positive.`);
    if (e.even.length > 16) p.push(`${name}: at most 16 even coefficients.`);
    if (e.odd.length > 20) p.push(`${name}: at most 20 odd coefficients.`);
  }
  const infinite = infiniteObjectProblem(s);
  if (infinite) p.push(infinite);
  if (s.object_distance !== null && !(s.object_distance > 0))
    p.push('Object distance must be positive (use infinity for none).');
  if (s.field_type !== 'angle' && s.object_distance === null)
    p.push('Height fields require a finite object distance.');
  if (s.custom_glasses.length > 10_000) p.push('At most 10,000 custom glasses.');
  const keys = new Set<string>();
  for (const g of s.custom_glasses) {
    const k = glassKey(g);
    if (keys.has(k)) p.push(`Duplicate custom glass ${k}.`);
    keys.add(k);
  }
  s.wavelengths.forEach((w, i) => {
    if (w.value < 100 || w.value > 20000) p.push(`Wavelength ${i + 1}: value must be 100–20000 nm.`);
    if (w.weight < 0) p.push(`Wavelength ${i + 1}: weight cannot be negative.`);
  });
  if (!s.wavelengths.some((w) => w.weight > 0)) p.push('At least one wavelength weight must be positive.');
  s.fields.forEach((f, i) => {
    const n = i + 1;
    if (f.weight < 0) p.push(`Field ${n}: weight cannot be negative.`);
    if (s.field_type === 'angle' && (Math.abs(f.x) >= 80 || Math.abs(f.y) >= 80))
      p.push(`Field ${n}: angles must be within ±80°.`);
    if (f.vignetting.some((v) => !Number.isFinite(v) || v >= 1)) p.push(`Field ${n}: vignetting factors must be finite and less than 1.`);
  });
  if (!s.fields.some((f) => f.weight > 0)) p.push('At least one field weight must be positive.');
  s.surfaces.forEach((x, i) => {
    const n = i + 1;
    if (x.label.length > 256) p.push(`Surface ${n}: label is longer than 256 characters.`);
    if (!(x.aperture > 0)) p.push(`Surface ${n}: clear radius must be positive.`);
    if (x.even.length > 16) p.push(`Surface ${n}: at most 16 even coefficients.`);
    if (x.odd.length > 20) p.push(`Surface ${n}: at most 20 odd coefficients.`);
    const g = x.gap_solve;
    if (g.kind === 'pickup' && g.source >= s.surfaces.length)
      p.push(`Surface ${n}: pickup source must select an existing gap.`);
    else if (g.kind === 'pickup' && g.source === i)
      p.push(`Surface ${n}: pickup source must be another gap.`);
    if (g.kind === 'overall_length') {
      if (g.first >= g.last || g.last > s.surfaces.length)
        p.push(`Surface ${n}: overall-length interval must be a non-empty range of existing gaps.`);
      else if (i < g.first || i >= g.last)
        p.push(`Surface ${n}: overall-length interval must include its own gap.`);
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
