<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import {
    Box,
    Crosshair,
    Download,
    Grid3x3,
    List,
    Minus,
    Maximize2,
    Plus,
    RefreshCw,
    RotateCcw,
    Save,
    Sparkles,
    Trash2,
    ZoomIn,
    ZoomOut,
} from 'lucide-vue-next';
import { callTool, errorMessage, settleArtifact } from '../api';
import { tr } from '../i18n';
import {
    benchPorts,
    benchElementPorts,
    benchRotate,
    type Bench,
    type BenchCatalogItem,
    type BenchDrawing,
    type BenchElement,
    META_KEY,
    metaValue,
    type BenchRegion,
    type BenchLabCheck,
    type BenchPolarization,
    type BenchTrace,
    type Vec3,
    type BlueprintFormat,
    type Layout3dOptions,
    PENDING_UNITS,
    pendingLayer,
    pendingValue,
    supplyPending,
} from '../bench';
import { profileCurve } from '../profile';
import { canonicalJson, clone } from '../validation';
import HeatMap from './HeatMap.vue';
import BenchLabPanel from './BenchLabPanel.vue';
import type { OpticalSystem, Paraxial } from '../types';

const props = defineProps<{
    system: OpticalSystem;
    revision: number;
    online: boolean;
    /** Kept for template compatibility; pending drafts are chained in automatically. */
    dirty?: boolean;
    busy: boolean;
    stale: boolean;
    refreshPending: boolean;
    /** Chains a valid pending draft, runs the operation with the current revision, then reloads. */
    mutate: (op: (revision: number, system: OpticalSystem) => Promise<unknown>) => Promise<boolean>;
    mode: '2d' | '3d';
    paraxial?: Paraxial | null;
    chatOpen?: boolean;
}>();
const emit = defineEmits<{ inspectGroup: []; chat: []; 'update:benchDirty': [dirty: boolean] }>();
const selected = defineModel<string>('selected', { default: '' });

// Local bench draft enables smooth 2D dragging, catalog placement & Auto-Aim with live preview before committing.
const draft = ref<Bench | null>(null);
const saving = ref(false);
const benchDirty = computed(() => {
    if (!draft.value && !props.system.bench) return false;
    return canonicalJson(draft.value) !== canonicalJson(props.system.bench ?? null);
});
const scene = computed(() => draft.value ?? props.system.bench ?? null);

// Canonical of the saved bench the local draft was based on. A chained sequential system_set replays
// that same base bench, which must not wipe a dirty local bench draft while a unified save runs.
let savedBenchBase = '';
watch(
    () => [props.revision, props.system.bench],
    () => {
        const incoming = canonicalJson(props.system.bench ?? null);
        if (saving.value) {
            savedBenchBase = incoming;
            return;
        }
        if (benchDirty.value && incoming === savedBenchBase) return;
        draft.value = props.system.bench ? clone(props.system.bench) : null;
        savedBenchBase = incoming;
    },
    { immediate: true },
);
watch(benchDirty, (d) => emit('update:benchDirty', d), { flush: 'post' });

const allEntities = computed(() => [
    ...(scene.value?.elements ?? []),
    ...(scene.value?.sources ?? []),
]);
const entity = computed(() => allEntities.value.find((e) => e.id === selected.value));
const selectedStage = computed(() =>
    scene.value?.stages?.find((stage) => stage.members.includes(selected.value)),
);
/** Height-layer filter: elements on other layers are dimmed. */
const layerFilter = ref('');
const periscopeMirrors = computed(
    () => new Set((scene.value?.periscopes ?? []).flatMap((p) => [p.lower, p.upper])),
);
/** A layered element's Z follows its layer, except on periscope mirrors. */
const zLocked = computed(() => {
    const e = entity.value;
    return !!e && 'kind' in e && !!e.layer && !periscopeMirrors.value.has(e.id);
});
function setRole(e: BenchElement, raw: string) {
    const role = raw.trim().toLowerCase();
    if (role) e.role = role;
    else delete e.role;
    schedulePreview();
}
/** Metadata rows keep their order while a key is renamed. */
function setMetaKey(e: BenchElement, from: string, raw: string) {
    const key = raw.trim().toLowerCase();
    if (
        !key ||
        key === from ||
        !e.metadata ||
        Object.prototype.hasOwnProperty.call(e.metadata, key)
    )
        return;
    e.metadata = Object.fromEntries(
        Object.entries(e.metadata).map(([k, v]) => [k === from ? key : k, v]),
    );
    schedulePreview();
}
function setMetaValue(e: BenchElement, key: string, raw: string) {
    e.metadata = { ...e.metadata, [key]: metaValue(raw) };
    schedulePreview();
}
function addMeta(e: BenchElement) {
    const used = e.metadata ?? {};
    let n = 1;
    while (Object.prototype.hasOwnProperty.call(used, `note${n === 1 ? '' : n}`)) n++;
    e.metadata = { ...used, [`note${n === 1 ? '' : n}`]: '' };
    schedulePreview();
}
function removeMeta(e: BenchElement, key: string) {
    const rest = { ...e.metadata };
    delete rest[key];
    if (Object.keys(rest).length) e.metadata = rest;
    else delete e.metadata;
    schedulePreview();
}
function setLayer(e: BenchElement, id: string) {
    e.layer = id || null;
    const layer = scene.value?.layers?.find((l) => l.id === id);
    if (layer && !periscopeMirrors.value.has(e.id)) e.position[2] = layer.height_mm;
    schedulePreview();
}
/** Zone outlines on the table surface. */
const zoneOutlines = computed(() => {
    const b = scene.value;
    if (!b) return [];
    const z = b.origin[2];
    const drawn = new Map((plan.value?.zones ?? []).map((zone) => [zone.id, zone.outline]));
    return (b.zones ?? []).map((zone) => {
        const pts: [number, number][] =
            drawn.get(zone.id) ??
            zone.polygon_mm ??
            (zone.rect
                ? [
                      zone.rect.min,
                      [zone.rect.max[0], zone.rect.min[1]],
                      zone.rect.max,
                      [zone.rect.min[0], zone.rect.max[1]],
                  ]
                : []);
        const ring = pts.map(([x, y]) => [x, y, z] as Vec3);
        const c = pts.reduce(
            (a, p) => [a[0] + p[0] / pts.length, a[1] + p[1] / pts.length],
            [0, 0],
        );
        return {
            id: zone.id,
            label: zone.label || zone.id,
            points: path([...ring, ...ring.slice(0, 1)]),
            center: project([c[0], c[1], z]),
            outside: labCheck.value?.zones?.find((r) => r.id === zone.id)?.out_of_bounds.length,
        };
    });
});
/** Vertical periscope legs between paired mirrors. */
const periscopeLegs = computed(() => {
    const b = scene.value;
    if (!b) return [];
    return (b.periscopes ?? []).flatMap((p) => {
        const lo = b.elements.find((e) => e.id === p.lower);
        const hi = b.elements.find((e) => e.id === p.upper);
        return lo && hi ? [{ id: p.id, points: path([lo.position, hi.position]) }] : [];
    });
});
/** Referenced sequential prescription summary for the selected lens group. */
const groupSummary = computed(() => {
    const e = entity.value;
    if (!e || !('kind' in e) || e.kind !== 'lens_group') return null;
    const surfaces = props.system.surfaces;
    return {
        surfaces: surfaces.length,
        efl: props.paraxial?.efl ?? null,
        maxAperture: surfaces.reduce((m, s) => Math.max(m, s.aperture), 0),
    };
});
const trace = ref<BenchTrace | null>(null);
const labEdit = ref(0);
const labInput = computed(() => ({
    revision: props.revision,
    bench: scene.value,
    edit: labEdit.value,
}));
const labSnapshot = shallowRef<{ input: typeof labInput.value; report: BenchLabCheck }>();
const labRevisionMismatch = ref(false);
/** The shared plan drawing, with each part's position when it was drawn. */
const planSnapshot = shallowRef<{ drawing: BenchDrawing; positions: Map<string, Vec3> }>();
const labCheck = computed(() => labSnapshot.value?.report ?? null);
const labStale = computed(
    () =>
        labSnapshot.value?.input !== labInput.value ||
        labRevisionMismatch.value ||
        !props.online ||
        props.stale ||
        props.refreshPending,
);
const pending = ref(false);
const labPending = ref(false);
const labError = ref('');
const error = ref('');
const statusMsg = ref('');
const branch = ref('');
const snapHoles = ref(true);
const showGaussian = ref(true);
const showTree = ref(true);
const hoverCoord = ref<{ x: number; y: number; col: number; row: number } | null>(null);
const connPort = ref('reflected');
const connTo = ref('');
const templateChoice = ref('mach_zehnder');
/** Export choice: a blueprint format, coordinates CSV, or the 3-D layout as PDF or PNG. */
const exportFormat = ref<BlueprintFormat | 'csv' | 'layout3d-pdf' | 'layout3d-png'>('svg');
/** PNG resolution in pixels per drawing millimetre (4 px/mm ≈ 102 dpi). */
const exportPxPerMm = ref(4);

const svg = ref<SVGSVGElement>();
const yaw = ref(-25);
const pitch = ref(35);
const zoom = ref(1);
const pan = ref<[number, number]>([0, 0]);
let token = 0;
let labToken = 0;
let alive = true;

const TEMPLATES = [
    'mach_zehnder',
    'delay_line',
    'three_arm',
    'michelson',
    'polarized_beam_combiner',
    'beam_expander_focus',
    'periscope_routing',
    'modal_hybrid',
] as const;

const canCommit = computed(
    () => props.online && !props.busy && !props.stale && !props.refreshPending && !saving.value,
);

function applyStagePoses(bench: Bench, poses: BenchTrace['stage_poses']): void {
    if (!poses) return;
    const elements = new Map(bench.elements.map((element) => [element.id, element]));
    for (const [id, pose] of Object.entries(poses)) {
        const element = elements.get(id);
        if (element) {
            element.position = pose.position;
            element.rotation = pose.rotation;
        }
    }
}

async function run(): Promise<void> {
    const request = ++token;
    const revision = props.revision;
    const active = scene.value;
    error.value = '';
    if (!active || !props.online) {
        trace.value = null;
        pending.value = false;
        return;
    }
    pending.value = true;
    try {
        const result = await callTool('bench_trace', benchDirty.value ? { bench: active } : {});
        if (
            alive &&
            request === token &&
            props.online &&
            active === scene.value &&
            revision === props.revision &&
            result.revision === revision
        ) {
            if (active === draft.value) applyStagePoses(active, result.stage_poses);
            trace.value = result;
        }
    } catch (e) {
        if (alive && request === token) {
            trace.value = null;
            error.value = errorMessage(e);
        }
    } finally {
        if (alive && request === token) pending.value = false;
    }
}

async function runLabCheck(): Promise<void> {
    const request = ++labToken;
    const input = labInput.value;
    labError.value = '';
    if (!input.bench || !props.online) {
        if (!input.bench) labSnapshot.value = undefined;
        labPending.value = false;
        return;
    }
    const current = () => alive && request === labToken && props.online && input === labInput.value;
    labPending.value = true;
    try {
        const report = await callTool(
            'bench_lab_check',
            benchDirty.value ? { bench: input.bench } : {},
        );
        if (!current()) return;
        labRevisionMismatch.value = report.revision !== input.revision;
        if (labRevisionMismatch.value) labError.value = tr('benchLab.revisionChanged');
        else labSnapshot.value = { input, report };
    } catch (e) {
        if (current()) labError.value = errorMessage(e);
    } finally {
        if (alive && request === labToken) labPending.value = false;
    }
}

