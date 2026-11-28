// Drawings carried by agent tool results, lifted out for the chat panel.
//
// Hissabeth tools return a drawing in one of three shapes:
// - bench_export_blueprint: text content holding JSON {filename, mime,
//   content, encoding?}, where content is SVG text or base64 PNG/PDF bytes;
// - artifact_fetch: a text metadata block, then MCP image content
//   {type: 'image', data, mimeType} or an embedded resource
//   {type: 'resource', resource: {uri, mimeType, text | blob}};
// - the Claude SDK repeats MCP images as {type: 'image', source: {type:
//   'base64', media_type, data}}.
// Results reach the bridge as a Codex `item.result` ({content: [...]}), as a
// Claude `tool_result` content array, or as a bare string.
//
// Every drawing is also written to <workspace>/chat-figures/<sha256>.<ext>, so
// a drawing too large to inline in the conversation history stays viewable
// and downloadable from its file (the app reads it with `chat_figure`).
// bench_lab_check results become a compact assertion summary (`ChatChecks`).

import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/** A drawing for the chat panel. `data` is base64 and inlined only up to
 * FIGURE_LIMIT; `file` names the stored copy under chat-figures. A drawing
 * that is neither inlined nor stored is marked `omitted`. */
export interface ChatFigure {
    mime: string;
    filename: string;
    bytes: number;
    data?: string;
    file?: string;
    omitted?: 'too_large';
    /** Workspace revision the drawing was made from, and whether it shows the
     * saved bench (false: a draft passed to the tool). */
    revision?: number;
    saved?: boolean;
}

/** One failed or undecided assertion, interrupted branch or lab issue, with
 * where to find it. */
export interface ChatCheckItem {
    kind: 'assertion' | 'branch' | 'issue';
    label: string;
    detail: string;
    severity: 'error' | 'warning';
    element?: string;
    branch?: string;
}

/** A zone footprint, fold length or relay matrix read off a lab check. */
export interface ChatReading {
    kind: 'zone' | 'fold' | 'relay';
    label: string;
    detail: string;
    /** In bounds, unbroken, and every relay check passed. */
    ok: boolean;
    element?: string;
    branch?: string;
}

/** The summary of a bench_lab_check result. */
export interface ChatChecks {
    passed: boolean;
    readiness: number;
    assertions: { pass: number; fail: number; indeterminate: number; unreachable: number };
    errors: number;
    warnings: number;
    items: ChatCheckItem[];
    /** Items beyond CHECK_ITEMS left out of the summary. */
    more: number;
    /** Up to READING_ITEMS readings and how many more were left out; absent
     * when the bench declares no zones, folds or relays. */
    readings?: ChatReading[];
    readingsMore?: number;
    revision?: number;
    saved?: boolean;
}

/** Image types the panel shows inline; anything else (PDF, DXF) stays in
 * the raw tool result. */
export const FIGURE_TYPES = ['image/svg+xml', 'image/png', 'image/jpeg', 'image/webp'];
/** Largest base64 payload inlined in a chat entry (about 1.1 MB of image);
 * larger drawings are read back from their stored file. */
export const FIGURE_LIMIT = 1_500_000;
/** Largest base64 payload accepted at all (about 48 MB of image). */
export const FIGURE_MAX = 64_000_000;
/** Failure items kept in a check summary. */
export const CHECK_ITEMS = 24;
/** Zone, fold and relay readings listed in a chat check summary. */
export const READING_ITEMS = 12;
const EXTENSIONS: Record<string, string> = {
    'image/svg+xml': 'svg',
    'image/png': 'png',
    'image/jpeg': 'jpg',
    'image/webp': 'webp',
};

const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;

/** The Hissabeth tool name without the Claude MCP prefix. */
export function toolName(name: string): string {
    return name.replace(/^mcp__hissabeth__/, '');
}

type Block = Record<string, unknown>;

function blocks(result: unknown): Block[] {
    if (typeof result === 'string') return [{ type: 'text', text: result }];
    if (Array.isArray(result))
        return result.filter((b): b is Block => !!b && typeof b === 'object');
    if (result && typeof result === 'object' && 'content' in result)
        return blocks((result as { content: unknown }).content);
    return [];
}

