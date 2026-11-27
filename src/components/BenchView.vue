<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import {
    Crosshair,
    Download,
    Grid3x3,
    List,
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
import { callTool, errorMessage } from '../api';
import { tr } from '../i18n';
import {
    benchPorts,
    benchRotate,
    type Bench,
    type BenchCatalogItem,
    type BenchElement,
    type BenchLabCheck,
    type BenchPolarization,
    type BenchTrace,
    type Vec3,
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
const labCheck = ref<BenchLabCheck | null>(null);
const pending = ref(false);
const labPending = ref(false);
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
const exportFormat = ref<'svg' | 'dxf' | 'stl' | 'alignment_md' | 'csv'>('svg');

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
    'michelson',
    'polarized_beam_combiner',
    'beam_expander_focus',
    'periscope_routing',
] as const;

const canCommit = computed(
    () => props.online && !props.busy && !props.stale && !props.refreshPending && !saving.value,
);

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
            revision === props.revision &&
            result.revision === revision
        ) {
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
    const active = scene.value;
    if (!active || !props.online) {
        labCheck.value = null;
        labPending.value = false;
        return;
    }
    labPending.value = true;
    try {
        const res = await callTool('bench_lab_check', benchDirty.value ? { bench: active } : {});
        if (alive && request === labToken) labCheck.value = res;
    } catch {
        if (alive && request === labToken) labCheck.value = null;
    } finally {
        if (alive && request === labToken) labPending.value = false;
    }
}

let debounceTimer: ReturnType<typeof setTimeout> | undefined;
function schedulePreview(): void {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
        void run();
        void runLabCheck();
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
    clearTimeout(debounceTimer);
});