let planToken = 0;
let planZoomTimer: ReturnType<typeof setTimeout> | undefined;
/** Label height asked of bench_drawing so labels read at about 11 px. */
const planTextMm = () => Math.min(50, Math.max(1, 11 / factor.value));
async function runDrawing(): Promise<void> {
    const request = ++planToken;
    const input = labInput.value;
    if (props.mode !== '2d' || !input.bench || !props.online) return;
    const text_mm = planTextMm();
    try {
        const drawing = await callTool(
            'bench_drawing',
            benchDirty.value ? { bench: input.bench, text_mm } : { text_mm },
        );
        // Any edit replaces labInput at once, so a reply to an older bench is dropped.
        if (
            !alive ||
            request !== planToken ||
            input !== labInput.value ||
            drawing.revision !== input.revision
        )
            return;
        // The engine drew stage-solved positions; offsets are measured from those.
        const positions = new Map<string, Vec3>(drawing.positions.map((p) => [p.id, p.position]));
        planSnapshot.value = { drawing, positions };
    } catch {
        // Keep the last drawing; the lab check reports why the bench cannot be drawn.
    }
}

/** Engine 3-D layout shown beside the interactive 3D view, at its view. */
const showRender = ref(true);
const renderUrl = ref('');
/** The draft and view the shown render was drawn for. */
const renderFor = shallowRef<{ input: typeof labInput.value; view: string }>();
let renderToken = 0;
let renderTimer: ReturnType<typeof setTimeout> | undefined;
/** Width of the engine page's drawing area (mm) over the 1000-unit view box. */
const PAGE_MM_PER_UNIT = 396 / 1000;
const round = (n: number) => Number(n.toFixed(2));
/**
 * The interactive view as layout3d options: yaw wrapped into ±180°, the same
 * zoom over the fitted scale, and the pan as the same fraction of the drawing
 * width.
 */
function currentView(): Layout3dOptions {
    const azimuth = ((((yaw.value + 180) % 360) + 360) % 360) - 180;
    return {
        view: {
            azimuth_deg: round(azimuth),
            elevation_deg: round(pitch.value),
            zoom: Number(Math.min(50, Math.max(0.05, zoom.value)).toPrecision(4)),
            pan_mm: [
                round(pan.value[0] * PAGE_MM_PER_UNIT),
                round(pan.value[1] * PAGE_MM_PER_UNIT),
            ],
        },
    };
}
const viewKey = computed(() => JSON.stringify(currentView()));
/** The shown render no longer matches the draft or the view. */
const renderStale = computed(
    () => renderFor.value?.input !== labInput.value || renderFor.value?.view !== viewKey.value,
);
async function runRender(): Promise<void> {
    const request = ++renderToken;
    const input = labInput.value;
    const view = viewKey.value;
    if (props.mode !== '3d' || !showRender.value || !input.bench || !props.online) return;
    try {
        const out = await callTool('bench_export_blueprint', {
            ...(benchDirty.value ? { bench: input.bench } : {}),
            format: 'layout3d',
            ...currentView(),
        });
        if (!alive || request !== renderToken || input !== labInput.value) return;
        URL.revokeObjectURL(renderUrl.value);
        renderUrl.value = URL.createObjectURL(new Blob([out.content], { type: out.mime }));
        renderFor.value = { input, view };
    } catch {
        // Keep the last render, marked stale; the lab check reports why the bench cannot be drawn.
    }
}
function scheduleRender(): void {
    clearTimeout(renderTimer);
    renderTimer = setTimeout(() => void runRender(), 250);
}
// Every draft change (edits, discard, a new revision) replaces labInput.
watch(
    () => [labInput.value, props.online, props.mode, showRender.value, viewKey.value],
    scheduleRender,
);

let debounceTimer: ReturnType<typeof setTimeout> | undefined;
let stagePreviewTimer: ReturnType<typeof setTimeout> | undefined;
function schedulePreview(stageMotion = false): void {
    labEdit.value++;
    if (stageMotion && stagePreviewTimer === undefined) {
        stagePreviewTimer = setTimeout(() => {
            stagePreviewTimer = undefined;
            void run();
        }, 40);
    }
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
        if (!stageMotion) void run();
        void runLabCheck();
        void runDrawing();
    }, 120);
}

watch(
    () => [props.revision, props.online],
    () => {
        branch.value = '';
        if (!allEntities.value.some((e) => e.id === selected.value)) selected.value = '';
        void run();
        void runLabCheck();
    },
    { immediate: true },
);

onBeforeUnmount(() => {
    alive = false;
    token++;
    labToken++;
    planToken++;
    renderToken++;
    clearTimeout(renderTimer);
    URL.revokeObjectURL(renderUrl.value);
    clearTimeout(debounceTimer);
    clearTimeout(stagePreviewTimer);
    clearTimeout(planZoomTimer);
});

/** Placeholders a reference image left for the user, with the value typed so far. */
const pendingInputs = computed(() =>
    (scene.value?.pending ?? []).map((input, index) => {
        const unit = PENDING_UNITS[input.field];
        return {
            target: input.target,
            note: input.note,
            index,
            key: `${input.target}:${input.field}`,
            label: `${input.target} · ${input.field}${unit ? ` (${unit})` : ''}`,
            value: pendingValue(scene.value!, input),
            layer: pendingLayer(scene.value!, input)?.id,
        };
    }),
);
const pendingEntries = ref<Record<string, string>>({});
function supplyInput(index: number, key: string, fallback: number | undefined): void {
    if (saving.value) return;
    const typed = pendingEntries.value[key];
    const value = typed === undefined || typed === '' ? fallback : Number(typed);
    const bench = ensureDraft();
    if (!bench || value === undefined || !supplyPending(bench, index, value)) return;
    delete pendingEntries.value[key];
    schedulePreview();
}

function ensureDraft(): Bench | null {
    if (!draft.value && props.system.bench) draft.value = clone(props.system.bench);
    return draft.value;
}

function setStagePosition(id: string, dof: 'position' | 'pzt', value: number): void {
    if (saving.value) return;
    const stage = ensureDraft()?.stages?.find((candidate) => candidate.id === id);
    if (!stage) return;
    if (dof === 'position') stage.position_mm = value;
    else if (stage.pzt) stage.pzt.value_um = value;
    schedulePreview(true);
}

async function loadTemplate(name: string): Promise<void> {
    if (!props.online) return;
    error.value = '';
    statusMsg.value = '';
    try {
        if (canCommit.value && !benchDirty.value) {
            const ok = await props.mutate((rev) =>
                callTool('bench_template', { name, apply: true, revision: rev }),
            );
            if (ok) statusMsg.value = tr('benchLab.templateLoaded', { name });
        } else {
            const res = await callTool('bench_template', { name, apply: false });
            draft.value = res.bench;
            statusMsg.value = tr('benchLab.templateStaged', { name });
            schedulePreview();
        }
    } catch (e) {
        error.value = errorMessage(e);
    }
}

async function runAutoAim(): Promise<void> {
    const active = ensureDraft();
    if (!active || !props.online) return;
    const input = canonicalJson(active);
    error.value = '';
    statusMsg.value = '';
    try {
        const res = await callTool('bench_auto_aim', {
            bench: active,
            snap_holes: snapHoles.value,
            apply: false,
        });
        if (!alive || canonicalJson(draft.value) !== input) return;
        const residuals = res.residuals.map((r) => {
            const separation =
                r.position_mm == null || r.angle_mrad == null
                    ? ''
                    : ` ${r.position_mm.toExponential(3)} mm / ${r.angle_mrad.toExponential(3)} mrad`;
            const interruption = r.break
                ? ` (${r.break.element} / ${r.break.port}, ${r.break.reason})`
                : '';
            return `${r.branch}${separation}${interruption}`;
        });
        if (!res.ok) {
            const ports = res.conflicts.map(({ element, port }) => `${element} / ${port}`);
            error.value = [
                tr('benchLab.autoAimConflict', { ports: ports.join(', ') }),
                ...residuals,
            ].join(' · ');
            return;
        }
        draft.value = res.bench;
        statusMsg.value = [
            tr('benchLab.autoAimDone', { n: res.adjusted_elements.length }),
            ...residuals,
        ].join(' · ');
        schedulePreview();
    } catch (e) {
        error.value = errorMessage(e);
    }
}

async function saveBench(): Promise<void> {
    if (!draft.value || !canCommit.value) return;
    saving.value = true;
    error.value = '';
    statusMsg.value = '';
    try {
        const active = draft.value;
        const next = clone(draft.value);
        const ok = await props.mutate(async (rev) => {
            const saved = await callTool('bench_set', { bench: next, revision: rev });
            if (saved.trace && active === draft.value) {
                token++;
                pending.value = false;
                applyStagePoses(active, saved.trace.stage_poses);
            }
        });
        if (ok) statusMsg.value = tr('benchLab.saved');
    } catch (e) {
        error.value = errorMessage(e);
    } finally {
        saving.value = false;
    }
}

function discardBench(): void {
    draft.value = props.system.bench ? clone(props.system.bench) : null;
    error.value = '';
    statusMsg.value = '';
    void run();
    void runLabCheck();
    // The drawing of the discarded draft must not linger; a newer request also
    // invalidates any still in flight.
    planSnapshot.value = undefined;
    void runDrawing();
}

function nextElementId(prefix: string): string {
    const used = new Set(allEntities.value.map((e) => e.id));
    for (let i = 1; i < 999; i++) {
        const candidate = `${prefix}${i}`;
        if (!used.has(candidate)) return candidate;
    }
    return `${prefix}_${Date.now()}`;
}

function snapCoord(value: number, originAxis: number, pitchMm: number): number {
    return originAxis + Math.round((value - originAxis) / pitchMm) * pitchMm;
}

function addFromCatalog(item: BenchCatalogItem): void {
    const b = ensureDraft();
    if (!b) return;
    const prefixMap: Record<string, string> = {
        mirror: 'M',
        curved_mirror: 'CM',
        lens: 'L',
        lens_group: 'G',
        splitter: 'BS',
        pbs: 'PBS',
        waveplate: 'WP',
        polarizer: 'POL',
        detector: 'D',
        aperture: 'AP',
        blocker: 'BD',
    };
    const id = nextElementId(prefixMap[item.kind] ?? 'E');
    const cx = snapCoord(b.origin[0] + b.table_size[0] * 0.5, b.origin[0], b.hole_pitch);
    const cy = snapCoord(b.origin[1] + b.table_size[1] * 0.5, b.origin[1], b.hole_pitch);
    const cz = b.sources[0]?.position[2] ?? b.origin[2] + 75;
    const elem: BenchElement = {
        id,
        label: item.name,
        kind: item.kind,
        position: [cx, cy, cz],
        rotation: null,
        aperture: item.aperture,
        body_radius: item.body_radius,
        focal_length: item.focal_length,
        reflectance: item.reflectance,
        transmittance: item.transmittance,
        mount: clone(item.mount),
        ...(item.retardance_waves != null ? { retardance_waves: item.retardance_waves } : {}),
        ...(item.fast_axis_deg != null ? { fast_axis_deg: item.fast_axis_deg } : {}),
        ...(item.extinction_ratio != null ? { extinction_ratio: item.extinction_ratio } : {}),
        ...(item.screw_tpi != null ? { screw_tpi: item.screw_tpi } : {}),
        ...(item.lever_arm_mm != null ? { lever_arm_mm: item.lever_arm_mm } : {}),
        ...(item.lens_form ? { lens_form: item.lens_form } : {}),
    };
    b.elements.push(elem);
    selected.value = id;
    statusMsg.value = tr('benchLab.addedPart', { id, part: item.part });
    schedulePreview();
}