function parse(text: unknown): Block | undefined {
    if (typeof text !== 'string' || !text.trimStart().startsWith('{')) return undefined;
    try {
        const value: unknown = JSON.parse(text);
        return value && typeof value === 'object' && !Array.isArray(value)
            ? (value as Block)
            : undefined;
    } catch {
        return undefined;
    }
}

function figure(mime: unknown, base64: unknown, filename: unknown): ChatFigure | undefined {
    if (typeof mime !== 'string' || !FIGURE_TYPES.includes(mime)) return undefined;
    if (typeof base64 !== 'string' || !BASE64.test(base64)) return undefined;
    const name =
        typeof filename === 'string' && filename ? filename : `figure.${mime.split(/[/+]/)[1]}`;
    const bytes = Math.floor((base64.length * 3) / 4) - (base64.match(/=*$/)?.[0].length ?? 0);
    if (base64.length > FIGURE_MAX) return { mime, filename: name, bytes, omitted: 'too_large' };
    return { mime, filename: name, bytes, data: base64 };
}

const encode = (text: string) => Buffer.from(text, 'utf8').toString('base64');

/** The drawing a tool result carries, if any. */
export function figureOf(result: unknown): ChatFigure | undefined {
    const content = blocks(result);
    let filename: unknown;
    let origin: Pick<ChatFigure, 'revision' | 'saved'> = {};
    const stamp = (found: ChatFigure) => ({ ...found, ...origin });
    for (const block of content) {
        const meta = block.type === 'text' ? parse(block.text) : undefined;
        if (!meta) continue;
        filename ??= meta.filename;
        if (origin.revision === undefined && typeof meta.revision === 'number')
            origin = {
                revision: meta.revision,
                ...(typeof meta.saved === 'boolean' ? { saved: meta.saved } : {}),
            };
        // A direct export: the drawing itself, as text or base64.
        if (typeof meta.content === 'string' && typeof meta.mime === 'string') {
            const data = meta.encoding === 'base64' ? meta.content : encode(meta.content);
            const found = figure(meta.mime, data, meta.filename);
            if (found) return stamp(found);
        }
    }
    for (const block of content) {
        if (block.type === 'image') {
            const source = block.source as Block | undefined;
            const found = source
                ? figure(source.media_type, source.data, filename)
                : figure(block.mimeType, block.data, filename);
            if (found) return stamp(found);
        }
        if (block.type === 'resource' && block.resource && typeof block.resource === 'object') {
            const resource = block.resource as Block;
            const name =
                typeof resource.uri === 'string' ? resource.uri.split('/').pop() : undefined;
            const data = typeof resource.text === 'string' ? encode(resource.text) : resource.blob;
            const found = figure(resource.mimeType, data, filename ?? name);
            if (found) return stamp(found);
        }
    }
    return undefined;
}

/** Writes the drawing to `dir` under its content hash and inlines its data only
 * up to FIGURE_LIMIT. Without a stored copy a large drawing is `omitted`. */
export function storeFigure(found: ChatFigure, dir: string | undefined): ChatFigure {
    if (!found.data) return found;
    let file: string | undefined;
    if (dir)
        try {
            const bytes = Buffer.from(found.data, 'base64');
            const hash = createHash('sha256').update(bytes).digest('hex').slice(0, 32);
            file = `${hash}.${EXTENSIONS[found.mime]}`;
            mkdirSync(dir, { recursive: true });
            writeFileSync(join(dir, file), bytes);
        } catch {
            file = undefined;
        }
    const { data, ...rest } = found;
    if (data.length <= FIGURE_LIMIT) return { ...rest, data, ...(file ? { file } : {}) };
    return file ? { ...rest, file } : { ...rest, omitted: 'too_large' };
}