function ensureDraft(): Bench | null {
    if (!draft.value && props.system.bench) draft.value = clone(props.system.bench);
    return draft.value;
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
    error.value = '';
    statusMsg.value = '';
    try {
        const res = await callTool('bench_auto_aim', {
            bench: active,
            snap_holes: snapHoles.value,
            apply: false,
        });
        draft.value = res.bench;
        statusMsg.value = tr('benchLab.autoAimDone', { n: res.adjusted_elements.length });
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
        const next = clone(draft.value);
        const ok = await props.mutate((rev) =>
            callTool('bench_set', { bench: next, revision: rev }),
        );
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
    if (!b || !selected.value) return;
    const idx = b.elements.findIndex((e) => e.id === selected.value);
    if (idx < 0) return;
    const removedId = b.elements[idx]!.id;
    b.elements.splice(idx, 1);
    b.connections = b.connections.filter((c) => c.from !== removedId && c.to !== removedId);
    selected.value = '';
    schedulePreview();
}

function snapSelectedToHole(): void {
    const b = ensureDraft();
    if (!b || !selected.value) return;
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
    return benchPorts('kind' in e ? e.kind : 'source');
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

const visibleRays = computed(
    () =>
        trace.value?.segments.filter(
            (s) =>
                !branch.value ||
                s.branch === branch.value ||
                branch.value.startsWith(s.branch + '/'),
        ) ?? [],
);
const failures = computed(
    () => trace.value?.terminations.filter((t) => t.reason !== 'detected') ?? [],
);
const problemIds = computed(
    () =>
        new Set(
            (labCheck.value?.issues ?? [])
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
    if (!showGaussian.value || !labCheck.value?.gaussian.segments.length) return [];
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
    if (props.mode !== '2d' || event.button !== 0) return;
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
            target.position[0] = nx;
            target.position[1] = ny;
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

function downloadBlob(content: string, filename: string, mime: string): void {
    const url = URL.createObjectURL(new Blob([content], { type: mime }));
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
}

async function exportDeliverable(): Promise<void> {
    if (!props.online || !scene.value) return;
    error.value = '';
    try {
        if (exportFormat.value === 'csv') {
            const report = await callTool(
                'bench_assembly',
                benchDirty.value ? { bench: scene.value } : {},
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
        const out = await callTool('bench_export_blueprint', {
            ...(benchDirty.value ? { bench: scene.value } : {}),
            format: exportFormat.value,
        });
        downloadBlob(out.content, out.filename, out.mime);
        statusMsg.value = tr('benchLab.exported', { file: out.filename });
    } catch (e) {
        error.value = errorMessage(e);
    }
}

// Exposed so the surrounding layout (and the app toolbar) can drive the unified bench draft.
defineExpose({ saveBench, discardBench });
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
                    <option value="dxf">AutoCAD DXF</option>
                    <option value="stl">3D CAD STL</option>
                    <option value="alignment_md">Lab SOP (Markdown)</option>
                    <option value="csv">Coordinates CSV</option>
                </select>
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
                    </div>
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
                        <!-- Gaussian 1/e² beam envelope polygons -->
                        <polygon
                            v-for="(poly, gi) in gaussianPolygons"
                            :key="'gauss' + gi"
                            :points="poly"
                            class="bench-gaussian"
                        />
                        <polyline
                            v-for="(ray, i) in visibleRays"
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
                            <circle
                                :cx="project(e.position)[0]"
                                :cy="project(e.position)[1]"
                                r="7"
                                class="bench-center"
                            />
                            <text :x="project(e.position)[0] + 10" :y="project(e.position)[1] - 10">
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
                            <circle
                                :cx="project(s.position)[0]"
                                :cy="project(s.position)[1]"
                                r="8"
                            />
                            <text :x="project(s.position)[0] + 12" :y="project(s.position)[1] - 10">
                                {{ s.id }}
                            </text>
                        </g>
                    </svg>
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
                    <div class="panel-head flush">
                        <strong>{{ entity ? selected : tr('bench.selection') }}</strong>
                        <button
                            v-if="entity && 'kind' in entity"
                            type="button"
                            class="small"
                            :aria-label="tr('bench.remove')"
                            @click="deleteSelectedElement"
                        >
                            <Trash2 :size="12" />
                        </button>
                    </div>
                    <template v-if="entity">
                        <label>
                            X (mm)
                            <input
                                type="number"
                                step="1"
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
                                :value="entity.position[2]"
                                @change="
                                    entity.position[2] = Number(
                                        ($event.target as HTMLInputElement).value,
                                    );
                                    schedulePreview();
                                "
                            />
                        </label>
                        <button type="button" class="small" @click="snapSelectedToHole">
                            <Grid3x3 :size="12" /> {{ tr('benchLab.snapSelected') }}
                        </button>

                        <template v-if="'kind' in entity">
                            <label>
                                {{ tr('bench.kind') }}
                                <span>{{ tr('bench.kinds.' + entity.kind) }}</span>
                            </label>
                            <label>
                                {{ tr('bench.autoRotation') }}
                                <input
                                    type="checkbox"
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
                                <label>
                                    Roll (°)
                                    <input
                                        type="number"
                                        step="1"
                                        :value="entity.rotation[0]"
                                        @change="setRotation(entity, 0, $event)"
                                    />
                                </label>
                                <label>
                                    Pitch (°)
                                    <input
                                        type="number"
                                        step="1"
                                        :value="entity.rotation[1]"
                                        @change="setRotation(entity, 1, $event)"
                                    />
                                </label>
                                <label>
                                    Yaw (°)
                                    <input
                                        type="number"
                                        step="1"
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
                :lab-check="labCheck"
                :lab-pending="labPending"
                @select="selected = $event"
                @add-from-catalog="addFromCatalog"
                @add-beam-dump="addBeamDumpAt"
                @refresh-lab="runLabCheck"
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
.bench-inspector {
    margin: 0;
    border: 0;
    border-left: 1px solid var(--border);
    padding: 10px;
    min-width: 0;
}
.bench-inspector label {
    display: grid;
    grid-template-columns: 1fr 92px;
    gap: 5px;
    align-items: center;
    margin-bottom: 6px;
    font-size: 12px;
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