function addBeamDumpAt(position: Vec3, direction: Vec3, branchName: string): void {
    const b = ensureDraft();
    if (!b) return;
    const id = nextElementId('BD');
    const yawDeg = (Math.atan2(-direction[1], -direction[0]) * 180) / Math.PI;
    b.elements.push({
        id,
        label: `Beam Dump (${branchName})`,
        kind: 'blocker',
        position: [position[0], position[1], position[2]],
        rotation: [0, 0, Number(yawDeg.toFixed(2))],
        aperture: 12.5,
        body_radius: 19,
        focal_length: 0,
        reflectance: 0,
        transmittance: 0,
        mount: { part: 'BT610/M', base_size: [40, 40], height_range: [45, 130] },
    });
    selected.value = id;
    statusMsg.value = tr('benchLab.dumpAdded', { id });
    schedulePreview();
}

function deleteSelectedElement(): void {
    const b = ensureDraft();
    if (!b || !selected.value || selectedStage.value) return;
    const idx = b.elements.findIndex((e) => e.id === selected.value);
    if (idx < 0) return;
    const removedId = b.elements[idx]!.id;
    b.elements.splice(idx, 1);
    b.connections = b.connections.filter((c) => c.from !== removedId && c.to !== removedId);
    // Its pending inputs go with it; the bench rejects a pending unknown target.
    if (b.pending) {
        b.pending = b.pending.filter((input) => input.target !== removedId);
        if (!b.pending.length) delete b.pending;
    }
    for (const key of Object.keys(pendingEntries.value))
        if (key.startsWith(`${removedId}:`)) delete pendingEntries.value[key];
    selected.value = '';
    schedulePreview();
}

function snapSelectedToHole(): void {
    const b = ensureDraft();
    if (!b || !selected.value || selectedStage.value) return;
    const target =
        b.elements.find((e) => e.id === selected.value) ??
        b.sources.find((s) => s.id === selected.value);
    if (!target) return;
    target.position[0] = snapCoord(target.position[0], b.origin[0], b.hole_pitch);
    target.position[1] = snapCoord(target.position[1], b.origin[1], b.hole_pitch);
    schedulePreview();
}

function updateSelectedPol(kind: string): void {
    const b = ensureDraft();
    if (!b) return;
    const src = b.sources.find((s) => s.id === selected.value);
    if (!src) return;
    const map: Record<string, BenchPolarization> = {
        unpolarized: { kind: 'unpolarized' },
        horizontal: { kind: 'horizontal' },
        vertical: { kind: 'vertical' },
        diagonal45: { kind: 'diagonal45' },
        right_circular: { kind: 'right_circular' },
        left_circular: { kind: 'left_circular' },
        linear: { kind: 'linear', angle_deg: 0 },
    };
    src.polarization = map[kind] ?? { kind: 'unpolarized' };
    schedulePreview();
}

const selectedPorts = computed(() => {
    const e = entity.value;
    if (!e) return [];
    return 'kind' in e ? benchElementPorts(e) : benchPorts('source');
});
const selectedConnections = computed(() =>
    (scene.value?.connections ?? []).filter((c) => c.from === selected.value),
);
watch(selectedPorts, (ports) => {
    if (ports.length && !ports.includes(connPort.value)) connPort.value = ports[0]!;
});
function addSelectedConnection(): void {
    const b = ensureDraft();
    if (!b || !selected.value || !connTo.value || connTo.value === selected.value) return;
    b.connections = b.connections.filter(
        (c) => !(c.from === selected.value && c.port === connPort.value),
    );
    b.connections.push({ from: selected.value, port: connPort.value, to: connTo.value });
    schedulePreview();
}
function removeConnection(from: string, port: string): void {
    const b = ensureDraft();
    if (!b) return;
    b.connections = b.connections.filter((c) => !(c.from === from && c.port === port));
    schedulePreview();
}

const holeDots = computed(() => {
    const b = scene.value;
    if (!b || props.mode !== '2d') return [];
    if (plan.value) return [];
    const [w, h] = b.table_size;
    const cols = Math.floor(w / b.hole_pitch) + 1;
    const rows = Math.floor(h / b.hole_pitch) + 1;
    if (cols * rows > 1400) return [];
    const [ox, oy, oz] = b.origin;
    const pts: [number, number][] = [];
    for (let c = 0; c < cols; c++) {
        for (let r = 0; r < rows; r++) {
            pts.push(project([ox + c * b.hole_pitch, oy + r * b.hole_pitch, oz]));
        }
    }
    return pts;
});

/** The ray split path is the selected one or leads into it. */
const onBranch = (path: string) =>
    !branch.value || path === branch.value || branch.value.startsWith(path + '/');
const visibleRays = computed(() => trace.value?.segments.filter((s) => onBranch(s.branch)) ?? []);
const failures = computed(
    () => trace.value?.terminations.filter((t) => t.reason !== 'detected') ?? [],
);
const problemIds = computed(
    () =>
        new Set(
            (labStale.value ? [] : (labCheck.value?.issues ?? []))
                .filter((i) => i.severity === 'error' || i.severity === 'warning')
                .map((i) => i.element_id)
                .filter((id): id is string => !!id),
        ),
);

const fixed = (n: number) => Number(n.toPrecision(6)).toString();
const basis = (point: Vec3): [number, number] => {
    const b = scene.value;
    if (!b) return [0, 0];
    const x = point[0] - b.origin[0] - b.table_size[0] / 2;
    const y = point[1] - b.origin[1] - b.table_size[1] / 2;
    const z = point[2] - b.origin[2];
    if (props.mode === '2d') return [x, y];
    const a = (yaw.value * Math.PI) / 180;
    const p = (pitch.value * Math.PI) / 180;
    return [
        Math.cos(a) * x - Math.sin(a) * y,
        Math.sin(a) * Math.sin(p) * x + Math.cos(a) * Math.sin(p) * y + Math.cos(p) * z,
    ];
};
const factor = computed(() => {
    if (!scene.value) return 1;
    const [w, h] = scene.value.table_size;
    return Math.min(840 / (w + h * 0.5), 450 / (h + w * 0.3)) * zoom.value;
});
const project = (p: Vec3): [number, number] => {
    const [x, y] = basis(p);
    return [500 + x * factor.value + pan.value[0], 340 - y * factor.value + pan.value[1]];
};
const path = (points: Vec3[]) => points.map((p) => project(p).join(',')).join(' ');
/** The plan drawing in 2D mode, without parts that have since been removed. */
const plan = computed(() => {
    const snapshot = planSnapshot.value;
    if (props.mode !== '2d' || !snapshot || !scene.value) return null;
    const ids = new Set(allEntities.value.map((e) => e.id));
    const d = snapshot.drawing;
    return {
        ...d,
        symbols: d.symbols.filter((symbol) => ids.has(symbol.id)),
        labels: d.labels.filter(
            (label) => !['element', 'source'].includes(label.kind) || ids.has(label.target),
        ),
    };
});
/** Every hole of the shared drawing as one path of circles: the same holes as the SVG blueprint. */
const planHolePath = computed(() => {
    const d = plan.value;
    if (!d) return '';
    const r = Math.max(1, d.hole_radius * factor.value);
    return d.holes
        .map((hole) => {
            const [x, y] = flat(hole);
            return `M${fixed(x - r)} ${fixed(y)}a${fixed(r)} ${fixed(r)} 0 1 0 ${fixed(2 * r)} 0a${fixed(r)} ${fixed(r)} 0 1 0 ${fixed(-2 * r)} 0`;
        })
        .join('');
});
const planBeams = computed(() => (plan.value?.beams ?? []).filter((beam) => onBranch(beam.path)));
/** Dash pattern scaled from mm to screen, kept legible when zoomed out. */
const dashPattern = (dash: number[]) =>
    dash.length ? dash.map((d) => fixed(d * Math.max(factor.value, 2.7))).join(' ') : undefined;
const planSymbols = computed(() => new Map((plan.value?.symbols ?? []).map((s) => [s.id, s])));
const flat = (p: [number, number]) => project([p[0], p[1], scene.value?.origin[2] ?? 0]);
const flatPath = (points: [number, number][]) => points.map((p) => flat(p).join(',')).join(' ');
/** Screen offset of a part dragged since the drawing was made, so its symbol follows it. */
const planShift = (id: string): string | undefined => {
    const was = planSnapshot.value?.positions.get(id);
    const now = allEntities.value.find((e) => e.id === id)?.position;
    if (!was || !now || (was[0] === now[0] && was[1] === now[1])) return undefined;
    const f = factor.value;
    return `translate(${(now[0] - was[0]) * f} ${-(now[1] - was[1]) * f})`;
};
const planLabels = computed(() => {
    const d = plan.value;
    if (!d) return [];
    const em = d.text_mm;
    return d.labels.map((label) => {
        const near: [number, number] = [
            Math.min(Math.max(label.anchor[0], label.corner[0]), label.corner[0] + label.size[0]),
            Math.min(Math.max(label.anchor[1], label.corner[1]), label.corner[1] + label.size[1]),
        ];
        const mark = d.assertions.find((a) => a.id === label.target);
        const tone =
            label.kind === 'assertion'
                ? mark?.passed
                    ? 'pass'
                    : mark?.text === 'indeterminate'
                      ? 'indeterminate'
                      : 'fail'
                : label.kind;
        return {
            kind: label.kind,
            target: label.target,
            text: label.text,
            leader: label.leader,
            at: flat([label.corner[0] + 0.3 * em, label.corner[1] + 0.35 * em]),
            leaderFrom: flat(label.anchor),
            leaderTo: flat(near),
            tone,
            shift: ['element', 'source', 'annotation'].includes(label.kind)
                ? planShift(label.target)
                : undefined,
        };
    });
});
const planRulers = computed(() => {
    const d = plan.value;
    if (!d) return [];
    const [x0, y0] = d.origin;
    return d.rulers
        .filter((tick) => tick.major)
        .map((tick) => {
            const along: [number, number] =
                tick.axis === 'x' ? [x0 + tick.value_mm, y0] : [x0, y0 + tick.value_mm];
            const [x, y] = flat(along);
            return { axis: tick.axis, value_mm: tick.value_mm, x, y };
        });
});
/** The shared drawing holds only the centre ray; a sample-ray path picked in the
 * branch filter that it does not cover keeps its traced rays. */
const raysShown = computed(() =>
    plan.value && (!branch.value || planBeams.value.length)
        ? visibleRays.value.filter(
              (ray) =>
                  !['hit', 'escaped', 'group_surface'].includes(ray.status) ||
                  ray.branch.includes(':scatter') ||
                  ray.branch.includes(':ghost'),
          )
        : visibleRays.value,
);
watch(
    () => [props.revision, props.online, props.mode],
    () => void runDrawing(),
    { immediate: true },
);
watch(factor, () => {
    const drawn = planSnapshot.value?.drawing.text_mm;
    if (!drawn || Math.abs(planTextMm() / drawn - 1) < 0.25) return;
    clearTimeout(planZoomTimer);
    planZoomTimer = setTimeout(() => void runDrawing(), 250);
});
const floor = computed(() => {
    const b = scene.value;
    if (!b) return '';
    const [x, y, z] = b.origin;
    return path([
        [x, y, z],
        [x + b.table_size[0], y, z],
        [x + b.table_size[0], y + b.table_size[1], z],
        [x, y + b.table_size[1], z],
    ]);
});
const grid = computed(() => {
    const b = scene.value;
    if (!b) return [];
    const [w, h] = b.table_size;
    const [x, y, z] = b.origin;
    const step = b.hole_pitch * Math.max(1, Math.ceil(Math.max(w, h) / b.hole_pitch / 60));
    const lines: Vec3[][] = [];
    for (let i = 0; i <= w; i += step)
        lines.push([
            [x + i, y, z],
            [x + i, y + h, z],
        ]);
    for (let j = 0; j <= h; j += step)
        lines.push([
            [x, y + j, z],
            [x + w, y + j, z],
        ]);
    return lines;
});