/** A path location in a lab check: a break or a comparison port. */
interface Located {
    element?: string;
    port?: string;
    reason?: string;
    detail?: string;
}
/** The parts of a bench_lab_check report the summary reads. */
interface LabReport {
    passed?: unknown;
    readiness_score?: unknown;
    revision?: unknown;
    saved?: unknown;
    counts?: { errors?: unknown; warnings?: unknown };
    assertions: {
        assertion?: unknown;
        kind?: unknown;
        status?: unknown;
        branches?: string[];
        at?: Located;
        break?: Located;
        unit?: unknown;
        target?: unknown;
        tolerance?: unknown;
        residual?: unknown;
        basis?: { kind?: unknown };
    }[];
    branches?: { id: string; break?: Located }[];
    zones?: {
        id: string;
        label?: string;
        area_mm2?: unknown;
        bounds_mm?: [[number, number], [number, number]];
        members?: string[];
        out_of_bounds?: string[];
    }[];
    folds?: {
        id: string;
        from?: string;
        break?: Located;
        paths?: {
            source?: string;
            wavelength_nm?: unknown;
            length_mm?: unknown;
            opl_mm?: unknown;
            break?: Located;
        }[];
    }[];
    relays?: {
        id: string;
        branch?: string;
        abcd?: number[] | null;
        pupil_stop?: string | null;
        pupil_target?: string | null;
        pupil_image_mm?: number | null;
        break?: Located;
        checks?: { kind?: unknown; value?: unknown; passed?: unknown }[];
    }[];
    issues?: {
        severity?: unknown;
        code?: unknown;
        message?: unknown;
        element_id?: string | null;
        branch?: string | null;
    }[];
}

function located(at: Located | undefined): string {
    return at && typeof at.element === 'string' ? `${at.element}:${at.port}` : '';
}
const stopped = (stop: Located) =>
    `${located(stop)} ${stop.reason}${stop.detail ? ` (${stop.detail})` : ''}`;

/** A reading for the summary: finite numbers to six significant digits. */
const num = (value: unknown): string =>
    typeof value === 'number' && Number.isFinite(value)
        ? String(Number(value.toPrecision(6)))
        : '—';

/** Zone footprints, fold lengths and relay matrices, passing or not. */
function readingsOf(report: LabReport): ChatReading[] {
    const readings: ChatReading[] = [];
    for (const z of report.zones ?? []) {
        const [[x0, y0], [x1, y1]] = z.bounds_mm ?? [
            [0, 0],
            [0, 0],
        ];
        const out = z.out_of_bounds ?? [];
        readings.push({
            kind: 'zone',
            label: String(z.id),
            detail: [
                z.label,
                `${num(z.area_mm2)} mm² (${num(x1 - x0)} × ${num(y1 - y0)} mm)`,
                `${z.members?.length ?? 0} members`,
                out.length ? `outside: ${out.join(', ')}` : '',
            ]
                .filter(Boolean)
                .join(' · '),
            ok: !out.length,
            // The first stray member, else the first member, locates the zone.
            ...((out[0] ?? z.members?.[0]) ? { element: out[0] ?? z.members?.[0] } : {}),
        });
    }
    for (const f of report.folds ?? []) {
        const paths = f.paths ?? [];
        const stop = f.break ?? paths.find((p) => p.break)?.break;
        const lengths = paths.map(
            (p) =>
                `${paths.length > 1 ? `${p.source} ${num(p.wavelength_nm)} nm: ` : ''}${p.break ? 'partial ' : ''}${num(p.length_mm)} mm (OPL ${num(p.opl_mm)} mm)`,
        );
        readings.push({
            kind: 'fold',
            label: String(f.id),
            detail: [...lengths, stop ? stopped(stop) : ''].filter(Boolean).join(' · '),
            ok: !stop,
            // Where the fold stopped, else where it starts.
            ...(stop?.element || f.from ? { element: stop?.element || f.from } : {}),
        });
    }
    for (const r of report.relays ?? []) {
        const checks = r.checks ?? [];
        const parts: string[] = [];
        if (r.abcd?.length === 4) {
            const [a, b, c, d] = r.abcd;
            parts.push(`ABCD [${num(a)} ${num(b)} mm; ${num(c)} /mm ${num(d)}]`);
        }
        if (r.pupil_stop) {
            parts.push(
                `pupil ${r.pupil_stop} → ${r.pupil_image_mm == null ? '∞' : `${num(r.pupil_image_mm)} mm`} from ${r.pupil_target ?? '—'}`,
            );
        }
        for (const c of checks) parts.push(`${c.kind} ${num(c.value)} ${c.passed ? '✓' : '✗'}`);
        if (r.break) parts.push(stopped(r.break));
        readings.push({
            kind: 'relay',
            label: String(r.id),
            detail: parts.join(' · '),
            ok: !r.break && checks.every((c) => c.passed),
            ...(r.break?.element ? { element: r.break.element } : {}),
            ...(r.branch ? { branch: r.branch } : {}),
        });
    }
    return readings;
}

