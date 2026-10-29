<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import { tr } from '../i18n';
import {
    BENCH_KINDS,
    benchPorts,
    benchProblems,
    benchRotate,
    type Bench,
    type BenchElement,
    type BenchKind,
    type BenchSource,
    type BenchTrace,
    type BenchAssembly,
    type Vec3,
} from '../bench';
import { canonicalJson, clone } from '../validation';
import NumberField from './NumberField.vue';

const props = defineProps<{
    modelValue: Bench | null | undefined;
    revision: number;
    online: boolean;
    locked: boolean;
    canSave: boolean;
}>();
const emit = defineEmits<{ 'update:modelValue': [bench: Bench | null]; save: [] }>();
const scene = computed(() => props.modelValue);
const selected = ref('');
const mode = ref<'2d' | '3d'>('2d');
const transform = ref<'move' | 'rotate'>('move');
const axis = ref(0);
const selectedElement = computed(() => scene.value?.elements.find((e) => e.id === selected.value));
const selectedSource = computed(() => scene.value?.sources.find((s) => s.id === selected.value));
const entity = computed(() => selectedElement.value ?? selectedSource.value);
const trace = ref<BenchTrace | null>(null);
const assembly = ref<BenchAssembly | null>(null);
const error = ref('');
const pending = ref(false);
const branch = ref('');
const undo = ref<(Bench | null)[]>([]);
const redo = ref<(Bench | null)[]>([]);
const addKind = ref<BenchKind>('mirror');
const addCount = ref(1);
const connectionFrom = ref('');
const connectionPort = ref('out');
const connectionTo = ref('');
const raw = ref('');
const rawError = ref('');
const svg = ref<SVGSVGElement>();
const yaw = ref(-25);
const pitch = ref(35);
const zoom = ref(1);
const pan = ref<[number, number]>([0, 0]);
let token = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let alive = true;
let lastEmitted = '';
const problems = computed(() => benchProblems(scene.value));
const editable = computed(() => props.online && !props.locked);
const allEntities = computed(() => [
    ...(scene.value?.sources ?? []),
    ...(scene.value?.elements ?? []),
]);
const fromPorts = computed(() =>
    benchPorts(
        scene.value?.sources.some((s) => s.id === connectionFrom.value)
            ? 'source'
            : (scene.value?.elements.find((e) => e.id === connectionFrom.value)?.kind ??
                  'detector'),
    ),
);
watch(connectionFrom, () => {
    connectionPort.value = fromPorts.value[0] ?? 'out';
});
const axes = ['X', 'Y', 'Z'];
const angleNames = ['roll', 'pitch', 'yaw'];
function edit(next: Bench | null, remember = true): void {
    if (!editable.value) return;
    if (remember) {
        undo.value.push(clone(scene.value ?? null));
        if (undo.value.length > 100) undo.value.shift();
        redo.value = [];
    }
    lastEmitted = canonicalJson(next);
    emit('update:modelValue', next);
}
function change(fn: (b: Bench) => void): void {
    if (!scene.value) return;
    const next = clone(scene.value);
    fn(next);
    edit(next);
}
function travel(back: boolean): void {
    if (!editable.value) return;
    const source = back ? undo : redo;
    const dest = back ? redo : undo;
    if (!source.value.length) return;
    dest.value.push(clone(scene.value ?? null));
    edit(source.value.pop()!, false);
}
function updateEntity(field: string, value: unknown): void {
    change((b) => {
        const e = [...b.elements, ...b.sources].find((e) => e.id === selected.value);
        if (e) Object.assign(e, { [field]: value });
    });
}
function updateVector(
    field: 'position' | 'rotation' | 'direction',
    i: number,
    value: number,
): void {
    const e = entity.value as (BenchElement & BenchSource) | undefined;
    if (!e) return;
    const v = [...e[field]] as Vec3;
    v[i] = value;
    updateEntity(field, v);
}
function number(b: Bench, prefix: string): string {
    let i = 1;
    while ([...b.elements, ...b.sources].some((e) => e.id === prefix + i)) i++;
    return prefix + i;
}
function addElements(): void {
    if (
        !scene.value ||
        !Number.isInteger(addCount.value) ||
        addCount.value < 1 ||
        addCount.value + scene.value.elements.length > 128
    )
        return;
    change((b) => {
        for (let i = 0; i < addCount.value; i++) {
            const id = number(b, 'E');
            b.elements.push({
                id,
                label: id,
                kind: addKind.value,
                position: [
                    100 + (b.elements.length % 12) * 50,
                    100 + Math.floor(b.elements.length / 12) * 50,
                    75,
                ],
                rotation: [0, 0, 0],
                aperture: 12.5,
                body_radius: 15,
                focal_length: 100,
                reflectance:
                    addKind.value === 'mirror' ? 1 : addKind.value === 'splitter' ? 0.5 : 0,
                transmittance:
                    addKind.value === 'mirror' ? 0 : addKind.value === 'splitter' ? 0.5 : 1,
                mount: null,
            });
            selected.value = id;
        }
    });
}
function addSource(): void {
    change((b) => {
        if (b.sources.length >= 16) return;
        const id = number(b, 'S');
        b.sources.push({
            id,
            position: [50, 100, 75],
            direction: [1, 0, 0],
            wavelength: 532,
            power: 1,
            beam_radius: 2,
            rings: 1,
        });
        selected.value = id;
    });
}
function remove(): void {
    if (selectedSource.value && scene.value?.sources.length === 1) return;
    change((b) => {
        b.elements = b.elements.filter((e) => e.id !== selected.value);
        b.sources = b.sources.filter((e) => e.id !== selected.value);
        b.connections = b.connections.filter(
            (c) => c.from !== selected.value && c.to !== selected.value,
        );
    });
    selected.value = '';
}
async function example(): Promise<void> {
    const before = canonicalJson(scene.value);
    const revision = props.revision;
    error.value = '';
    try {
        const reply = await callTool('bench_example', {});
        if (
            alive &&
            editable.value &&
            before === canonicalJson(scene.value) &&
            revision === props.revision
        )
            edit(reply.bench);
    } catch (e) {
        if (alive) error.value = errorMessage(e);
    }
}
async function run(): Promise<void> {
    if (!scene.value || !props.online || problems.value.length) return;
    const request = ++token;
    const snapshot = clone(scene.value);
    const input = canonicalJson(snapshot);
    const revision = props.revision;
    pending.value = true;
    error.value = '';
    try {
        const reply = await callTool('bench_trace', { bench: snapshot });
        if (
            alive &&
            request === token &&
            props.online &&
            revision === props.revision &&
            reply.revision === revision &&
            input === canonicalJson(scene.value)
        )
            trace.value = reply;
    } catch (e) {
        if (alive && request === token) error.value = errorMessage(e);
    } finally {
        if (alive && request === token) pending.value = false;
    }
}
watch(
    () => [canonicalJson(scene.value), props.revision, props.online],
    () => {
        token++;
        pending.value = false;
        trace.value = null;
        assembly.value = null;
        branch.value = '';
        clearTimeout(timer);
        if (canonicalJson(scene.value) !== lastEmitted) {
            undo.value = [];
            redo.value = [];
        }
        if (scene.value && props.online && !problems.value.length)
            timer = setTimeout(() => void run(), 160);
    },
    { immediate: true },
);
onBeforeUnmount(() => {
    alive = false;
    token++;
    clearTimeout(timer);
});
function download(name: string, text: string, type = 'application/json'): void {
    const url = URL.createObjectURL(new Blob([text], { type }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
}
async function exportAssembly(csv = false): Promise<void> {
    if (!scene.value || !props.online || problems.value.length) return;
    const input = canonicalJson(scene.value);
    const revision = props.revision;
    try {
        const result = await callTool('bench_assembly', { bench: clone(scene.value) });
        if (
            !alive ||
            !props.online ||
            input !== canonicalJson(scene.value) ||
            revision !== props.revision ||
            result.revision !== revision
        )
            return;
        assembly.value = result;
        if (csv) {
            const cell = (v: unknown) => '"' + String(v).replace(/"/g, '""') + '"';
            const rows = [
                [
                    'ID',
                    'Label',
                    'Kind',
                    'X_mm',
                    'Y_mm',
                    'Z_mm',
                    'Roll_deg',
                    'Pitch_deg',
                    'Yaw_deg',
                    'Normal_X',
                    'Normal_Y',
                    'Normal_Z',
                    'Hole_X',
                    'Hole_Y',
                    'Offset_X_mm',
                    'Offset_Y_mm',
                    'Mount',
                ],
                ...result.items.map((e) => [
                    e.id,
                    e.label,
                    e.kind,
                    ...e.position,
                    ...e.rotation,
                    ...e.normal,
                    ...e.hole,
                    ...e.hole_offset,
                    e.mount?.part ?? 'Unspecified',
                ]),
                ...result.sources.map((s) => [
                    s.id,
                    s.id,
                    'source',
                    ...s.position,
                    '',
                    '',
                    '',
                    ...s.direction,
                    '',
                    '',
                    '',
                    '',
                    'Unspecified',
                ]),
            ];
            download(
                'bench-assembly.csv',
                rows.map((r) => r.map(cell).join(',')).join('\r\n'),
                'text/csv',
            );
        } else download('bench-assembly.json', JSON.stringify(result, null, 2));
    } catch (e) {
        if (alive) error.value = errorMessage(e);
    }
}
function applyRaw(): void {
    try {
        const value: unknown = JSON.parse(raw.value);
        const p = benchProblems(value);
        if (p.length || value == null) throw Error(p[0] ?? tr('bench.invalid'));
        edit(value as Bench);
        rawError.value = '';
    } catch (e) {
        rawError.value = errorMessage(e);
    }
}
function connect(): void {
    change((b) => {
        b.connections = b.connections.filter(
            (c) => !(c.from === connectionFrom.value && c.port === connectionPort.value),
        );
        b.connections.push({
            from: connectionFrom.value,
            port: connectionPort.value,
            to: connectionTo.value,
        });
    });
}
function addMount(): void {
    updateEntity('mount', { part: '', base_size: [30, 30], height_range: [40, 150] });
}
function mountField(field: string, value: unknown): void {
    if (selectedElement.value?.mount)
        updateEntity('mount', { ...selectedElement.value.mount, [field]: value });
}
const warnings = computed(() => assembly.value?.warnings ?? trace.value?.warnings ?? []);
const failures = computed(
    () => trace.value?.terminations.filter((t) => t.reason !== 'detected') ?? [],
);
const visibleRays = computed(
    () =>
        trace.value?.segments.filter(
            (s) =>
                !branch.value ||
                s.branch === branch.value ||
                branch.value.startsWith(s.branch + '/'),
        ) ?? [],
);
const basis = (point: Vec3): [number, number] => {
    const b = scene.value;
    if (!b) return [0, 0];
    const x = point[0] - b.origin[0] - b.table_size[0] / 2;
    const y = point[1] - b.origin[1] - b.table_size[1] / 2;
    const z = point[2] - b.origin[2];
    if (mode.value === '2d') return [x, y];
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
function shape(e: BenchElement, radius = e.body_radius): string {
    return path(
        Array.from({ length: 33 }, (_, i) => {
            const a = (i * Math.PI) / 16;
            const v = benchRotate([0, radius * Math.cos(a), radius * Math.sin(a)], e.rotation);
            return e.position.map((x, j) => x + v[j]!) as Vec3;
        }),
    );
}
function mountLines(e: BenchElement): Vec3[][] {
    if (!e.mount || !scene.value) return [];
    const [w, h] = e.mount.base_size;
    const low = scene.value.origin[2];
    const high = Math.max(low, e.position[2] - e.body_radius);
    const corners = Array.from({ length: 8 }, (_, i) => {
        const v = benchRotate(
            [((i % 2 ? 1 : -1) * w) / 2, ((Math.floor(i / 2) % 2 ? 1 : -1) * h) / 2, 0],
            [0, 0, e.rotation[2]],
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
function vectorPoint(e: Vec3, i: number, length = 60): Vec3 {
    return e.map((x, j) => x + (i === j ? length : 0)) as Vec3;
}
type Drag = {
    scene: Bench;
    start: [number, number];
    position: Vec3;
    axis: number | null;
    rotate: boolean;
    camera: boolean;
    pan: boolean;
    yaw: number;
    pitch: number;
    offset: [number, number];
};
let drag: Drag | null = null;
function point(event: PointerEvent): [number, number] {
    const p = svg.value!.createSVGPoint();
    p.x = event.clientX;
    p.y = event.clientY;
    const r = p.matrixTransform(svg.value!.getScreenCTM()!.inverse());
    return [r.x, r.y];
}
function begin(event: PointerEvent, id?: string, handle: number | null = null): void {
    if (!scene.value || drag) return;
    const camera = event.button === 1 || event.button === 2;
    if (!camera && (!editable.value || !id)) return;
    if (id) selected.value = id;
    if (!camera && mode.value === '3d' && handle === null) return;
    const e = allEntities.value.find((e) => e.id === id);
    const position = e?.position ?? [0, 0, 0];
    drag = {
        scene: clone(scene.value),
        start: point(event),
        position: [...position],
        axis: handle,
        rotate: transform.value === 'rotate' && !!selectedElement.value,
        camera,
        pan: event.shiftKey,
        yaw: yaw.value,
        pitch: pitch.value,
        offset: [...pan.value],
    };
    svg.value!.setPointerCapture(event.pointerId);
    event.preventDefault();
    event.stopPropagation();
}
function move(event: PointerEvent): void {
    if (!drag) return;
    const p = point(event);
    const dx = p[0] - drag.start[0];
    const dy = p[1] - drag.start[1];
    if (drag.camera) {
        if (drag.pan) {
            pan.value = [drag.offset[0] + dx, drag.offset[1] + dy];
        } else if (mode.value === '3d') {
            yaw.value = drag.yaw + dx * 0.35;
            pitch.value = Math.max(5, Math.min(85, drag.pitch + dy * 0.25));
        }
        return;
    }
    if (!editable.value) {
        cancel();
        return;
    }
    const next = clone(drag.scene);
    const e = [...next.elements, ...next.sources].find((e) => e.id === selected.value);
    if (!e) return;
    if (drag.rotate && 'rotation' in e) {
        const i = drag.axis ?? 2;
        e.rotation[i] += dx * 0.5;
    } else if (mode.value === '2d' && drag.axis === null) {
        e.position[0] += dx / factor.value;
        e.position[1] -= dy / factor.value;
    } else {
        const i = drag.axis ?? axis.value;
        const a = project(drag.position);
        const b = project(vectorPoint(drag.position, i, 1));
        const vx = b[0] - a[0];
        const vy = b[1] - a[1];
        const den = vx * vx + vy * vy;
        if (den > 1e-6) e.position[i] += (dx * vx + dy * vy) / den;
    }
    edit(next, false);
}
function end(): void {
    if (drag && !drag.camera && canonicalJson(drag.scene) !== canonicalJson(scene.value)) {
        undo.value.push(drag.scene);
        redo.value = [];
    }
    drag = null;
}
function cancel(): void {
    if (drag && !drag.camera && canonicalJson(scene.value) === lastEmitted) {
        lastEmitted = canonicalJson(drag.scene);
        emit('update:modelValue', clone(drag.scene));
    }
    drag = null;
}
function key(event: KeyboardEvent): void {
    if ((event.target as HTMLElement).matches('input,select,textarea')) return;
    if (event.key === 'Escape') {
        cancel();
        event.preventDefault();
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z') {
        travel(!event.shiftKey);
        event.preventDefault();
        event.stopPropagation();
    }
    if (event.key.toLowerCase() === 'w') transform.value = 'move';
    if (event.key.toLowerCase() === 'e') transform.value = 'rotate';
    if (
        ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'PageUp', 'PageDown'].includes(
            event.key,
        ) &&
        entity.value
    ) {
        const i = event.key.startsWith('Page')
            ? 2
            : event.key === 'ArrowLeft' || event.key === 'ArrowRight'
              ? 0
              : 1;
        const sign = ['ArrowLeft', 'ArrowDown', 'PageDown'].includes(event.key) ? -1 : 1;
        const amount = sign * (event.shiftKey ? 10 : event.altKey ? 0.1 : 1);
        if (transform.value === 'rotate' && selectedElement.value)
            updateVector(
                'rotation',
                mode.value === '2d' ? 2 : axis.value,
                selectedElement.value.rotation[mode.value === '2d' ? 2 : axis.value]! + amount,
            );
        else if (mode.value === '3d' || i !== 2)
            updateVector('position', i, entity.value.position[i]! + amount);
        event.preventDefault();
        event.stopPropagation();
    }
}
const fixed = (n: number) => Number(n.toPrecision(6)).toString();
</script>

<template>
    <div class="bench" @keydown="key">
        <div class="bench-tools">
            <button :disabled="!editable" @click="example">{{ tr('bench.example') }}</button>
            <button :disabled="!editable || !undo.length" @click="travel(true)">
                {{ tr('bench.undo') }}
            </button>
            <button :disabled="!editable || !redo.length" @click="travel(false)">
                {{ tr('bench.redo') }}
            </button>
            <button :disabled="!canSave" class="primary" @click="emit('save')">
                {{ tr('app.applyChanges') }}
            </button>
            <span v-if="scene" class="bench-count"
                >{{ scene.elements.length }} / 128 · {{ tr('bench.elements') }}</span
            >
        </div>
        <p v-if="!scene" class="empty">{{ tr('bench.empty') }}</p>
        <p v-if="error" class="banner error" role="alert">{{ error }}</p>
        <p v-for="p in problems" :key="p" class="banner error" role="alert">{{ p }}</p>
        <template v-if="scene">
            <div class="bench-controls">
                <label
                    >{{ tr('bench.name')
                    }}<input
                        :disabled="!editable"
                        :value="scene.name"
                        @change="
                            change((b) => {
                                b.name = ($event.target as HTMLInputElement).value;
                            })
                        "
                /></label>
                <button :aria-pressed="mode === '2d'" @click="mode = '2d'">
                    {{ tr('bench.plan') }}
                </button>
                <button :aria-pressed="mode === '3d'" @click="mode = '3d'">
                    {{ tr('bench.spatial') }}
                </button>
                <button
                    @click="
                        zoom = 1;
                        pan = [0, 0];
                    "
                >
                    {{ tr('bench.fit') }}
                </button>
                <select v-model="transform" :aria-label="tr('bench.transform')">
                    <option value="move">{{ tr('bench.move') }} (W)</option>
                    <option value="rotate">{{ tr('bench.rotate') }} (E)</option>
                </select>
                <select v-model="axis" :aria-label="tr('bench.axis')">
                    <option v-for="(a, i) in axes" :key="a" :value="i">{{ a }}</option>
                </select>
                <button :disabled="!online || pending || !!problems.length" @click="run">
                    {{ pending ? tr('bench.tracing') : tr('bench.trace') }}
                </button>
            </div>
            <div class="bench-grid">
                <div class="bench-tree">
                    <strong>{{ tr('bench.elements') }}</strong>
                    <fieldset :disabled="!editable">
                        <legend class="sr-only">{{ tr('bench.add') }}</legend>
                        <select v-model="addKind" :aria-label="tr('bench.kind')">
                            <option v-for="k in BENCH_KINDS" :key="k" :value="k">
                                {{ tr('bench.kinds.' + k) }}
                            </option>
                        </select>
                        <input
                            v-model.number="addCount"
                            type="number"
                            min="1"
                            max="128"
                            :aria-label="tr('bench.addCount')"
                        />
                        <button
                            :disabled="
                                !Number.isInteger(addCount) ||
                                addCount < 1 ||
                                scene.elements.length + addCount > 128
                            "
                            @click="addElements"
                        >
                            {{ tr('bench.add') }}
                        </button>
                        <button :disabled="scene.sources.length >= 16" @click="addSource">
                            {{ tr('bench.addSource') }}
                        </button>
                    </fieldset>
                    <div role="list" class="bench-element-list">
                        <button
                            v-for="e in allEntities"
                            :key="e.id"
                            :aria-pressed="selected === e.id"
                            @click="selected = e.id"
                        >
                            <b>{{ e.id }}</b>
                            {{ 'kind' in e ? tr('bench.kinds.' + e.kind) : tr('bench.source')
                            }}<small>{{ e.position.map(fixed).join(', ') }} mm</small>
                        </button>
                    </div>
                </div>
                <div class="bench-stage">
                    <svg
                        ref="svg"
                        class="bench-viewport"
                        viewBox="0 0 1000 600"
                        tabindex="0"
                        :aria-label="tr('bench.viewport')"
                        @contextmenu.prevent
                        @pointerdown="begin($event)"
                        @pointermove="move"
                        @pointerup="end"
                        @pointercancel="cancel"
                        @wheel.prevent="
                            zoom = Math.max(
                                0.15,
                                Math.min(8, zoom * ($event.deltaY > 0 ? 0.9 : 1.1)),
                            )
                        "
                    >
                        <polygon :points="floor" class="bench-table" />
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
                        <polyline
                            v-for="(s, i) in visibleRays"
                            :key="'ray' + i"
                            :points="path([s.from, s.to])"
                            class="bench-ray"
                            :class="{
                                bad: !['hit', 'escaped'].includes(s.status),
                                reflected: s.branch.includes('reflected'),
                            }"
                            :style="{
                                opacity: Math.max(
                                    0.18,
                                    Math.min(0.9, (s.power / (trace?.power.launched || 1)) * 6),
                                ),
                            }"
                        >
                            <title>
                                {{ s.branch }} · {{ tr('bench.reasons.' + s.status) }} ·
                                {{ fixed(s.power) }}
                            </title>
                        </polyline>
                        <g
                            v-for="e in scene.elements"
                            :key="e.id"
                            class="bench-object"
                            :class="{ chosen: selected === e.id }"
                            role="button"
                            tabindex="0"
                            :aria-label="tr('bench.selectElement', { id: e.id })"
                            :data-element="e.id"
                            @pointerdown.stop="begin($event, e.id)"
                            @click="selected = e.id"
                            @keydown.enter="selected = e.id"
                        >
                            <polyline :points="shape(e)" class="bench-body" :class="e.kind" />
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
                            @pointerdown.stop="begin($event, s.id)"
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
                        <g v-if="entity && mode === '3d'" class="bench-gizmo">
                            <g
                                v-for="(a, i) in axes"
                                :key="a"
                                :data-axis="a"
                                role="button"
                                tabindex="0"
                                :aria-label="tr('bench.dragAxis', { axis: a })"
                                @pointerdown.stop="begin($event, selected, i)"
                                @keydown.enter="axis = i"
                            >
                                <polyline
                                    :points="
                                        path([entity.position, vectorPoint(entity.position, i)])
                                    "
                                    :class="'axis-' + i"
                                />
                                <circle
                                    :cx="project(vectorPoint(entity.position, i))[0]"
                                    :cy="project(vectorPoint(entity.position, i))[1]"
                                    r="12"
                                    :class="'axis-' + i"
                                />
                                <text
                                    :x="project(vectorPoint(entity.position, i))[0] + 10"
                                    :y="project(vectorPoint(entity.position, i))[1]"
                                >
                                    {{ a }}
                                </text>
                            </g>
                        </g>
                    </svg>
                    <p class="bench-help">
                        {{ mode === '2d' ? tr('bench.planHelp') : tr('bench.spatialHelp') }}
                        {{ tr('bench.keyboardHelp') }}
                    </p>
                    <div v-if="trace" class="bench-ledger" role="status">
                        <span v-for="(value, key) in trace.power" :key="key"
                            >{{ tr('bench.power.' + key) }} <b>{{ fixed(value) }}</b></span
                        ><span
                            >{{ tr('bench.balance') }} <b>{{ fixed(trace.balance_error) }}</b></span
                        >
                    </div>
                    <label v-if="trace" class="bench-branch"
                        >{{ tr('bench.branch')
                        }}<select v-model="branch">
                            <option value="">{{ tr('bench.allBranches') }}</option>
                            <option
                                v-for="t in trace.terminations"
                                :key="t.branch"
                                :value="t.branch"
                            >
                                {{ t.branch }} · {{ t.element ?? '—' }} ·
                                {{ tr('bench.reasons.' + t.reason) }}
                            </option>
                        </select></label
                    >
                </div>
                <fieldset class="bench-inspector" :disabled="!editable">
                    <legend>{{ entity ? selected : tr('bench.selection') }}</legend>
                    <template v-if="entity">
                        <label v-if="selectedElement"
                            >{{ tr('bench.label')
                            }}<input
                                :value="selectedElement.label"
                                @change="
                                    updateEntity('label', ($event.target as HTMLInputElement).value)
                                "
                        /></label>
                        <label v-for="(a, i) in axes" :key="a"
                            >{{ a }} (mm)<NumberField
                                :model-value="entity.position[i]!"
                                :aria-label="tr('bench.position', { axis: a })"
                                @update:model-value="updateVector('position', i, $event)"
                        /></label>
                        <template v-if="selectedElement">
                            <label v-for="(a, i) in angleNames" :key="a"
                                >{{ tr('bench.' + a) }} (°)<NumberField
                                    :model-value="selectedElement.rotation[i]!"
                                    :aria-label="tr('bench.' + a)"
                                    @update:model-value="updateVector('rotation', i, $event)"
                            /></label>
                            <label
                                >{{ tr('bench.aperture') }} (mm)<NumberField
                                    :model-value="selectedElement.aperture"
                                    @update:model-value="updateEntity('aperture', $event)"
                            /></label>
                            <label
                                >{{ tr('bench.bodyRadius') }} (mm)<NumberField
                                    :model-value="selectedElement.body_radius"
                                    @update:model-value="updateEntity('body_radius', $event)"
                            /></label>
                            <label v-if="selectedElement.kind === 'lens'"
                                >{{ tr('bench.focal') }} (mm)<NumberField
                                    :model-value="selectedElement.focal_length"
                                    @update:model-value="updateEntity('focal_length', $event)"
                            /></label>
                            <label v-if="['splitter', 'mirror'].includes(selectedElement.kind)"
                                >{{ tr('bench.reflectance')
                                }}<NumberField
                                    :model-value="selectedElement.reflectance"
                                    @update:model-value="updateEntity('reflectance', $event)"
                            /></label>
                            <label v-if="['splitter', 'lens'].includes(selectedElement.kind)"
                                >{{ tr('bench.transmittance')
                                }}<NumberField
                                    :model-value="selectedElement.transmittance"
                                    @update:model-value="updateEntity('transmittance', $event)"
                            /></label>
                            <p v-if="selectedElement.kind === 'lens'" class="bench-help">
                                {{ tr('bench.idealLens') }}
                            </p>
                            <button v-if="!selectedElement.mount" @click="addMount">
                                {{ tr('bench.addMount') }}
                            </button>
                            <template v-if="selectedElement.mount">
                                <label
                                    >{{ tr('bench.mountPart')
                                    }}<input
                                        :value="selectedElement.mount.part"
                                        @change="
                                            mountField(
                                                'part',
                                                ($event.target as HTMLInputElement).value,
                                            )
                                        "
                                /></label>
                                <label v-for="i in [0, 1]" :key="'foot' + i"
                                    >{{
                                        tr(i === 0 ? 'bench.footWidth' : 'bench.footDepth')
                                    }}
                                    (mm)<NumberField
                                        :model-value="selectedElement.mount.base_size[i]!"
                                        @update:model-value="
                                            mountField(
                                                'base_size',
                                                selectedElement!.mount!.base_size.map((x, j) =>
                                                    i === j ? $event : x,
                                                ),
                                            )
                                        "
                                /></label>
                                <label v-for="i in [0, 1]" :key="'height' + i"
                                    >{{
                                        tr(i === 0 ? 'bench.minHeight' : 'bench.maxHeight')
                                    }}
                                    (mm)<NumberField
                                        :model-value="selectedElement.mount.height_range[i]!"
                                        @update:model-value="
                                            mountField(
                                                'height_range',
                                                selectedElement!.mount!.height_range.map((x, j) =>
                                                    i === j ? $event : x,
                                                ),
                                            )
                                        "
                                /></label>
                                <button @click="updateEntity('mount', null)">
                                    {{ tr('bench.removeMount') }}
                                </button>
                            </template>
                        </template>
                        <template v-if="selectedSource">
                            <label v-for="(a, i) in axes" :key="'dir' + a"
                                >{{ tr('bench.direction', { axis: a })
                                }}<NumberField
                                    :model-value="selectedSource.direction[i]!"
                                    @update:model-value="updateVector('direction', i, $event)"
                            /></label>
                            <label
                                v-for="f in [
                                    'wavelength',
                                    'power',
                                    'beam_radius',
                                    'rings',
                                ] as const"
                                :key="f"
                                >{{ tr(f === 'power' ? 'bench.sourcePower' : 'bench.' + f)
                                }}<NumberField
                                    :model-value="selectedSource[f]"
                                    @update:model-value="updateEntity(f, $event)"
                            /></label>
                        </template>
                        <button
                            :disabled="!!selectedSource && scene.sources.length === 1"
                            @click="remove"
                        >
                            {{ tr('bench.remove') }}
                        </button>
                    </template>
                    <p v-else>{{ tr('bench.selectHelp') }}</p>
                </fieldset>
            </div>
            <details class="bench-section">
                <summary>{{ tr('bench.connections') }} ({{ scene.connections.length }})</summary>
                <fieldset :disabled="!editable" class="bench-controls">
                    <label
                        >{{ tr('bench.from')
                        }}<select v-model="connectionFrom">
                            <option value="">—</option>
                            <option v-for="e in allEntities" :key="e.id" :value="e.id">
                                {{ e.id }}
                            </option>
                        </select></label
                    ><label
                        >{{ tr('bench.port')
                        }}<select v-model="connectionPort">
                            <option v-for="p in fromPorts" :key="p" :value="p">
                                {{ tr('bench.ports.' + p) }}
                            </option>
                        </select></label
                    ><label
                        >{{ tr('bench.to')
                        }}<select v-model="connectionTo">
                            <option value="">—</option>
                            <option v-for="e in scene.elements" :key="e.id" :value="e.id">
                                {{ e.id }}
                            </option>
                        </select></label
                    ><button
                        :disabled="
                            !connectionFrom || !connectionTo || !fromPorts.includes(connectionPort)
                        "
                        @click="connect"
                    >
                        {{ tr('bench.connect') }}
                    </button>
                </fieldset>
                <table>
                    <thead>
                        <tr>
                            <th>{{ tr('bench.from') }}</th>
                            <th>{{ tr('bench.port') }}</th>
                            <th>{{ tr('bench.to') }}</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="(c, i) in scene.connections" :key="i">
                            <td>{{ c.from }}</td>
                            <td>{{ tr('bench.ports.' + c.port) }}</td>
                            <td>{{ c.to }}</td>
                            <td>
                                <button
                                    :disabled="!editable"
                                    :aria-label="
                                        tr('bench.disconnect', { id: c.from + '/' + c.port })
                                    "
                                    @click="
                                        change((b) => {
                                            b.connections.splice(i, 1);
                                        })
                                    "
                                >
                                    {{ tr('bench.remove') }}
                                </button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </details>
            <details class="bench-section">
                <summary>{{ tr('bench.table') }}</summary>
                <fieldset :disabled="!editable" class="bench-controls">
                    <label v-for="(a, i) in ['X', 'Y']" :key="a"
                        >{{ tr('bench.tableSize', { axis: a })
                        }}<NumberField
                            :model-value="scene.table_size[i]!"
                            @update:model-value="
                                change((b) => {
                                    b.table_size[i] = $event;
                                })
                            " /></label
                    ><label
                        >{{ tr('bench.holePitch')
                        }}<NumberField
                            :model-value="scene.hole_pitch"
                            @update:model-value="
                                change((b) => {
                                    b.hole_pitch = $event;
                                })
                            " /></label
                    ><label v-for="(a, i) in axes" :key="'origin' + a"
                        >{{ tr('bench.origin', { axis: a })
                        }}<NumberField
                            :model-value="scene.origin[i]!"
                            @update:model-value="
                                change((b) => {
                                    b.origin[i] = $event;
                                })
                            " /></label
                    ><label
                        >{{ tr('bench.maxInteractions')
                        }}<NumberField
                            :model-value="scene.max_interactions"
                            @update:model-value="
                                change((b) => {
                                    b.max_interactions = $event;
                                })
                            " /></label
                    ><label
                        >{{ tr('bench.minPower')
                        }}<NumberField
                            :model-value="scene.min_power"
                            @update:model-value="
                                change((b) => {
                                    b.min_power = $event;
                                })
                            "
                    /></label>
                </fieldset>
            </details>
            <div class="bench-section">
                <strong>{{ tr('bench.assembly') }}</strong>
                <p>{{ tr('bench.assemblyHelp') }}</p>
                <div class="bench-controls">
                    <button :disabled="!online || !!problems.length" @click="exportAssembly(false)">
                        {{ tr('bench.exportJson') }}</button
                    ><button :disabled="!online || !!problems.length" @click="exportAssembly(true)">
                        {{ tr('bench.exportCsv') }}
                    </button>
                </div>
                <ul v-if="warnings.length" class="bench-diagnostics">
                    <li v-for="(w, i) in warnings.slice(0, 100)" :key="i">{{ w }}</li>
                </ul>
                <p v-if="warnings.length > 100">{{ tr('bench.capped', { n: warnings.length }) }}</p>
                <table v-if="assembly">
                    <thead>
                        <tr>
                            <th>ID</th>
                            <th>XYZ (mm)</th>
                            <th>{{ tr('bench.normal') }}</th>
                            <th>{{ tr('bench.mountPart') }}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="e in assembly.items" :key="e.id">
                            <td>{{ e.id }}</td>
                            <td>{{ e.position.map(fixed).join(', ') }}</td>
                            <td>{{ e.normal.map(fixed).join(', ') }}</td>
                            <td>{{ e.mount?.part || tr('bench.unresolved') }}</td>
                        </tr>
                    </tbody>
                </table>
                <table v-if="assembly">
                    <thead>
                        <tr>
                            <th>{{ tr('bench.mountPart') }}</th>
                            <th>{{ tr('bench.addCount') }}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="part in assembly.bom" :key="part[0]">
                            <td>{{ part[0] }}</td>
                            <td>{{ part[1] }}</td>
                        </tr>
                    </tbody>
                </table>
            </div>
            <details v-if="failures.length" class="bench-section" open>
                <summary>{{ tr('bench.failures') }} ({{ failures.length }})</summary>
                <ul class="bench-diagnostics">
                    <li v-for="(f, i) in failures.slice(0, 100)" :key="i">
                        <button
                            @click="
                                selected = f.element ?? '';
                                branch = f.branch;
                            "
                        >
                            {{ f.element ?? f.source }}: {{ tr('bench.reasons.' + f.reason) }} ({{
                                fixed(f.power)
                            }})
                        </button>
                    </li>
                </ul>
            </details>
            <details class="bench-section">
                <summary>{{ tr('bench.raw') }}</summary>
                <button @click="raw = JSON.stringify(scene, null, 2)">
                    {{ tr('bench.readDraft') }}</button
                ><textarea
                    v-model="raw"
                    :aria-label="tr('bench.raw')"
                    rows="10"
                    spellcheck="false"
                /><button :disabled="!editable" @click="applyRaw">
                    {{ tr('bench.applyRaw') }}
                </button>
                <p v-if="rawError" role="alert">{{ rawError }}</p>
            </details>
            <p class="bench-help">{{ tr('bench.scope') }}</p>
        </template>
    </div>
</template>

<style scoped>
.bench {
    min-width: 0;
}
.bench-tools,
.bench-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 10px;
}
.bench-controls label {
    display: flex;
    gap: 6px;
    align-items: center;
}
.bench-count {
    margin-left: auto;
    color: var(--muted);
}
.bench-grid {
    display: grid;
    grid-template-columns: 170px minmax(280px, 1fr) 205px;
    border: 1px solid var(--border);
    background: var(--panel);
}
.bench-tree {
    border-right: 1px solid var(--border);
    padding: 10px;
    min-width: 0;
}
.bench-tree fieldset {
    display: grid;
    grid-template-columns: 1fr 55px;
    gap: 4px;
    padding: 8px 0;
    border: 0;
}
.bench-tree fieldset button {
    grid-column: 1/-1;
}
.bench-element-list {
    max-height: 510px;
    overflow: auto;
    display: grid;
    gap: 3px;
}
.bench-element-list button {
    text-align: left;
    overflow-wrap: anywhere;
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
    min-height: 330px;
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
.bench-ray {
    fill: none;
    stroke: #258ca9;
    stroke-width: 1.5;
    pointer-events: none;
}
.bench-ray.reflected {
    stroke: #be8622;
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
.bench-body.splitter {
    stroke: #8b6db1;
}
.bench-body.lens {
    stroke: #258ca9;
    fill: transparent;
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
    cursor: pointer;
}
.bench-object.chosen .bench-center {
    fill: var(--accent);
    stroke: var(--accent);
    stroke-width: 4;
}
.bench-object text,
.bench-source text,
.bench-gizmo text {
    fill: var(--text);
    font-size: 14px;
    paint-order: stroke;
    stroke: var(--panel);
    stroke-width: 3px;
    pointer-events: none;
}
.bench-source circle {
    fill: #bf535c;
    stroke: var(--panel);
    stroke-width: 2;
}
.bench-gizmo {
    cursor: grab;
}
.bench-gizmo polyline {
    fill: none;
    stroke-width: 4;
    pointer-events: none;
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
.bench-gizmo .axis-0 {
    stroke: #d15454;
    fill: #d15454;
}
.bench-gizmo .axis-1 {
    stroke: #389363;
    fill: #389363;
}
.bench-gizmo .axis-2 {
    stroke: #397bbe;
    fill: #397bbe;
}
.bench-inspector {
    margin: 0;
    border: 0;
    border-left: 1px solid var(--border);
    padding: 10px;
    min-width: 0;
}
.bench-inspector legend {
    font-weight: 650;
}
.bench-inspector label {
    display: grid;
    grid-template-columns: 1fr 85px;
    gap: 5px;
    align-items: center;
    margin-bottom: 7px;
    font-size: 12px;
}
.bench-inspector input {
    width: 100%;
    min-width: 0;
}
.bench-inspector button {
    margin: 4px 0;
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
.bench-section {
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
}
.bench-section summary {
    cursor: pointer;
    font-weight: 600;
    padding-bottom: 6px;
}
.bench-section textarea {
    display: block;
    width: 100%;
    margin: 8px 0;
    font-family: monospace;
}
.bench-section table {
    width: 100%;
    font-size: 12px;
}
.bench-section th,
.bench-section td {
    text-align: left;
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
}
.bench-diagnostics {
    max-height: 180px;
    overflow: auto;
    padding-left: 22px;
    font-size: 12px;
}
.bench-diagnostics button {
    text-align: left;
}
.bench-controls input {
    width: 100px;
}
.bench-controls fieldset {
    border: 0;
}
@media (max-width: 1100px) {
    .bench-grid {
        grid-template-columns: 140px minmax(240px, 1fr);
    }
    .bench-inspector {
        grid-column: 1/-1;
        border-left: 0;
        border-top: 1px solid var(--border);
        display: flex;
        gap: 12px;
        flex-wrap: wrap;
    }
    .bench-inspector label {
        width: 190px;
    }
    .bench-inspector p {
        width: 100%;
    }
}
@media (max-width: 650px) {
    .bench-grid {
        display: flex;
        flex-direction: column;
    }
    .bench-tree {
        border-right: 0;
    }
    .bench-element-list {
        max-height: 130px;
        grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .bench-stage {
        order: -1;
    }
    .bench-viewport {
        min-height: 260px;
    }
    .bench-branch {
        flex-wrap: wrap;
    }
    .bench-controls label {
        max-width: 100%;
    }
    .bench-controls input {
        width: 85px;
    }
}
</style>