const gaussianPolygons = computed(() => {
    if (!showGaussian.value || labStale.value || !labCheck.value?.gaussian.segments.length)
        return [];
    return labCheck.value.gaussian.segments
        .map((seg) => {
            const p0 = project(seg.from);
            const p1 = project(seg.to);
            const dx = p1[0] - p0[0];
            const dy = p1[1] - p0[1];
            const len = Math.hypot(dx, dy);
            if (len < 1e-3) return '';
            const nx = -dy / len;
            const ny = dx / len;
            const r0 = Math.max(2, seg.w_from * factor.value);
            const r1 = Math.max(2, seg.w_to * factor.value);
            return [
                `${(p0[0] + nx * r0).toFixed(1)},${(p0[1] + ny * r0).toFixed(1)}`,
                `${(p1[0] + nx * r1).toFixed(1)},${(p1[1] + ny * r1).toFixed(1)}`,
                `${(p1[0] - nx * r1).toFixed(1)},${(p1[1] - ny * r1).toFixed(1)}`,
                `${(p0[0] - nx * r0).toFixed(1)},${(p0[1] - ny * r0).toFixed(1)}`,
            ].join(' ');
        })
        .filter(Boolean);
});

function setRotation(element: BenchElement, axis: 0 | 1 | 2, event: Event): void {
    if (!element.rotation) return;
    element.rotation[axis] = Number((event.target as HTMLInputElement).value);
    schedulePreview();
}

function shape(e: BenchElement, radius = e.body_radius): string {
    const rotation = e.rotation;
    if (!rotation) return '';
    return path(
        Array.from({ length: 33 }, (_, i) => {
            const a = (i * Math.PI) / 16;
            const v = benchRotate([0, radius * Math.cos(a), radius * Math.sin(a)], rotation);
            return e.position.map((x, j) => x + v[j]!) as Vec3;
        }),
    );
}
function regionOutline(e: BenchElement, region: BenchRegion): string {
    const rotation = e.rotation;
    if (!rotation) return '';
    const s = region.shape;
    const local: [number, number][] =
        s.kind === 'circle'
            ? Array.from({ length: 25 }, (_, i) => {
                  const a = (i * Math.PI) / 12;
                  return [
                      s.center[0] + s.radius * Math.cos(a),
                      s.center[1] + s.radius * Math.sin(a),
                  ];
              })
            : (
                  [
                      [-1, -1],
                      [1, -1],
                      [1, 1],
                      [-1, 1],
                      [-1, -1],
                  ] as const
              ).map(([a, b]) => [
                  s.center[0] + (a * s.size[0]) / 2,
                  s.center[1] + (b * s.size[1]) / 2,
              ]);
    return path(
        local.map(([y, z]) => {
            const v = benchRotate([0, y, z], rotation);
            return e.position.map((x, j) => x + v[j]!) as Vec3;
        }),
    );
}
function regionCenter(e: BenchElement, region: BenchRegion): [number, number] {
    const v = benchRotate(
        [0, region.shape.center[0], region.shape.center[1]],
        e.rotation ?? [0, 0, 0],
    );
    const p = project(e.position.map((x, j) => x + v[j]!) as Vec3);
    return [p[0], p[1]];
}
function mountLines(e: BenchElement): Vec3[][] {
    if (!e.mount || !scene.value || !e.rotation) return [];
    const [w, h] = e.mount.base_size;
    const mountYaw = e.rotation[2];
    const low = scene.value.origin[2];
    const high = Math.max(low, e.position[2] - e.body_radius);
    const corners = Array.from({ length: 8 }, (_, i) => {
        const v = benchRotate(
            [((i % 2 ? 1 : -1) * w) / 2, ((Math.floor(i / 2) % 2 ? 1 : -1) * h) / 2, 0],
            [0, 0, mountYaw],
        );
        return [e.position[0] + v[0], e.position[1] + v[1], i < 4 ? low : high] as Vec3;
    });
    return [
        [0, 1],
        [1, 3],
        [3, 2],
        [2, 0],
        [4, 5],
        [5, 7],
        [7, 6],
        [6, 4],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ].map(([a, b]) => [corners[a!]!, corners[b!]!]);
}

function groupCurves(e: BenchElement): Vec3[][] {
    const rotation = e.rotation;
    if (e.kind !== 'lens_group' || !rotation) return [];
    let z = 0;
    return props.system.surfaces.flatMap((surface) => {
        const curve = profileCurve(surface, z);
        z += surface.thickness;
        return [0, Math.PI / 2].map((angle) =>
            curve.map(([x, r]) => {
                const v = benchRotate([x, r * Math.cos(angle), r * Math.sin(angle)], rotation);
                return e.position.map((p, i) => p + v[i]!) as Vec3;
            }),
        );
    });
}

let camera: {
    x: number;
    y: number;
    yaw: number;
    pitch: number;
    pan: [number, number];
    translate: boolean;
} | null = null;

let dragging: {
    id: string;
    startClientX: number;
    startClientY: number;
    startPos: Vec3;
    moved: boolean;
} | null = null;

function beginDragEntity(event: PointerEvent, id: string, pos: Vec3): void {
    selected.value = id;
    if (props.mode !== '2d' || event.button !== 0 || selectedStage.value) return;
    dragging = {
        id,
        startClientX: event.clientX,
        startClientY: event.clientY,
        startPos: [pos[0], pos[1], pos[2]],
        moved: false,
    };
    svg.value?.setPointerCapture(event.pointerId);
    event.preventDefault();
}

function begin(event: PointerEvent): void {
    if (event.button !== 0 && event.button !== 1 && event.button !== 2) return;
    camera = {
        x: event.clientX,
        y: event.clientY,
        yaw: yaw.value,
        pitch: pitch.value,
        pan: [...pan.value],
        translate: event.shiftKey || props.mode === '2d' || event.button === 1,
    };
    svg.value?.setPointerCapture(event.pointerId);
    event.preventDefault();
}

function move(event: PointerEvent): void {
    if (dragging && scene.value && svg.value) {
        const b = ensureDraft();
        if (!b) return;
        const rect = svg.value.getBoundingClientRect();
        const svgScale = 1000 / Math.max(1, rect.width);
        const dxMm = ((event.clientX - dragging.startClientX) * svgScale) / factor.value;
        const dyMm = -((event.clientY - dragging.startClientY) * svgScale) / factor.value;
        if (Math.hypot(dxMm, dyMm) > 0.2) dragging.moved = true;
        let nx = dragging.startPos[0] + dxMm;
        let ny = dragging.startPos[1] + dyMm;
        if (snapHoles.value) {
            nx = snapCoord(nx, b.origin[0], b.hole_pitch);
            ny = snapCoord(ny, b.origin[1], b.hole_pitch);
        } else {
            nx = Number(nx.toFixed(2));
            ny = Number(ny.toFixed(2));
        }
        const target =
            b.elements.find((e) => e.id === dragging!.id) ??
            b.sources.find((s) => s.id === dragging!.id);
        if (target) {
            if (target.position[0] !== nx || target.position[1] !== ny) {
                dragging.moved = true;
                target.position[0] = nx;
                target.position[1] = ny;
                labEdit.value++;
            }
            hoverCoord.value = {
                x: nx,
                y: ny,
                col: Math.round((nx - b.origin[0]) / b.hole_pitch),
                row: Math.round((ny - b.origin[1]) / b.hole_pitch),
            };
        }
        return;
    }
    if (props.mode === '2d' && scene.value && svg.value && !camera) {
        const b = scene.value;
        const rect = svg.value.getBoundingClientRect();
        const sx = ((event.clientX - rect.left) / Math.max(1, rect.width)) * 1000;
        const sy = ((event.clientY - rect.top) / Math.max(1, rect.height)) * 600;
        const wx = (sx - 500 - pan.value[0]) / factor.value + b.origin[0] + b.table_size[0] / 2;
        const wy = (340 + pan.value[1] - sy) / factor.value + b.origin[1] + b.table_size[1] / 2;
        hoverCoord.value = {
            x: wx,
            y: wy,
            col: Math.round((wx - b.origin[0]) / b.hole_pitch),
            row: Math.round((wy - b.origin[1]) / b.hole_pitch),
        };
    }
    if (!camera) return;
    const dx = event.clientX - camera.x;
    const dy = event.clientY - camera.y;
    if (camera.translate) {
        const scale = 1000 / Math.max(1, svg.value!.getBoundingClientRect().width);
        pan.value = [camera.pan[0] + dx * scale, camera.pan[1] + dy * scale];
    } else {
        yaw.value = camera.yaw + dx * 0.35;
        pitch.value = Math.max(-85, Math.min(85, camera.pitch + dy * 0.25));
    }
}

function endPointer(): void {
    if (dragging?.moved) schedulePreview();
    dragging = null;
    camera = null;
}