/** The assertion summary of a bench_lab_check result, recognised by its shape. */
export function checksOf(result: unknown): ChatChecks | undefined {
    const report = blocks(result)
        .map((block) => (block.type === 'text' ? parse(block.text) : undefined))
        .find((meta) => meta && Array.isArray(meta.assertions) && 'readiness_score' in meta) as
        | LabReport
        | undefined;
    if (!report) return undefined;
    const items: ChatCheckItem[] = [];
    const assertions = { pass: 0, fail: 0, indeterminate: 0, unreachable: 0 };
    for (const a of report.assertions) {
        if (a.status === 'pass') {
            assertions.pass++;
            continue;
        }
        const pair = Array.isArray(a.branches) ? a.branches.join(' / ') : '';
        if (a.status === 'unreachable') {
            assertions.unreachable++;
            const stop = a.break;
            items.push({
                kind: 'assertion',
                severity: 'error',
                label: String(a.assertion),
                detail: `${a.kind} · ${pair} · ${stop ? stopped(stop) : 'unreachable'}`,
                element: stop?.element ?? a.at?.element,
                branch: a.branches?.[0],
            });
        } else if (a.status === 'indeterminate') {
            // The value's basis (a bound or an estimate) cannot decide the target.
            assertions.indeterminate++;
            const basis = String(a.basis?.kind ?? 'estimate').replace('_', ' ');
            items.push({
                kind: 'assertion',
                severity: 'warning',
                label: String(a.assertion),
                detail: `${a.kind} · ${pair} · indeterminate: ${basis}, residual ${a.residual} ${a.unit}, tolerance ±${a.tolerance} (target ${a.target})`,
                element: a.at?.element,
                branch: a.branches?.[0],
            });
        } else {
            assertions.fail++;
            items.push({
                kind: 'assertion',
                severity: 'error',
                label: String(a.assertion),
                detail: `${a.kind} · ${pair} · residual ${a.residual} ${a.unit}, tolerance ±${a.tolerance} (target ${a.target})`,
                element: a.at?.element,
                branch: a.branches?.[0],
            });
        }
    }
    for (const b of report.branches ?? []) {
        const stop = b.break;
        if (!stop) continue;
        items.push({
            kind: 'branch',
            severity: 'error',
            label: String(b.id),
            detail: stopped(stop),
            element: stop.element,
            branch: b.id,
        });
    }
    for (const issue of report.issues ?? []) {
        // Assertion issues repeat the assertion items above.
        if (issue.severity === 'info' || String(issue.code).startsWith('assertion_')) continue;
        items.push({
            kind: 'issue',
            severity: issue.severity === 'error' ? 'error' : 'warning',
            label: String(issue.code),
            detail: String(issue.message),
            ...(issue.element_id ? { element: issue.element_id } : {}),
            ...(issue.branch ? { branch: issue.branch } : {}),
        });
    }
    items.sort((a, b) => Number(a.severity === 'warning') - Number(b.severity === 'warning'));
    const readings = readingsOf(report);
    return {
        passed: !!report.passed,
        readiness: Number(report.readiness_score) || 0,
        assertions,
        errors: Number(report.counts?.errors) || 0,
        warnings: Number(report.counts?.warnings) || 0,
        items: items.slice(0, CHECK_ITEMS),
        more: Math.max(0, items.length - CHECK_ITEMS),
        ...(readings.length
            ? {
                  readings: readings.slice(0, READING_ITEMS),
                  readingsMore: Math.max(0, readings.length - READING_ITEMS),
              }
            : {}),
        ...(typeof report.revision === 'number' ? { revision: report.revision } : {}),
        ...(typeof report.saved === 'boolean' ? { saved: report.saved } : {}),
    };
}

/** The drawing and check summary a tool result carries, ready for a chat event. */
export function reviewOf(
    result: unknown,
    workspace: string | undefined,
): { figure?: ChatFigure; checks?: ChatChecks } {
    const found = figureOf(result);
    const checks = checksOf(result);
    return {
        ...(found
            ? { figure: storeFigure(found, workspace && join(workspace, 'chat-figures')) }
            : {}),
        ...(checks ? { checks } : {}),
    };
}