const cell = (v: unknown) => '"' + String(v ?? '').replace(/"/g, '""') + '"';

function downloadBlob(content: BlobPart, filename: string, mime: string): void {
    const url = URL.createObjectURL(new Blob([content], { type: mime }));
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
}

async function exportDeliverable(): Promise<void> {
    const input = labInput.value;
    if (!props.online || !input.bench) return;
    error.value = '';
    try {
        if (exportFormat.value === 'csv') {
            const report = await callTool(
                'bench_assembly',
                benchDirty.value ? { bench: input.bench } : {},
            );
            const rows = [
                [
                    'id',
                    'kind',
                    'x_mm',
                    'y_mm',
                    'z_mm',
                    'roll_deg',
                    'pitch_deg',
                    'yaw_deg',
                    'hole_col',
                    'hole_row',
                    'mount',
                ],
                ...report.items.map((e) => [
                    e.id,
                    e.kind,
                    e.position[0],
                    e.position[1],
                    e.position[2],
                    e.rotation[0],
                    e.rotation[1],
                    e.rotation[2],
                    e.hole[0],
                    e.hole[1],
                    e.mount?.part ?? '',
                ]),
                ...report.sources.map((e) => [
                    e.id,
                    'source',
                    e.position[0],
                    e.position[1],
                    e.position[2],
                    '',
                    '',
                    '',
                    '',
                    '',
                    '',
                ]),
            ];
            downloadBlob(
                rows.map((r) => r.map(cell).join(',')).join('\r\n'),
                'optical-assembly.csv',
                'text/csv',
            );
            return;
        }
        const choice = exportFormat.value;
        const layout = choice === 'layout3d' || choice.startsWith('layout3d-');
        const format: BlueprintFormat =
            choice === 'layout3d-pdf' ? 'pdf' : choice === 'layout3d-png' ? 'png' : choice;
        // The 3-D layout follows the 3D view; from 2D it uses the iso preset.
        const layoutOptions: Layout3dOptions =
            props.mode === '3d' ? { layout3d: true, ...currentView() } : { layout3d: true };
        const params = {
            ...(benchDirty.value ? { bench: input.bench } : {}),
            ...(format === 'png' ? { px_per_mm: exportPxPerMm.value } : {}),
            ...(layout ? layoutOptions : {}),
        };
        const { id } = await callTool('artifact_start', {
            kind: 'blueprint',
            format,
            ...(Object.keys(params).length ? { params } : {}),
        });
        const status = await settleArtifact(id);
        if (status.state !== 'done') {
            error.value = status.error ?? status.state;
            return;
        }
        // Drawings carry their revision in the title block; refuse one that no longer matches.
        const drawing = ['svg', 'pdf', 'png', 'layout3d'].includes(format);
        if (drawing && (input !== labInput.value || status.revision !== input.revision)) {
            error.value = tr('benchLab.revisionChanged');
            return;
        }
        const out = await callTool('artifact_fetch', { id });
        const bytes = Uint8Array.from(atob(out.data), (c) => c.charCodeAt(0));
        downloadBlob(bytes, out.filename, out.mime);
        statusMsg.value = tr('benchLab.exported', { file: out.filename });
    } catch (e) {
        error.value = errorMessage(e);
    }
}

// Exposed so the surrounding layout (and the app toolbar) can drive the unified bench draft.
/** Shows a place a chat review points at: selects the element (or the start of
 * the named branch) and narrows the ray filter to the branch when the trace
 * has a split path of that name. Returns whether anything was found. */
function locate(target: { element?: string; branch?: string }): boolean {
    const named = scene.value?.branches?.find((b) => b.id === target.branch);
    const id = [target.element, named?.from.element].find(
        (candidate) => candidate && allEntities.value.some((e) => e.id === candidate),
    );
    if (id) selected.value = id;
    const wanted = target.branch;
    const traced =
        !!wanted &&
        !!trace.value?.terminations.some(
            (t) => t.branch === wanted || t.branch.startsWith(wanted + '/'),
        );
    branch.value = traced ? wanted : '';
    return !!id || traced;
}
defineExpose({ saveBench, discardBench, locate });
</script>

<template>
    <section class="bench">
        <div v-if="!scene" class="empty-bench">
            <p class="empty">{{ tr('optical.emptyBench') }}</p>
            <div class="controls" style="justify-content: center; margin-bottom: 12px">
                <button
                    v-for="tpl in TEMPLATES"
                    :key="tpl"
                    type="button"
                    class="primary"
                    :disabled="!online"
                    @click="loadTemplate(tpl)"
                >
                    <Sparkles :size="14" /> {{ tr('benchLab.templates.' + tpl) }}
                </button>
                <button v-if="!chatOpen" type="button" @click="emit('chat')">
                    {{ tr('optical.openChat') }}
                </button>
            </div>
        </div>
        <p v-if="error" class="banner error" role="alert">{{ error }}</p>
        <p v-if="statusMsg" class="banner ok" role="status">{{ statusMsg }}</p>

        <template v-if="scene">
            <div class="bench-controls">
                <button
                    type="button"
                    :aria-pressed="showTree"
                    :title="tr('benchLab.toggleTree')"
                    @click="showTree = !showTree"
                >
                    <List :size="13" /> {{ tr('benchLab.toggleTree') }} ({{
                        scene.elements.length
                    }})
                </button>
                <span v-if="benchDirty" class="badge badge-warn">
                    ● {{ tr('statusbar.draft') }}
                </span>

                <select v-model="templateChoice" aria-label="Lab template">
                    <option v-for="tpl in TEMPLATES" :key="tpl" :value="tpl">
                        {{ tr('benchLab.templates.' + tpl) }}
                    </option>
                </select>
                <button type="button" :disabled="!online" @click="loadTemplate(templateChoice)">
                    <Sparkles :size="13" /> {{ tr('benchLab.loadTemplate') }}
                </button>

                <button type="button" :disabled="!online" @click="runAutoAim">
                    <Crosshair :size="13" /> {{ tr('benchLab.autoAim') }}
                </button>
                <button type="button" :aria-pressed="snapHoles" @click="snapHoles = !snapHoles">
                    <Grid3x3 :size="13" /> {{ tr('benchLab.snapHoles') }}
                </button>
                <button
                    type="button"
                    :aria-pressed="showGaussian"
                    @click="showGaussian = !showGaussian"
                >
                    {{ tr('benchLab.gaussianOverlay') }}
                </button>

                <div class="spacer"></div>

                <button
                    v-if="benchDirty"
                    type="button"
                    class="primary"
                    :disabled="!canCommit"
                    @click="saveBench"
                >
                    <Save :size="13" /> {{ tr('benchLab.saveBench') }}
                </button>
                <button v-if="benchDirty" type="button" @click="discardBench">
                    <RotateCcw :size="13" /> {{ tr('app.revert') }}
                </button>

                <select v-model="exportFormat" aria-label="Export format">
                    <option value="svg">Blueprint SVG (1:1)</option>
                    <option value="pdf">Blueprint PDF (print at 100%)</option>
                    <option value="png">Blueprint PNG</option>
                    <option value="layout3d">3D layout SVG</option>
                    <option value="layout3d-pdf">3D layout PDF</option>
                    <option value="layout3d-png">3D layout PNG</option>
                    <option value="dxf">AutoCAD DXF</option>
                    <option value="stl">3D CAD STL</option>
                    <option value="alignment_md">Lab SOP (Markdown)</option>
                    <option value="csv">Coordinates CSV</option>
                </select>
                <label
                    v-if="exportFormat === 'png' || exportFormat === 'layout3d-png'"
                    class="export-resolution"
                >
                    <input
                        v-model.number="exportPxPerMm"
                        type="number"
                        min="0.5"
                        max="40"
                        step="0.5"
                        aria-label="PNG resolution (px/mm)"
                    />
                    px/mm
                </label>
                <button type="button" :disabled="!online" @click="exportDeliverable">
                    <Download :size="13" /> {{ tr('benchLab.export') }}
                </button>
            </div>

            <div class="bench-grid" :class="{ 'no-tree': !showTree }">
                <div v-if="showTree" class="bench-tree">
                    <strong>{{ scene.name }}</strong>
                    <div class="bench-element-list" role="list">
                        <button
                            v-for="e in allEntities"
                            :key="e.id"
                            :aria-pressed="selected === e.id"
                            :class="{ 'entity-warn': problemIds.has(e.id) }"
                            @click="selected = e.id"
                        >
                            <b>{{ e.id }}</b>
                            {{ 'kind' in e ? tr('bench.kinds.' + e.kind) : tr('bench.source') }}
                            <small>{{ e.position.map(fixed).join(', ') }} mm</small>
                        </button>
                    </div>
                </div>

                <div class="bench-stage">
                    <div class="stage-hud-top">
                        <button
                            type="button"
                            class="small"
                            :title="tr('bench.fit')"
                            @click="
                                zoom = 1;
                                pan = [0, 0];
                            "
                        >
                            <Maximize2 :size="12" />
                        </button>
                        <button
                            type="button"
                            class="small"
                            aria-label="Zoom in"
                            @click="zoom = Math.min(8, zoom * 1.2)"
                        >
                            <ZoomIn :size="12" />
                        </button>
                        <button
                            type="button"
                            class="small"
                            aria-label="Zoom out"
                            @click="zoom = Math.max(0.15, zoom / 1.2)"
                        >
                            <ZoomOut :size="12" />
                        </button>
                        <button
                            type="button"
                            class="small"
                            :disabled="!online || pending"
                            :title="tr('optical.refreshTrace')"
                            @click="run"
                        >
                            <RefreshCw :size="12" />
                        </button>
                        <button
                            v-if="mode === '3d'"
                            type="button"
                            class="small"
                            :aria-pressed="showRender"
                            :title="tr('benchLab.render3dHint')"
                            @click="showRender = !showRender"
                        >
                            <Box :size="12" />
                        </button>
                    </div>
                    <figure
                        v-if="mode === '3d' && showRender && renderUrl"
                        class="render3d"
                        :class="{ stale: renderStale }"
                        :title="tr('benchLab.render3dHint')"
                    >
                        <img :src="renderUrl" :alt="tr('benchLab.render3d')" />
                        <figcaption>
                            {{ tr(renderStale ? 'benchLab.render3dStale' : 'benchLab.render3d') }}
                        </figcaption>
                    </figure>
                    <div v-if="hoverCoord && mode === '2d'" class="stage-hud-coord">
                        X {{ hoverCoord.x.toFixed(1) }} · Y {{ hoverCoord.y.toFixed(1) }} mm ·
                        <b>C{{ hoverCoord.col }}, R{{ hoverCoord.row }}</b>
                    </div>
                    <svg
                        ref="svg"
                        class="bench-viewport"
                        viewBox="0 0 1000 600"
                        tabindex="0"
                        :aria-label="tr('bench.viewport')"
                        @contextmenu.prevent
                        @pointerdown="begin"
                        @pointermove="move"
                        @pointerup="endPointer"
                        @pointercancel="endPointer"
                        @pointerleave="hoverCoord = null"
                        @keydown.esc="endPointer"
                        @wheel.prevent="
                            zoom = Math.max(
                                0.15,
                                Math.min(8, zoom * ($event.deltaY > 0 ? 0.9 : 1.1)),
                            )
                        "
                    >
                        <polygon :points="floor" class="bench-table" />
                        <path
                            v-if="plan?.holes.length"
                            class="bench-holes plan-holes"
                            :d="planHolePath"
                            :data-holes="plan.holes.length"
                        />
                        <g v-if="holeDots.length" class="bench-holes">
                            <circle
                                v-for="(pt, hi) in holeDots"
                                :key="hi"
                                :cx="pt[0]"
                                :cy="pt[1]"
                                r="1.4"
                            />
                        </g>
                        <g class="bench-mounts">
                            <template v-for="e in scene.elements" :key="e.id">
                                <polyline
                                    v-for="(line, i) in mountLines(e)"
                                    :key="i"
                                    :points="path(line)"
                                />
                            </template>
                        </g>
                        <polyline
                            v-for="(line, i) in grid"
                            :key="'grid' + i"
                            :points="path(line)"
                            class="bench-gridline"
                        />
                        <g v-if="planRulers.length" class="plan-rulers" aria-hidden="true">
                            <text
                                v-for="tick in planRulers"
                                :key="tick.axis + tick.value_mm"
                                :x="tick.axis === 'x' ? tick.x : tick.x - 6"
                                :y="tick.axis === 'x' ? tick.y + 14 : tick.y + 3"
                                :text-anchor="tick.axis === 'x' ? 'middle' : 'end'"
                            >
                                {{ tick.value_mm }}
                            </text>
                        </g>
                        <g
                            v-for="zone in zoneOutlines"
                            :key="'zone' + zone.id"
                            class="bench-zone"
                            :class="{ flagged: zone.outside }"
                            :data-zone="zone.id"
                        >
                            <polyline :points="zone.points" />
                            <text
                                v-if="!plan"
                                :x="zone.center[0]"
                                :y="zone.center[1]"
                                text-anchor="middle"
                            >
                                {{ zone.label }}
                            </text>
                        </g>
                        <polyline
                            v-for="leg in periscopeLegs"
                            :key="'periscope' + leg.id"
                            :points="leg.points"
                            class="bench-periscope"
                            :data-periscope="leg.id"
                        >
                            <title>{{ tr('benchLab.readings.periscope', { id: leg.id }) }}</title>
                        </polyline>
                        <!-- Gaussian 1/e² beam envelope polygons -->
                        <polygon
                            v-for="(poly, gi) in gaussianPolygons"
                            :key="'gauss' + gi"
                            :points="poly"
                            class="bench-gaussian"
                        />
                        <g v-if="plan" class="plan-beams" :data-revision="plan.revision">
                            <template v-for="(beam, i) in planBeams" :key="'beam' + i">
                                <line
                                    :x1="flat(beam.from)[0]"
                                    :y1="flat(beam.from)[1]"
                                    :x2="flat(beam.to)[0]"
                                    :y2="flat(beam.to)[1]"
                                    :stroke="beam.color"
                                    :stroke-dasharray="dashPattern(beam.dash)"
                                    :data-branch="beam.branch ?? 'unbranched'"
                                />
                                <polygon
                                    v-if="beam.arrow"
                                    :points="flatPath(beam.arrow)"
                                    :fill="beam.color"
                                />
                            </template>
                        </g>
                        <g v-if="plan" class="plan-stages">
                            <g v-for="stage in plan.stages" :key="stage.id" :data-stage="stage.id">
                                <polygon :points="flatPath(stage.outline)" />
                                <polyline :points="flatPath(stage.travel)" class="travel" />
                            </g>
                        </g>
                        <polyline
                            v-for="(ray, i) in raysShown"
                            :key="'ray' + i"
                            :points="path([ray.from, ray.to])"
                            class="bench-ray"
                            :class="{
                                bad: !['hit', 'escaped', 'group_surface'].includes(ray.status),
                                reflected: ray.branch.includes('reflected'),
                                scattered: ray.branch.includes(':scatter'),
                                ghost: ray.branch.includes(':ghost'),
                            }"
                        >
                            <title>
                                {{ ray.branch }} · {{ tr('bench.reasons.' + ray.status) }} ·
                                {{ fixed(ray.power) }}
                            </title>
                        </polyline>
                        <!-- Recommended Beam Dump safety markers -->
                        <g
                            v-for="(bd, bdi) in labCheck?.recommended_beam_dumps ?? []"
                            :key="'bd' + bdi"
                            class="bench-dump-hint"
                            @pointerdown.stop
                            @click="addBeamDumpAt(bd.position, bd.direction, bd.branch)"
                        >
                            <circle
                                :cx="project(bd.position)[0]"
                                :cy="project(bd.position)[1]"
                                r="11"
                            />
                            <text
                                :x="project(bd.position)[0] + 12"
                                :y="project(bd.position)[1] + 4"
                            >
                                +BD
                            </text>
                        </g>
                        <g
                            v-for="e in scene.elements"
                            :key="e.id"
                            class="bench-object"
                            :class="{
                                chosen: selected === e.id,
                                flagged: problemIds.has(e.id),
                                dimmed: !!layerFilter && e.layer !== layerFilter,
                            }"
                            role="button"
                            tabindex="0"
                            :aria-label="tr('bench.selectElement', { id: e.id })"
                            :data-element="e.id"
                            @pointerdown.stop="beginDragEntity($event, e.id, e.position)"
                            @click="selected = e.id"
                            @keydown.enter="selected = e.id"
                            @dblclick="e.kind === 'lens_group' && emit('inspectGroup')"
                        >
                            <g
                                v-if="planSymbols.get(e.id)"
                                class="plan-symbol"
                                :data-symbol="planSymbols.get(e.id)?.kind"
                                :transform="planShift(e.id)"
                            >
                                <component
                                    :is="stroke.closed ? 'polygon' : 'polyline'"
                                    v-for="(stroke, i) in planSymbols.get(e.id)?.strokes"
                                    :key="i"
                                    :points="flatPath(stroke.points)"
                                    :class="stroke.role"
                                />
                            </g>
                            <template v-else>
                                <polyline
                                    v-if="e.kind !== 'lens_group'"
                                    :points="shape(e)"
                                    class="bench-body"
                                    :class="e.kind"
                                />
                                <polyline
                                    v-for="(curve, i) in groupCurves(e)"
                                    :key="i"
                                    :points="path(curve)"
                                    class="bench-body lens"
                                />
                                <polyline :points="shape(e, e.aperture)" class="bench-aperture" />
                            </template>
                            <g
                                v-for="region in e.regions ?? []"
                                :key="region.port"
                                class="bench-region"
                            >
                                <polyline :points="regionOutline(e, region)" />
                                <text
                                    :x="regionCenter(e, region)[0]"
                                    :y="regionCenter(e, region)[1]"
                                    text-anchor="middle"
                                >
                                    {{ region.port }}
                                </text>
                            </g>
                            <circle
                                :cx="project(e.position)[0]"
                                :cy="project(e.position)[1]"
                                r="7"
                                class="bench-center"
                                :class="{ 'plan-hit': planSymbols.has(e.id) }"
                            />
                            <text
                                v-if="!plan"
                                :x="project(e.position)[0] + 10"
                                :y="project(e.position)[1] - 10"
                            >
                                {{ e.id }}
                            </text>
                        </g>
                        <g
                            v-for="s in scene.sources"
                            :key="s.id"
                            class="bench-source"
                            role="button"
                            tabindex="0"
                            :aria-label="tr('bench.selectElement', { id: s.id })"
                            @pointerdown.stop="beginDragEntity($event, s.id, s.position)"
                            @click="selected = s.id"
                            @keydown.enter="selected = s.id"
                        >
                            <g
                                v-if="planSymbols.get(s.id)"
                                class="plan-symbol"
                                data-symbol="laser"
                                :transform="planShift(s.id)"
                            >
                                <component
                                    :is="stroke.closed ? 'polygon' : 'polyline'"
                                    v-for="(stroke, i) in planSymbols.get(s.id)?.strokes"
                                    :key="i"
                                    :points="flatPath(stroke.points)"
                                    :class="stroke.role"
                                />
                            </g>
                            <circle
                                :cx="project(s.position)[0]"
                                :cy="project(s.position)[1]"
                                r="8"
                                :class="{ 'plan-hit': planSymbols.has(s.id) }"
                            />
                            <text
                                v-if="!plan"
                                :x="project(s.position)[0] + 12"
                                :y="project(s.position)[1] - 10"
                            >
                                {{ s.id }}
                            </text>
                        </g>
                        <g v-if="plan" class="plan-periscopes">
                            <g
                                v-for="mark in plan.periscopes"
                                :key="mark.id"
                                :data-periscope="mark.id"
                                :data-rise="mark.rise_mm"
                            >
                                <circle :cx="flat(mark.at)[0]" :cy="flat(mark.at)[1]" r="9" />
                                <path
                                    :d="`M${flat(mark.at)[0] - 9} ${flat(mark.at)[1]}h18M${flat(mark.at)[0]} ${flat(mark.at)[1] - 9}v18`"
                                />
                            </g>
                        </g>
                        <g
                            v-if="plan"
                            class="plan-labels"
                            :style="{ fontSize: `${plan.text_mm * factor}px` }"
                        >
                            <g
                                v-for="(label, i) in planLabels"
                                :key="'label' + i"
                                :transform="label.shift"
                            >
                                <line
                                    v-if="label.leader"
                                    :x1="label.leaderFrom[0]"
                                    :y1="label.leaderFrom[1]"
                                    :x2="label.leaderTo[0]"
                                    :y2="label.leaderTo[1]"
                                />
                                <text
                                    :x="label.at[0]"
                                    :y="label.at[1]"
                                    :class="label.tone"
                                    :data-label="label.kind"
                                    :data-target="label.target"
                                >
                                    {{ label.text }}
                                </text>
                            </g>
                        </g>
                    </svg>
                    <ul v-if="plan?.legend.length" class="plan-legend">
                        <li
                            v-for="entry in plan.legend"
                            :key="entry.kind + entry.key"
                            :data-legend="entry.kind"
                            :data-key="entry.key"
                        >
                            <span
                                class="swatch"
                                :class="entry.kind"
                                :style="{
                                    borderColor: entry.color,
                                    borderStyle: entry.dash.length ? 'dashed' : 'solid',
                                }"
                            ></span>
                            {{ entry.text }}
                        </li>
                    </ul>
                    <ul v-if="plan?.cards.length" class="plan-cards">
                        <li
                            v-for="card in plan.cards"
                            :key="card.kind + card.id"
                            :data-kind="card.kind"
                            :data-id="card.id"
                            :style="{ borderColor: card.color, color: card.color }"
                        >
                            <span v-for="(line, li) in card.lines" :key="li">{{ line }}</span>
                        </li>
                    </ul>
                    <p class="bench-help">
                        {{ tr('optical.cameraHelp') }}
                        <span v-if="mode === '2d'"> · {{ tr('benchLab.dragHint') }}</span>
                    </p>
                    <div v-if="trace" class="bench-ledger" role="status">
                        <span v-for="(value, key) in trace.power" :key="key">
                            {{ tr('bench.power.' + key) }} <b>{{ fixed(value) }}</b>
                        </span>
                        <span>
                            {{ tr('bench.balance') }} <b>{{ fixed(trace.balance_error) }}</b>
                        </span>
                    </div>
                    <label v-if="scene?.layers?.length" class="bench-branch">
                        {{ tr('bench.layer') }}
                        <select v-model="layerFilter">
                            <option value="">{{ tr('bench.allLayers') }}</option>
                            <option v-for="l in scene.layers" :key="l.id" :value="l.id">
                                {{ l.id }} · {{ l.height_mm }} mm
                            </option>
                        </select>
                    </label>
                    <label v-if="trace" class="bench-branch">
                        {{ tr('bench.branch') }}
                        <select v-model="branch">
                            <option value="">{{ tr('bench.allBranches') }}</option>
                            <option
                                v-for="end in trace.terminations"
                                :key="end.branch"
                                :value="end.branch"
                            >
                                {{ end.branch }} · {{ end.element ?? '—' }} ·
                                {{ tr('bench.reasons.' + end.reason) }}
                            </option>
                        </select>
                    </label>
                </div>

                <!-- EDITABLE LAB INSPECTOR -->
                <section class="bench-inspector" :aria-label="tr('bench.selection')">
                    <section
                        v-if="pendingInputs.length"
                        class="bench-pending"
                        :aria-label="tr('benchLab.pendingTitle')"
                    >
                        <strong>{{
                            tr('benchLab.pendingTitle', { count: pendingInputs.length })
                        }}</strong>
                        <p class="note">{{ tr('benchLab.pendingHelp') }}</p>
                        <form
                            v-for="input in pendingInputs"
                            :key="input.key"
                            class="bench-pending-row"
                            :data-pending="input.key"
                            @submit.prevent="supplyInput(input.index, input.key, input.value)"
                        >
                            <button
                                type="button"
                                class="bench-pending-target"
                                :title="input.note"
                                @click="selected = input.target"
                            >
                                {{ input.label }}
                            </button>
                            <small v-if="input.note" class="bench-pending-note">{{
                                input.note
                            }}</small>
                            <small v-if="input.layer" class="bench-pending-note">{{
                                tr('benchLab.pendingLayer', { layer: input.layer })
                            }}</small>
                            <input
                                type="number"
                                step="any"
                                :aria-label="input.label"
                                :placeholder="input.value === undefined ? '' : String(input.value)"
                                :value="pendingEntries[input.key] ?? input.value"
                                @input="
                                    pendingEntries[input.key] = (
                                        $event.target as HTMLInputElement
                                    ).value
                                "
                            />
                            <button type="submit" class="small" :disabled="saving">
                                {{ tr('benchLab.pendingConfirm') }}
                            </button>
                        </form>
                    </section>
                    <div class="panel-head flush">
                        <strong>{{ entity ? selected : tr('bench.selection') }}</strong>
                        <button
                            v-if="entity && 'kind' in entity && !selectedStage"
                            type="button"
                            class="small"
                            :aria-label="tr('bench.remove')"
                            @click="deleteSelectedElement"
                        >
                            <Trash2 :size="12" />
                        </button>
                    </div>
                    <template v-if="entity">
                        <p v-if="selectedStage" class="note">
                            {{ tr('benchLab.stageMember', { stage: selectedStage.id }) }}
                        </p>
                        <label>
                            X (mm)
                            <input
                                type="number"
                                step="1"
                                :readonly="!!selectedStage"
                                :value="entity.position[0]"
                                @change="
                                    entity.position[0] = Number(
                                        ($event.target as HTMLInputElement).value,
                                    );
                                    schedulePreview();
                                "
                            />
                        </label>
                        <label>
                            Y (mm)
                            <input
                                type="number"
                                step="1"
                                :readonly="!!selectedStage"
                                :value="entity.position[1]"
                                @change="
                                    entity.position[1] = Number(
                                        ($event.target as HTMLInputElement).value,
                                    );
                                    schedulePreview();
                                "
                            />
                        </label>
                        <label>
                            Z (mm)
                            <input
                                type="number"
                                step="1"
                                :readonly="!!selectedStage || zLocked"
                                :title="zLocked ? tr('bench.zFromLayer') : undefined"
                                :value="entity.position[2]"
                                @change="
                                    entity.position[2] = Number(
                                        ($event.target as HTMLInputElement).value,
                                    );
                                    schedulePreview();
                                "
                            />
                        </label>
                        <button
                            v-if="!selectedStage"
                            type="button"
                            class="small"
                            @click="snapSelectedToHole"
                        >
                            <Grid3x3 :size="12" /> {{ tr('benchLab.snapSelected') }}
                        </button>

                        <template v-if="'kind' in entity">
                            <label v-if="scene?.layers?.length">
                                {{ tr('bench.layer') }}
                                <select
                                    :value="entity.layer ?? ''"
                                    :disabled="!!selectedStage"
                                    @change="
                                        setLayer(entity, ($event.target as HTMLSelectElement).value)
                                    "
                                >
                                    <option value="">—</option>
                                    <option v-for="l in scene.layers" :key="l.id" :value="l.id">
                                        {{ l.id }} · {{ l.height_mm }} mm
                                    </option>
                                </select>
                            </label>
                            <label>
                                {{ tr('bench.kind') }}
                                <span>{{ tr('bench.kinds.' + entity.kind) }}</span>
                            </label>
                            <label>
                                {{ tr('bench.role') }}
                                <input
                                    type="text"
                                    class="bench-role"
                                    maxlength="64"
                                    :placeholder="tr('bench.rolePlaceholder')"
                                    :value="entity.role ?? ''"
                                    @change="
                                        setRole(entity, ($event.target as HTMLInputElement).value)
                                    "
                                />
                            </label>
                            <div
                                class="bench-metadata"
                                role="group"
                                :aria-label="tr('bench.metadata')"
                            >
                                <span>{{ tr('bench.metadata') }}</span>
                                <div
                                    v-for="(value, key) in entity.metadata ?? {}"
                                    :key="key"
                                    class="bench-metadata-row"
                                >
                                    <input
                                        type="text"
                                        maxlength="64"
                                        :aria-label="tr('bench.metadataKey')"
                                        :aria-invalid="!META_KEY.test(String(key))"
                                        :value="key"
                                        @change="
                                            setMetaKey(
                                                entity,
                                                String(key),
                                                ($event.target as HTMLInputElement).value,
                                            )
                                        "
                                    />
                                    <input
                                        type="text"
                                        maxlength="256"
                                        :aria-label="
                                            tr('bench.metadataValue', { key: String(key) })
                                        "
                                        :value="String(value)"
                                        @change="
                                            setMetaValue(
                                                entity,
                                                String(key),
                                                ($event.target as HTMLInputElement).value,
                                            )
                                        "
                                    />
                                    <button
                                        type="button"
                                        class="small"
                                        :aria-label="
                                            tr('bench.metadataRemove', { key: String(key) })
                                        "
                                        @click="removeMeta(entity, String(key))"
                                    >
                                        <Minus :size="12" />
                                    </button>
                                </div>
                                <button
                                    type="button"
                                    class="small"
                                    :disabled="Object.keys(entity.metadata ?? {}).length >= 32"
                                    @click="addMeta(entity)"
                                >
                                    <Plus :size="12" /> {{ tr('bench.metadataAdd') }}
                                </button>
                            </div>
                            <label>
                                {{ tr('bench.autoRotation') }}
                                <input
                                    type="checkbox"
                                    :disabled="!!selectedStage"
                                    :checked="entity.rotation == null"
                                    @change="
                                        entity.rotation = ($event.target as HTMLInputElement)
                                            .checked
                                            ? null
                                            : [0, 0, 0];
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <template v-if="entity.rotation">
                                <label v-if="!selectedStage">
                                    {{ tr('bench.aimExistingRotation') }}
                                    <input
                                        type="checkbox"
                                        :checked="entity.aim_rotation"
                                        @change="
                                            entity.aim_rotation =
                                                ($event.target as HTMLInputElement).checked ||
                                                undefined
                                        "
                                    />
                                </label>
                                <label>
                                    Roll (°)
                                    <input
                                        type="number"
                                        step="1"
                                        :readonly="!!selectedStage"
                                        :value="entity.rotation[0]"
                                        @change="setRotation(entity, 0, $event)"
                                    />
                                </label>
                                <label>
                                    Pitch (°)
                                    <input
                                        type="number"
                                        step="1"
                                        :readonly="!!selectedStage"
                                        :value="entity.rotation[1]"
                                        @change="setRotation(entity, 1, $event)"
                                    />
                                </label>
                                <label>
                                    Yaw (°)
                                    <input
                                        type="number"
                                        step="1"
                                        :readonly="!!selectedStage"
                                        :value="entity.rotation[2]"
                                        @change="setRotation(entity, 2, $event)"
                                    />
                                </label>
                            </template>
                            <label>
                                {{ tr('bench.aperture') }}
                                <input
                                    type="number"
                                    step="0.5"
                                    min="0.5"
                                    :value="entity.aperture"
                                    @change="
                                        entity.aperture = Number(
                                            ($event.target as HTMLInputElement).value,
                                        );
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label v-if="entity.kind === 'lens' || entity.kind === 'curved_mirror'">
                                {{ tr('bench.focal') }}
                                <input
                                    type="number"
                                    step="5"
                                    :value="entity.focal_length"
                                    @change="
                                        entity.focal_length = Number(
                                            ($event.target as HTMLInputElement).value,
                                        );
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label v-if="entity.kind === 'waveplate'">
                                {{ tr('benchLab.retardance') }}
                                <input
                                    type="number"
                                    step="0.25"
                                    :value="entity.retardance_waves ?? 0.5"
                                    @change="
                                        entity.retardance_waves = Number(
                                            ($event.target as HTMLInputElement).value,
                                        );
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label
                                v-if="entity.kind === 'waveplate' || entity.kind === 'polarizer'"
                            >
                                {{ tr('benchLab.fastAxis') }}
                                <input
                                    type="number"
                                    step="5"
                                    :value="entity.fast_axis_deg ?? 0"
                                    @change="
                                        entity.fast_axis_deg = Number(
                                            ($event.target as HTMLInputElement).value,
                                        );
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label v-if="entity.kind === 'pbs' || entity.kind === 'polarizer'">
                                ER
                                <input
                                    type="number"
                                    step="100"
                                    min="1"
                                    :value="entity.extinction_ratio ?? 1000"
                                    @change="
                                        entity.extinction_ratio = Number(
                                            ($event.target as HTMLInputElement).value,
                                        );
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label v-if="entity.mount">
                                {{ tr('optical.mount') }}
                                <span>{{ entity.mount.part }}</span>
                            </label>
                            <template v-if="entity.kind === 'lens_group'">
                                <p class="note">
                                    {{ tr('optical.currentPrescription') }} ·
                                    {{ groupSummary?.surfaces }} · {{ tr('metrics.efl') }}
                                    {{
                                        groupSummary?.efl == null
                                            ? tr('common.dash')
                                            : fixed(groupSummary.efl) + ' mm'
                                    }}
                                    · {{ tr('bench.aperture') }}
                                    {{ fixed(groupSummary?.maxAperture ?? 0) }} mm
                                </p>
                                <button type="button" @click="emit('inspectGroup')">
                                    {{ tr('optical.inspectGroup') }}
                                </button>
                            </template>
                        </template>

                        <template v-else>
                            <label>
                                λ (nm)
                                <input
                                    type="number"
                                    step="1"
                                    :value="entity.wavelength"
                                    @change="
                                        entity.wavelength = Number(
                                            ($event.target as HTMLInputElement).value,
                                        );
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label>
                                {{ tr('bench.autoDirection') }}
                                <input
                                    type="checkbox"
                                    :checked="entity.aim_direction"
                                    @change="
                                        entity.aim_direction = (
                                            $event.target as HTMLInputElement
                                        ).checked;
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label>
                                {{ tr('benchLab.waist') }}
                                <input
                                    type="number"
                                    step="0.1"
                                    min="0.01"
                                    :value="entity.waist ?? entity.beam_radius"
                                    @change="
                                        entity.waist = Number(
                                            ($event.target as HTMLInputElement).value,
                                        );
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label>
                                M²
                                <input
                                    type="number"
                                    step="0.1"
                                    min="1"
                                    :value="entity.m2 ?? 1"
                                    @change="
                                        entity.m2 = Number(
                                            ($event.target as HTMLInputElement).value,
                                        );
                                        schedulePreview();
                                    "
                                />
                            </label>
                            <label>
                                {{ tr('benchLab.polarization') }}
                                <select
                                    :value="entity.polarization?.kind ?? 'unpolarized'"
                                    @change="
                                        updateSelectedPol(
                                            ($event.target as HTMLSelectElement).value,
                                        )
                                    "
                                >
                                    <option value="unpolarized">Unpol</option>
                                    <option value="horizontal">H (0°)</option>
                                    <option value="vertical">V (90°)</option>
                                    <option value="diagonal45">45°</option>
                                    <option value="right_circular">RHC</option>
                                    <option value="left_circular">LHC</option>
                                </select>
                            </label>
                        </template>
                        <div v-if="selectedPorts.length" class="inspector-conns">
                            <div class="note">{{ tr('bench.connections') }}</div>
                            <div
                                v-for="c in selectedConnections"
                                :key="c.from + ':' + c.port"
                                class="conn-chip"
                            >
                                <span
                                    >{{ c.port }} → <b>{{ c.to }}</b></span
                                >
                                <button
                                    type="button"
                                    class="small"
                                    @click="removeConnection(c.from, c.port)"
                                >
                                    ×
                                </button>
                            </div>
                            <div class="conn-add">
                                <select v-model="connPort">
                                    <option v-for="pt in selectedPorts" :key="pt" :value="pt">
                                        {{ pt }}
                                    </option>
                                </select>
                                <select v-model="connTo">
                                    <option value="">→</option>
                                    <option
                                        v-for="e in scene.elements.filter((x) => x.id !== selected)"
                                        :key="e.id"
                                        :value="e.id"
                                    >
                                        {{ e.id }}
                                    </option>
                                </select>
                                <button
                                    type="button"
                                    class="small"
                                    :title="tr('benchLab.addConn')"
                                    :disabled="!connTo"
                                    @click="addSelectedConnection"
                                >
                                    <Plus :size="12" />
                                </button>
                            </div>
                        </div>
                    </template>
                    <p v-else class="note">{{ tr('bench.selectHelp') }}</p>
                </section>
            </div>

            <!-- LAB ENGINEERING STUDIO SUB-PANEL -->
            <BenchLabPanel
                :bench="scene"
                :online="online"
                :saving="saving"
                :lab-check="labCheck"
                :lab-pending="labPending"
                :lab-stale="labStale"
                :lab-error="labError"
                @select="selected = $event"
                @add-from-catalog="addFromCatalog"
                @add-beam-dump="addBeamDumpAt"
                @refresh-lab="runLabCheck"
                @set-stage="setStagePosition"
            />

            <details v-if="failures.length" style="margin-top: 8px">
                <summary>{{ tr('optical.traceDetails') }} ({{ failures.length }})</summary>
                <p v-for="(f, i) in failures.slice(0, 80)" :key="i">
                    {{ f.branch }} · {{ f.element ?? '—' }} ·
                    {{ tr('bench.reasons.' + f.reason) }} ·
                    {{ f.position.map(fixed).join(', ') }} mm
                </p>
            </details>
            <details v-if="trace?.detectors.length" open style="margin-top: 8px">
                <summary>{{ tr('bench.detectorMaps') }} ({{ trace.detectors.length }})</summary>
                <div class="bench-map-list">
                    <figure v-for="map in trace.detectors" :key="map.id" class="bench-map">
                        <HeatMap
                            :title="map.id"
                            :values="map.power"
                            :size="map.bins[1]"
                            :width="map.bins[0]"
                        />
                        <figcaption>
                            {{ tr('bench.mapTotal') }} <b>{{ fixed(map.total) }}</b> ·
                            {{ tr('bench.mapCentroid') }}
                            {{ map.centroid.map(fixed).join(', ') }} mm · RMS
                            {{ map.rms.map(fixed).join(', ') }} mm
                        </figcaption>
                    </figure>
                </div>
            </details>
        </template>
    </section>
</template>

<style scoped>
.bench {
    min-width: 0;
}
.empty-bench {
    padding: 16px;
    text-align: center;
}
.bench-controls {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    margin-bottom: 10px;
}
.bench-count {
    margin-left: auto;
    color: var(--muted);
    font-size: 12px;
}
.bench-grid {
    display: grid;
    grid-template-columns: 160px minmax(300px, 1fr) 232px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--panel);
}
.bench-grid.no-tree {
    grid-template-columns: minmax(300px, 1fr) 232px;
}
.bench-stage {
    position: relative;
    min-width: 0;
    background: var(--surface);
}
.stage-hud-top {
    position: absolute;
    top: 8px;
    right: 8px;
    z-index: 5;
    display: flex;
    gap: 3px;
    padding: 3px;
    border-radius: var(--radius);
    background: var(--panel);
    border: 1px solid var(--border);
    box-shadow: var(--shadow-sm);
}
.stage-hud-top button {
    border: 0;
    padding: 4px 6px;
    min-height: 24px;
}
.render3d {
    position: absolute;
    left: 8px;
    bottom: 8px;
    z-index: 5;
    width: min(42%, 360px);
    margin: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--panel);
    overflow: hidden;
    pointer-events: none;
}
.render3d img {
    display: block;
    width: 100%;
}
.render3d.stale img {
    opacity: 0.45;
}
.render3d figcaption {
    padding: 2px 6px;
    font-size: 11px;
    color: var(--muted);
}
.stage-hud-coord {
    position: absolute;
    top: 8px;
    left: 8px;
    z-index: 5;
    padding: 3px 8px;
    border-radius: var(--radius-sm);
    background: var(--panel);
    border: 1px solid var(--border);
    font-size: 11px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    pointer-events: none;
    box-shadow: var(--shadow-sm);
}
.export-resolution {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
}
.export-resolution input {
    width: 4.5em;
}
.plan-holes,
.bench-holes circle {
    fill: var(--border-strong);
    pointer-events: none;
}
.inspector-conns {
    margin-top: 10px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 5px;
}
.conn-chip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 3px 6px;
    border-radius: var(--radius-sm);
    background: var(--surface);
    border: 1px solid var(--border);
    font-size: 11px;
}
.conn-add {
    display: grid;
    grid-template-columns: 1fr 1fr auto;
    gap: 4px;
}
.bench-tree {
    border-right: 1px solid var(--border);
    padding: 10px;
    min-width: 0;
}
.bench-element-list {
    max-height: 510px;
    overflow: auto;
    display: grid;
    gap: 3px;
    margin-top: 6px;
}
.bench-element-list button {
    text-align: left;
    display: block;
    overflow-wrap: anywhere;
    padding: 6px;
}
.bench-element-list button.entity-warn {
    border-left: 3px solid var(--warning);
}
.bench-element-list small {
    display: block;
    color: var(--muted);
    font-size: 10px;
}
.bench button[aria-pressed='true'] {
    color: var(--accent);
    background: var(--hover);
    outline: 1px solid var(--accent);
}
.bench-stage {
    min-width: 0;
    background: var(--surface);
}
.bench-viewport {
    display: block;
    width: 100%;
    min-height: 350px;
    max-height: 650px;
    touch-action: none;
    user-select: none;
}
.bench-table {
    fill: var(--panel);
    stroke: var(--border);
    stroke-width: 2;
}
.bench-gridline {
    fill: none;
    stroke: var(--border);
    stroke-width: 0.7;
}
.bench-gaussian {
    fill: var(--accent);
    opacity: 0.16;
    pointer-events: none;
}
.bench-ray {
    fill: none;
    stroke: #258ca9;
    stroke-width: 1.5;
    pointer-events: none;
}
.bench-ray.reflected {
    stroke: #be8622;
}
.bench-ray.scattered {
    stroke: #7a9e42;
    stroke-width: 1;
}
.bench-ray.ghost {
    stroke: #8b6db1;
}
.bench-ray.bad {
    stroke: #d14450;
    stroke-dasharray: 5 3;
    opacity: 0.85 !important;
}
.bench-body {
    fill: var(--hover);
    stroke: var(--text);
    stroke-width: 2;
}
.bench-body.splitter,
.bench-body.pbs {
    stroke: #8b6db1;
}
.bench-body.lens {
    stroke: #258ca9;
    fill: transparent;
}
.bench-body.curved_mirror {
    stroke: #c05621;
}
.bench-body.waveplate {
    stroke: #0d9488;
}
.bench-body.polarizer {
    stroke: #d97706;
}
.bench-body.detector {
    stroke: #278260;
}
.bench-center {
    fill: var(--panel);
    stroke: var(--text);
    stroke-width: 1.5;
}
.bench-object {
    cursor: grab;
}
.bench-object:active {
    cursor: grabbing;
}
.bench-object.chosen .bench-center {
    fill: var(--accent);
    stroke: var(--accent);
    stroke-width: 4;
}
.bench-object.flagged .bench-aperture {
    stroke: var(--danger);
    stroke-dasharray: 4 2;
}
.bench-dump-hint {
    cursor: pointer;
}
.bench-object.dimmed {
    opacity: 0.25;
}
.bench-zone polyline {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-dasharray: 6 4;
}
.bench-zone.flagged polyline {
    stroke: var(--danger);
}
.plan-beams line {
    stroke-width: 2;
}
.plan-beams polygon {
    stroke: none;
}
.plan-stages polygon {
    fill: none;
    stroke: var(--muted);
    stroke-width: 1;
    stroke-dasharray: 5 3;
}
.plan-stages .travel {
    fill: none;
    stroke: var(--text);
    stroke-width: 1.5;
}
.plan-symbol polygon,
.plan-symbol polyline {
    stroke: var(--text);
    stroke-width: 1;
    stroke-linejoin: round;
}
.plan-symbol .body {
    fill: var(--panel);
}
.plan-symbol .fill {
    fill: var(--text);
}
.plan-symbol .face {
    fill: none;
    stroke-width: 2.2;
}
.plan-symbol .hatch {
    fill: none;
    stroke-width: 0.6;
}
.bench-object.chosen .plan-symbol polygon,
.bench-object.chosen .plan-symbol polyline {
    stroke: var(--accent);
}
.bench-object.flagged .plan-symbol .body {
    stroke: var(--danger);
    stroke-dasharray: 4 2;
}
.bench-center.plan-hit,
.bench-source circle.plan-hit {
    fill: transparent;
    stroke: none;
}
.bench-object.chosen .bench-center.plan-hit {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
}
.plan-periscopes {
    fill: none;
    stroke: var(--warning);
    stroke-width: 1.5;
    pointer-events: none;
}
.plan-labels {
    pointer-events: none;
}
.plan-labels text {
    fill: var(--text);
    paint-order: stroke;
    stroke: var(--panel);
    stroke-width: 3px;
}
.plan-labels text.annotation {
    fill: var(--warning);
    font-style: italic;
}
.plan-labels text.pass {
    fill: var(--success, #26734d);
}
.plan-labels text.fail {
    fill: var(--danger);
}
.plan-labels text.indeterminate {
    fill: #b54708;
}
.plan-labels text.zone,
.plan-labels text.stage {
    fill: var(--muted);
}
.plan-labels line {
    stroke: var(--muted);
    stroke-width: 1;
}
.plan-rulers text {
    fill: var(--muted);
    font-size: 10px;
    pointer-events: none;
}
.plan-cards {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
    font-size: 11px;
}
.plan-cards li {
    display: flex;
    flex-direction: column;
    padding: 3px 6px;
    border: 1px solid;
    border-radius: 4px;
    background: var(--surface);
}
.plan-legend {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
    color: var(--muted);
    font-size: 12px;
}
.plan-legend li {
    display: flex;
    align-items: center;
    gap: 6px;
}
.plan-legend .swatch {
    width: 18px;
    height: 0;
    border-top-width: 2px;
}
.plan-legend .swatch.symbol {
    width: 10px;
    height: 10px;
    border-width: 1px;
}
.bench-zone text {
    fill: var(--muted);
    font-size: 12px;
}
.bench-periscope {
    stroke: var(--warning);
    stroke-width: 3;
    stroke-dasharray: 3 3;
}
.bench-dump-hint circle {
    fill: var(--warning-bg);
    stroke: var(--warning);
    stroke-width: 2;
    stroke-dasharray: 3 2;
}
.bench-dump-hint text {
    fill: var(--warning);
    font-size: 11px;
    font-weight: 700;
}
.bench-object text,
.bench-source text {
    fill: var(--text);
    font-size: 14px;
    paint-order: stroke;
    stroke: var(--panel);
    stroke-width: 3px;
    pointer-events: none;
}
.bench-source {
    cursor: grab;
}
.bench-source circle {
    fill: #bf535c;
    stroke: var(--panel);
    stroke-width: 2;
}
.bench-mounts {
    pointer-events: none;
    fill: none;
    stroke: var(--muted);
    stroke-width: 1;
    opacity: 0.5;
}
.bench-aperture {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    pointer-events: none;
}
.bench-region polyline {
    fill: color-mix(in srgb, var(--accent) 18%, transparent);
    stroke: var(--accent);
    stroke-width: 1;
    pointer-events: none;
}
.bench-region text {
    font-size: 9px;
    fill: var(--accent);
    pointer-events: none;
}
.bench-inspector {
    margin: 0;
    border: 0;
    border-left: 1px solid var(--border);
    padding: 10px;
    min-width: 0;
}
.bench-pending {
    margin: 0 0 10px;
    padding: 0 0 8px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
}
.bench-pending .note {
    margin: 2px 0 6px;
    color: var(--muted);
    font-size: 11px;
}
.bench-pending-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 4px 5px;
    align-items: center;
    margin-bottom: 6px;
}
.bench-pending-target {
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font-family: var(--font-mono);
    font-size: 11px;
    text-align: left;
    overflow-wrap: break-word;
    justify-content: flex-start;
    grid-column: 1 / -1;
}
.bench-pending-note {
    grid-column: 1 / -1;
    color: var(--muted);
    font-size: 11px;
}
.bench-inspector label {
    display: grid;
    grid-template-columns: 1fr 92px;
    gap: 5px;
    align-items: center;
    margin-bottom: 6px;
    font-size: 12px;
}
.bench-metadata {
    display: grid;
    gap: 4px;
}
.bench-metadata-row {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(0, 3fr) auto;
    gap: 4px;
}
.bench-metadata-row input[aria-invalid='true'] {
    border-color: var(--danger);
}
.bench-inspector input,
.bench-inspector select {
    width: 100%;
    min-width: 0;
}
.bench-help {
    font-size: 11px;
    color: var(--muted);
    padding: 0 10px;
    line-height: 1.6;
}
.bench-ledger {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    padding: 10px;
    border-top: 1px solid var(--border);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
}
.bench-ledger span {
    display: grid;
    gap: 4px;
}
.bench-ledger b {
    font-size: 13px;
}
.bench-branch {
    display: flex;
    gap: 8px;
    margin: 0 10px 10px;
    align-items: center;
}
.bench-branch select {
    width: 100%;
    min-width: 0;
}
.bench-map-list {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    padding: 10px;
}
.bench-map {
    margin: 0;
    min-width: 0;
}
.bench-map figcaption {
    font-size: 11px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
}
@media (max-width: 1100px) {
    .bench-grid {
        grid-template-columns: 145px minmax(240px, 1fr);
    }
    .bench-inspector {
        grid-column: 1/-1;
        border-left: 0;
        border-top: 1px solid var(--border);
    }
}
</style>
