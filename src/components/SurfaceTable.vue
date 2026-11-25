<script setup lang="ts">
import { tr } from '../i18n';
import { computed, onBeforeUnmount, reactive, ref, watch, watchEffect } from 'vue';
import NumberField from './NumberField.vue';
import NumberListField from './NumberListField.vue';
import { useSurfaceSelection } from '../composables/useSurfaceSelection';
import type {
    Interaction,
    OpticalSystem,
    OptimizeVariable,
    Surface,
    SurfaceEditOperation,
    SurfaceEditResult,
} from '../types';
import { INTERACTIONS, canonicalJson, glassKey } from '../validation';
import { callTool, errorMessage } from '../api';

const props = withDefaults(
    defineProps<{
        modelValue: OpticalSystem;
        materials: string[];
        dirty: boolean;
        online: boolean;
        /** Active optimization variables; a `V` badge shows next to Radius/Thickness/Conic when present. */
        variables?: OptimizeVariable[];
    }>(),
    { variables: () => [] },
);
const emit = defineEmits<{
    'update:modelValue': [value: OpticalSystem];
    'toggle-variable': [variable: OptimizeVariable];
}>();

const surfaces = computed(() => props.modelValue.surfaces);
const known = computed(() => {
    const set = new Set(props.materials);
    for (const g of props.modelValue.custom_glasses) {
        set.add(glassKey(g));
        set.add(g.name);
    }
    return set;
});
const options = computed(() => [...known.value]);

function setSurfaces(next: Surface[], stop = props.modelValue.stop_index): void {
    emit('update:modelValue', {
        ...props.modelValue,
        surfaces: next,
        stop_index: Math.max(0, Math.min(stop, next.length - 1)),
    });
}
function patch(i: number, p: Partial<Surface>): void {
    setSurfaces(surfaces.value.map((s, k) => (k === i ? { ...s, ...p } : s)));
}

// ---- shared surface hover/selection (linked optical layout highlighting) --------

const {
    hoveredSurfaceIndex,
    selectedSurfaceIndex,
    hoverSurface,
    selectSurface,
    clearSurfaceSelection,
} = useSurfaceSelection();
const scrollRegion = ref<HTMLElement | null>(null);

watch(selectedSurfaceIndex, (v) => {
    if (v === null || v < 0 || v >= surfaces.value.length) return;
    requestAnimationFrame(() => {
        const row = scrollRegion.value?.querySelector<HTMLElement>(`[data-surface-row="${v}"]`);
        row?.scrollIntoView({ block: 'nearest' });
    });
});

// ---- progressive disclosure: advanced profile cards in expandable detail rows ---

/** Per-row view state; rows carrying advanced data start expanded so nothing already in the model is hidden. */
const expanded = reactive<Record<number, boolean>>({});
function rowHasData(s: Surface | undefined): boolean {
    if (!s) return false;
    return (
        s.conic !== 0 ||
        s.even.length > 0 ||
        s.odd.length > 0 ||
        !!s.toroid ||
        !!s.zernike ||
        !!s.transform
    );
}
watchEffect(() => {
    surfaces.value.forEach((s, i) => {
        if (!(i in expanded) && rowHasData(s)) expanded[i] = true;
    });
});
function isOpen(i: number): boolean {
    return !!expanded[i];
}
function toggleOpen(i: number): void {
    expanded[i] = !isOpen(i);
}
function openDetail(i: number): void {
    expanded[i] = true;
}
/** Highlight capsules for every advanced profile a row carries. */
function profileChips(s: Surface): string[] {
    const chips: string[] = [];
    if (s.even.length > 0 || s.odd.length > 0) chips.push('Asphere');
    if (s.toroid) chips.push('Toroid');
    if (s.zernike) chips.push('Zernike');
    if (s.transform) chips.push('Tilt/Decenter');
    return chips;
}

// ---- optimization variable badges ------------------------------------------------

type VarKind = 'radius' | 'thickness' | 'conic';
function hasVar(i: number, kind: VarKind): boolean {
    return props.variables.some((v) => v.kind === kind && v.surface === i);
}
function toggleVar(i: number, kind: VarKind): void {
    emit('toggle-variable', { kind, surface: i } satisfies OptimizeVariable);
}

// ---- multi-configuration override markers (read straight from the model) ---------

interface ConfigMark {
    index: number;
    active: boolean;
    title: string;
}
function configMarks(
    i: number,
    kind: 'thickness' | 'radius' | 'conic' | 'aperture' | 'material',
): ConfigMark[] {
    const configs = props.modelValue.configurations ?? [];
    if (!configs.length) return [];
    const active = props.modelValue.active_configuration ?? 0;
    const marks: ConfigMark[] = [];
    configs.forEach((c, index) => {
        const hit = c.overrides.some((o) => o.kind === kind && 'surface' in o && o.surface === i);
        if (!hit) return;
        const name = c.name || tr('cfg.defaultName', { n: index + 1 });
        marks.push({
            index,
            active: index === active,
            title: `${tr(`cfg.kinds.${kind}`)} — ${name}`,
        });
    });
    return marks;
}

// ---- structural edits (engine-computed draft transformations) ------------------

/** 1-based select values; the insert select uses surfaces.length + 1 for "Image". */
const insertBefore = ref(1);
const removeFirst = ref(1);
/** Raw count text: blank, fractional and out-of-range entries stay editable and only disable the action. */
const insertCountRaw = ref('1');
const removeCountRaw = ref('1');
const pending = ref(false);
const editError = ref('');
const editStatus = ref('');
let editToken = 0;
/** Bumped synchronously on every draft change (replacement or in-place), including edits that later revert. */
let draftGeneration = 0;
watch(
    () => props.modelValue,
    () => {
        draftGeneration++;
    },
    { flush: 'sync', deep: true },
);
/** Visible while a request runs so the lock is announced; otherwise the last outcome. */
const statusText = computed(() => (pending.value ? tr('rx.statusPending') : editStatus.value));

/** Positive integer within 1..max, else null; the engine enforces every other rule. */
function parseCount(raw: string, max: number): number | null {
    const t = raw.trim();
    if (!/^\d+$/.test(t)) return null;
    const n = Number(t);
    return n >= 1 && n <= max ? n : null;
}
const insertCount = computed(() => parseCount(insertCountRaw.value, 128 - surfaces.value.length));
const removeMax = computed(() =>
    Math.min(surfaces.value.length - (removeFirst.value - 1), surfaces.value.length - 1),
);
const removeCount = computed(() => parseCount(removeCountRaw.value, removeMax.value));
const canInsert = computed(() => props.online && !pending.value && insertCount.value !== null);
const canRemove = computed(() => props.online && !pending.value && removeCount.value !== null);

watch(
    () => surfaces.value.length,
    (n) => {
        if (insertBefore.value > n + 1) insertBefore.value = n + 1;
        if (insertBefore.value < 1) insertBefore.value = 1;
        if (removeFirst.value > n) removeFirst.value = Math.max(1, n);
    },
);

/** Follow a 1-based select value across an edit using the engine's row map; the Image entry stays last. */
function remapSelection(sel: number, r: SurfaceEditResult): number {
    const map = r.old_to_new;
    if (sel > map.length) return r.system.surfaces.length + 1;
    const mapped = map[sel - 1];
    if (mapped !== null && mapped !== undefined) return mapped + 1;
    for (let k = sel; k < map.length; k++) {
        const m = map[k];
        if (m !== null && m !== undefined) return m + 1;
    }
    for (let k = sel - 2; k >= 0; k--) {
        const m = map[k];
        if (m !== null && m !== undefined) return m + 1;
    }
    return 1;
}

/**
 * Every insert/remove goes through surface_edit with the whole current draft. The reply is applied
 * only when the draft is still exactly the captured one; a changed draft discards the reply instead.
 * The saved model is never written here: the result becomes draft content and is stored on Apply.
 */
async function runEdit(operation: SurfaceEditOperation): Promise<void> {
    if (pending.value) return; // one request at a time
    const token = ++editToken;
    const captured = canonicalJson(props.modelValue);
    const capturedGeneration = draftGeneration;
    const oldCount = props.modelValue.surfaces.length;
    pending.value = true;
    editError.value = '';
    editStatus.value = '';
    try {
        const r = await callTool('surface_edit', { system: props.modelValue, operation });
        if (token !== editToken) return;
        // A draft that changed and returned to the same JSON still counts as edited while the request ran.
        if (
            draftGeneration !== capturedGeneration ||
            canonicalJson(props.modelValue) !== captured
        ) {
            editStatus.value = tr('rx.statusDraftChanged');
            return;
        }
        insertBefore.value = remapSelection(insertBefore.value, r);
        removeFirst.value = Math.min(
            remapSelection(removeFirst.value, r),
            r.system.surfaces.length,
        );
        const n = operation.count;
        let notice =
            operation.kind === 'insert'
                ? `Inserted ${n} plane dummy ${n === 1 ? 'surface' : 'surfaces'} (zero gap, unclipped, incident medium) before ${operation.before >= oldCount ? tr('rx.insertedImage') : tr('rx.insertedSurface', { n: operation.before + 1 })}.`
                : `Removed ${n} ${n === 1 ? 'surface' : 'surfaces'} starting at surface ${operation.first + 1}.`;
        if (r.stop_reassigned) notice += tr('rx.stopMoved', { n: r.system.stop_index + 1 });
        emit('update:modelValue', r.system);
        editStatus.value = notice; // set after the emit so the resulting draft update cannot clear it
    } catch (e) {
        if (token === editToken) editError.value = errorMessage(e); // engine rejections keep the draft and the form
    } finally {
        if (token === editToken) pending.value = false;
    }
}
onBeforeUnmount(() => {
    editToken++;
    clearSurfaceSelection();
});

function solveLabel(s: Surface): string {
    const g = s.gap_solve;
    switch (g.kind) {
        case 'fixed':
            return '';
        case 'pickup':
            return tr('rx.solvePickup', { n: g.source + 1 });
        case 'edge_thickness':
            return tr('rx.solveEdge');
        case 'overall_length':
            return tr('rx.solveOverall', { first: g.first + 1, last: g.last });
        case 'marginal_height':
            return tr('rx.solveMarginal');
    }
}
function setStop(i: number): void {
    emit('update:modelValue', { ...props.modelValue, stop_index: i });
}
/** Real axial travel after each surface: +1 toward +z, -1 after an odd number of centered reflections. */
const travel = computed(() => {
    let direction = 1;
    return surfaces.value.map((s) => {
        if (s.interaction === 'reflect') direction = -direction;
        return direction;
    });
});
const hasReflection = computed(() => surfaces.value.some((s) => s.interaction === 'reflect'));
const retainsMedium = (s: Surface): boolean => s.interaction !== 'refract';
/** Medium a reflect or dummy row retains: the material of the last refracting row above it, else the object medium. */
function incidentMedium(i: number): string {
    for (let k = i - 1; k >= 0; k--)
        if (surfaces.value[k]!.interaction === 'refract') return surfaces.value[k]!.material;
    return props.modelValue.object_material;
}
const materialTitle = (s: Surface, i: number): string =>
    tr(s.interaction === 'reflect' ? 'rx.mirrorKeeps' : 'rx.dummyKeeps', {
        medium: incidentMedium(i),
        material: s.material,
    });
const CLEAR_HELP = computed(() => tr('rx.clearHelp'));
const DRAW_HELP = computed(() => tr('rx.drawHelp'));
</script>

<template>
    <section class="panel" aria-labelledby="rx-title" :aria-busy="pending">
        <div class="panel-head">
            <h3 id="rx-title">{{ tr('rx.title') }}</h3>
            <button
                type="button"
                class="small"
                :disabled="!online || pending || surfaces.length >= 128"
                @click="runEdit({ kind: 'insert', before: surfaces.length, count: 1 })"
            >
                {{ tr('rx.addSurface') }}
            </button>
        </div>
        <datalist id="material-options">
            <option v-for="m in options" :key="m" :value="m" />
        </datalist>
        <fieldset class="plain-fieldset" :disabled="pending">
            <legend class="sr-only">{{ tr('rx.fieldsetLegend') }}</legend>
            <div
                ref="scrollRegion"
                class="table-scroll"
                role="region"
                aria-labelledby="rx-title"
                tabindex="0"
            >
                <table>
                    <caption class="sr-only">
                        {{
                            tr('rx.caption')
                        }}
                    </caption>
                    <thead>
                        <tr>
                            <th scope="col">{{ tr('rx.colSurface') }}</th>
                            <th scope="col">{{ tr('rx.colLabel') }}</th>
                            <th scope="col">{{ tr('rx.colType') }}</th>
                            <th scope="col">{{ tr('rx.colRadius') }}</th>
                            <th scope="col">{{ tr('rx.colThickness') }}</th>
                            <th scope="col">{{ tr('rx.colMaterial') }}</th>
                            <th scope="col">{{ tr('rx.colClear') }}</th>
                            <th scope="col" :title="CLEAR_HELP">{{ tr('rx.colClip') }}</th>
                            <th scope="col" :title="DRAW_HELP">
                                {{ tr('rx.colDraw')
                                }}<span class="solve-tag">{{ tr('rx.colDrawSub') }}</span>
                            </th>
                            <th scope="col">
                                <span class="sr-only">{{ tr('common.actions') }}</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        <template v-for="(s, i) in surfaces" :key="i">
                            <tr
                                class="rx-row"
                                :class="{
                                    hovered: hoveredSurfaceIndex === i,
                                    selected: selectedSurfaceIndex === i,
                                }"
                                :id="`rx-surface-row-${i}`"
                                :data-surface-row="i"
                                @mouseenter="hoverSurface(i)"
                                @mouseleave="hoverSurface(null)"
                                @click="selectSurface(i)"
                            >
                                <th scope="row" class="row-head">
                                    <div class="row-idline">
                                        <button
                                            type="button"
                                            class="row-toggle"
                                            :aria-expanded="isOpen(i)"
                                            :aria-controls="`rx-detail-${i}`"
                                            :aria-label="`Surface ${i + 1} details`"
                                            @click.stop="toggleOpen(i)"
                                        >
                                            {{ isOpen(i) ? '▾' : '▸' }}
                                        </button>
                                        <span class="surface-index">{{ i + 1 }}</span>
                                        <input
                                            type="radio"
                                            name="stop-surface"
                                            :aria-label="tr('rx.surfaceStop', { n: i + 1 })"
                                            :checked="modelValue.stop_index === i"
                                            @change="setStop(i)"
                                        />
                                    </div>
                                    <div v-if="profileChips(s).length" class="profile-chips">
                                        <button
                                            v-for="chip in profileChips(s)"
                                            :key="chip"
                                            type="button"
                                            class="profile-chip"
                                            :aria-label="`Surface ${i + 1} ${chip} details`"
                                            @click.stop="openDetail(i)"
                                        >
                                            {{ chip }}
                                        </button>
                                    </div>
                                </th>
                                <td>
                                    <input
                                        type="text"
                                        maxlength="256"
                                        :aria-label="tr('rx.surfaceLabel', { n: i + 1 })"
                                        :value="s.label"
                                        @input="
                                            patch(i, {
                                                label: ($event.target as HTMLInputElement).value,
                                            })
                                        "
                                    />
                                </td>
                                <td>
                                    <select
                                        :aria-label="tr('rx.surfaceInteraction', { n: i + 1 })"
                                        :value="s.interaction"
                                        @change="
                                            patch(i, {
                                                interaction: ($event.target as HTMLSelectElement)
                                                    .value as Interaction,
                                            })
                                        "
                                    >
                                        <option v-for="t in INTERACTIONS" :key="t" :value="t">
                                            {{ tr(`reportData.${t}`) }}
                                        </option>
                                    </select>
                                </td>
                                <td>
                                    <div class="cell-line">
                                        <div class="field-stack">
                                            <NumberField
                                                :aria-label="tr('rx.surfaceRadius', { n: i + 1 })"
                                                :model-value="s.radius"
                                                @update:model-value="
                                                    (v: number) => patch(i, { radius: v })
                                                "
                                            />
                                        </div>
                                        <button
                                            type="button"
                                            class="var-badge"
                                            :class="{ on: hasVar(i, 'radius') }"
                                            :aria-pressed="hasVar(i, 'radius')"
                                            :aria-label="`Surface ${i + 1} radius optimization variable`"
                                            title="Toggle optimization variable"
                                            @click.stop="toggleVar(i, 'radius')"
                                        >
                                            V
                                        </button>
                                        <span
                                            v-for="m in configMarks(i, 'radius')"
                                            :key="m.index"
                                            class="cfg-tag"
                                            :class="{ active: m.active }"
                                            :title="m.title"
                                            >C{{ m.index + 1 }}</span
                                        >
                                    </div>
                                </td>
                                <td>
                                    <div class="cell-line">
                                        <div class="field-stack">
                                            <NumberField
                                                v-if="s.gap_solve.kind === 'fixed'"
                                                :aria-label="
                                                    tr('rx.surfaceThickness', { n: i + 1 })
                                                "
                                                :model-value="s.thickness"
                                                @update:model-value="
                                                    (v: number) => patch(i, { thickness: v })
                                                "
                                            />
                                            <template v-else>
                                                <NumberField
                                                    readonly
                                                    :title="
                                                        tr('rx.solvedTitle', {
                                                            solve: solveLabel(s),
                                                        })
                                                    "
                                                    :aria-label="
                                                        tr('rx.solvedThickness', {
                                                            n: i + 1,
                                                            solve: solveLabel(s),
                                                        })
                                                    "
                                                    :model-value="s.thickness"
                                                />
                                                <span
                                                    class="solve-tag solve-mark"
                                                    :title="
                                                        tr('rx.solvedTitle', {
                                                            solve: solveLabel(s),
                                                        })
                                                    "
                                                    >{{ solveLabel(s) }}</span
                                                >
                                            </template>
                                            <span
                                                v-if="hasReflection"
                                                class="solve-tag"
                                                :title="
                                                    travel[i]! < 0
                                                        ? tr('rx.travelNegTip')
                                                        : tr('rx.travelPosTip')
                                                "
                                                >{{
                                                    travel[i]! < 0
                                                        ? tr('rx.travelNeg')
                                                        : tr('rx.travelPos')
                                                }}</span
                                            >
                                        </div>
                                        <button
                                            type="button"
                                            class="var-badge"
                                            :class="{ on: hasVar(i, 'thickness') }"
                                            :aria-pressed="hasVar(i, 'thickness')"
                                            :aria-label="`Surface ${i + 1} thickness optimization variable`"
                                            title="Toggle optimization variable"
                                            @click.stop="toggleVar(i, 'thickness')"
                                        >
                                            V
                                        </button>
                                        <span
                                            v-for="m in configMarks(i, 'thickness')"
                                            :key="m.index"
                                            class="cfg-tag"
                                            :class="{ active: m.active }"
                                            :title="m.title"
                                            >C{{ m.index + 1 }}</span
                                        >
                                    </div>
                                </td>
                                <td>
                                    <div class="cell-line">
                                        <div class="field-stack">
                                            <input
                                                v-if="retainsMedium(s)"
                                                type="text"
                                                disabled
                                                :title="materialTitle(s, i)"
                                                :aria-label="
                                                    tr('rx.materialKept', {
                                                        n: i + 1,
                                                        medium: incidentMedium(i),
                                                        material: s.material,
                                                    })
                                                "
                                                :value="incidentMedium(i)"
                                            />
                                            <input
                                                v-else
                                                type="text"
                                                list="material-options"
                                                autocomplete="off"
                                                spellcheck="false"
                                                :aria-label="tr('rx.surfaceMaterial', { n: i + 1 })"
                                                :value="s.material"
                                                @input="
                                                    patch(i, {
                                                        material: (
                                                            $event.target as HTMLInputElement
                                                        ).value,
                                                    })
                                                "
                                            />
                                        </div>
                                        <span
                                            v-for="m in configMarks(i, 'material')"
                                            :key="m.index"
                                            class="cfg-tag"
                                            :class="{ active: m.active }"
                                            :title="m.title"
                                            >C{{ m.index + 1 }}</span
                                        >
                                    </div>
                                </td>
                                <td>
                                    <div class="cell-line">
                                        <div class="field-stack">
                                            <NumberField
                                                min="0"
                                                :aria-label="tr('rx.surfaceClear', { n: i + 1 })"
                                                :model-value="s.aperture"
                                                @update:model-value="
                                                    (v: number) => patch(i, { aperture: v })
                                                "
                                            />
                                        </div>
                                        <span
                                            v-for="m in configMarks(i, 'aperture')"
                                            :key="m.index"
                                            class="cfg-tag"
                                            :class="{ active: m.active }"
                                            :title="m.title"
                                            >C{{ m.index + 1 }}</span
                                        >
                                    </div>
                                </td>
                                <td class="center">
                                    <input
                                        type="checkbox"
                                        :title="CLEAR_HELP"
                                        :aria-label="tr('rx.surfaceClip', { n: i + 1 })"
                                        :checked="s.clear_aperture"
                                        @change="
                                            patch(i, {
                                                clear_aperture: ($event.target as HTMLInputElement)
                                                    .checked,
                                            })
                                        "
                                    />
                                </td>
                                <td>
                                    <NumberField
                                        readonly
                                        :title="DRAW_HELP"
                                        :aria-label="tr('rx.surfaceDraw', { n: i + 1 })"
                                        :model-value="s.semi_diameter"
                                    />
                                </td>
                                <td class="row-actions">
                                    <button
                                        type="button"
                                        class="small"
                                        :disabled="!online || surfaces.length >= 128"
                                        :aria-label="tr('rx.insertBefore', { n: i + 1 })"
                                        @click="runEdit({ kind: 'insert', before: i, count: 1 })"
                                    >
                                        +
                                    </button>
                                    <button
                                        type="button"
                                        class="small"
                                        :disabled="!online || surfaces.length === 1"
                                        :aria-label="tr('rx.removeSurface', { n: i + 1 })"
                                        @click="runEdit({ kind: 'remove', first: i, count: 1 })"
                                    >
                                        ×
                                    </button>
                                </td>
                            </tr>
                            <tr
                                v-if="isOpen(i)"
                                :id="`rx-detail-${i}`"
                                class="rx-detail-row"
                                @mouseenter="hoverSurface(i)"
                                @mouseleave="hoverSurface(null)"
                                @click="selectSurface(i)"
                            >
                                <td colspan="10">
                                    <div
                                        class="detail-panel"
                                        role="group"
                                        :aria-label="`Surface ${i + 1} advanced profile parameters`"
                                    >
                                        <div class="detail-grid">
                                            <section class="param-card">
                                                <h4 class="param-title">
                                                    {{ tr('rx.colConic') }}
                                                </h4>
                                                <div class="cell-line">
                                                    <div class="field-stack">
                                                        <NumberField
                                                            :aria-label="
                                                                tr('rx.surfaceConic', { n: i + 1 })
                                                            "
                                                            :model-value="s.conic"
                                                            @update:model-value="
                                                                (v: number) =>
                                                                    patch(i, { conic: v })
                                                            "
                                                        />
                                                    </div>
                                                    <button
                                                        type="button"
                                                        class="var-badge"
                                                        :class="{ on: hasVar(i, 'conic') }"
                                                        :aria-pressed="hasVar(i, 'conic')"
                                                        :aria-label="`Surface ${i + 1} conic optimization variable`"
                                                        title="Toggle optimization variable"
                                                        @click.stop="toggleVar(i, 'conic')"
                                                    >
                                                        V
                                                    </button>
                                                    <span
                                                        v-for="m in configMarks(i, 'conic')"
                                                        :key="m.index"
                                                        class="cfg-tag"
                                                        :class="{ active: m.active }"
                                                        :title="m.title"
                                                        >C{{ m.index + 1 }}</span
                                                    >
                                                </div>
                                            </section>
                                            <section class="param-card">
                                                <h4 class="param-title">{{ tr('rx.colEven') }}</h4>
                                                <NumberListField
                                                    allow-empty
                                                    :aria-label="tr('rx.surfaceEven', { n: i + 1 })"
                                                    :model-value="s.even"
                                                    @update:model-value="
                                                        (v: number[]) => patch(i, { even: v })
                                                    "
                                                />
                                            </section>
                                            <section class="param-card">
                                                <h4 class="param-title">{{ tr('rx.colOdd') }}</h4>
                                                <NumberListField
                                                    allow-empty
                                                    :aria-label="tr('rx.surfaceOdd', { n: i + 1 })"
                                                    :model-value="s.odd"
                                                    @update:model-value="
                                                        (v: number[]) => patch(i, { odd: v })
                                                    "
                                                />
                                            </section>
                                            <section class="param-card">
                                                <h4 class="param-title">
                                                    {{ tr('rx.colToroid') }}
                                                </h4>
                                                <template v-if="s.toroid">
                                                    <NumberField
                                                        :aria-label="
                                                            tr('rx.toroidRevolution', { n: i + 1 })
                                                        "
                                                        :model-value="s.toroid.revolution"
                                                        @update:model-value="
                                                            (v: number) =>
                                                                patch(i, {
                                                                    toroid: {
                                                                        ...s.toroid!,
                                                                        revolution: v,
                                                                    },
                                                                })
                                                        "
                                                    />
                                                    <NumberListField
                                                        allow-empty
                                                        :aria-label="
                                                            tr('rx.toroidEvenY', { n: i + 1 })
                                                        "
                                                        :model-value="s.toroid.even_y"
                                                        @update:model-value="
                                                            (v: number[]) =>
                                                                patch(i, {
                                                                    toroid: {
                                                                        ...s.toroid!,
                                                                        even_y: v,
                                                                    },
                                                                })
                                                        "
                                                    />
                                                    <div class="param-actions">
                                                        <button
                                                            type="button"
                                                            class="small"
                                                            :aria-label="
                                                                tr('rx.profileRemove', { n: i + 1 })
                                                            "
                                                            @click.stop="patch(i, { toroid: null })"
                                                        >
                                                            ×
                                                        </button>
                                                    </div>
                                                </template>
                                                <div v-else class="param-actions">
                                                    <button
                                                        type="button"
                                                        class="small"
                                                        :aria-label="
                                                            tr('rx.toroidAdd', { n: i + 1 })
                                                        "
                                                        @click.stop="
                                                            patch(i, {
                                                                toroid: {
                                                                    revolution: 0,
                                                                    even_y: [],
                                                                },
                                                            })
                                                        "
                                                    >
                                                        +
                                                    </button>
                                                </div>
                                            </section>
                                            <section class="param-card">
                                                <h4 class="param-title">
                                                    {{ tr('rx.colZernike') }}
                                                </h4>
                                                <template v-if="s.zernike">
                                                    <NumberField
                                                        min="0"
                                                        :aria-label="
                                                            tr('rx.zernikeNormalize', { n: i + 1 })
                                                        "
                                                        :model-value="s.zernike.normalize"
                                                        @update:model-value="
                                                            (v: number) =>
                                                                patch(i, {
                                                                    zernike: {
                                                                        ...s.zernike!,
                                                                        normalize: v,
                                                                    },
                                                                })
                                                        "
                                                    />
                                                    <NumberListField
                                                        allow-empty
                                                        :aria-label="
                                                            tr('rx.zernikeCoefficients', {
                                                                n: i + 1,
                                                            })
                                                        "
                                                        :model-value="s.zernike.coefficients"
                                                        @update:model-value="
                                                            (v: number[]) =>
                                                                patch(i, {
                                                                    zernike: {
                                                                        ...s.zernike!,
                                                                        coefficients: v,
                                                                    },
                                                                })
                                                        "
                                                    />
                                                    <div class="param-actions">
                                                        <button
                                                            type="button"
                                                            class="small"
                                                            :aria-label="
                                                                tr('rx.profileRemove', { n: i + 1 })
                                                            "
                                                            @click.stop="
                                                                patch(i, { zernike: null })
                                                            "
                                                        >
                                                            ×
                                                        </button>
                                                    </div>
                                                </template>
                                                <div v-else class="param-actions">
                                                    <button
                                                        type="button"
                                                        class="small"
                                                        :aria-label="
                                                            tr('rx.zernikeAdd', { n: i + 1 })
                                                        "
                                                        @click.stop="
                                                            patch(i, {
                                                                zernike: {
                                                                    normalize: 1,
                                                                    coefficients: [],
                                                                },
                                                            })
                                                        "
                                                    >
                                                        +
                                                    </button>
                                                </div>
                                            </section>
                                            <section class="param-card">
                                                <h4 class="param-title">
                                                    {{ tr('rx.colTransform') }}
                                                </h4>
                                                <div v-if="s.transform" class="transform-block">
                                                    <div class="transform-grid">
                                                        <NumberField
                                                            :aria-label="`S${i + 1} dx`"
                                                            :model-value="s.transform.decenter[0]"
                                                            title="dx (mm)"
                                                            @update:model-value="
                                                                (v: number) =>
                                                                    patch(i, {
                                                                        transform: {
                                                                            ...s.transform!,
                                                                            decenter: [
                                                                                v,
                                                                                s.transform!
                                                                                    .decenter[1],
                                                                            ],
                                                                        },
                                                                    })
                                                            "
                                                        />
                                                        <NumberField
                                                            :aria-label="`S${i + 1} dy`"
                                                            :model-value="s.transform.decenter[1]"
                                                            title="dy (mm)"
                                                            @update:model-value="
                                                                (v: number) =>
                                                                    patch(i, {
                                                                        transform: {
                                                                            ...s.transform!,
                                                                            decenter: [
                                                                                s.transform!
                                                                                    .decenter[0],
                                                                                v,
                                                                            ],
                                                                        },
                                                                    })
                                                            "
                                                        />
                                                        <NumberField
                                                            :aria-label="`S${i + 1} rx`"
                                                            :model-value="s.transform.tilt[0]"
                                                            title="rx (deg)"
                                                            @update:model-value="
                                                                (v: number) =>
                                                                    patch(i, {
                                                                        transform: {
                                                                            ...s.transform!,
                                                                            tilt: [
                                                                                v,
                                                                                s.transform!
                                                                                    .tilt[1],
                                                                                s.transform!
                                                                                    .tilt[2],
                                                                            ],
                                                                        },
                                                                    })
                                                            "
                                                        />
                                                        <NumberField
                                                            :aria-label="`S${i + 1} ry`"
                                                            :model-value="s.transform.tilt[1]"
                                                            title="ry (deg)"
                                                            @update:model-value="
                                                                (v: number) =>
                                                                    patch(i, {
                                                                        transform: {
                                                                            ...s.transform!,
                                                                            tilt: [
                                                                                s.transform!
                                                                                    .tilt[0],
                                                                                v,
                                                                                s.transform!
                                                                                    .tilt[2],
                                                                            ],
                                                                        },
                                                                    })
                                                            "
                                                        />
                                                        <NumberField
                                                            :aria-label="`S${i + 1} rz`"
                                                            :model-value="s.transform.tilt[2]"
                                                            title="rz (deg)"
                                                            @update:model-value="
                                                                (v: number) =>
                                                                    patch(i, {
                                                                        transform: {
                                                                            ...s.transform!,
                                                                            tilt: [
                                                                                s.transform!
                                                                                    .tilt[0],
                                                                                s.transform!
                                                                                    .tilt[1],
                                                                                v,
                                                                            ],
                                                                        },
                                                                    })
                                                            "
                                                        />
                                                    </div>
                                                    <div class="param-actions">
                                                        <label class="inline">
                                                            <input
                                                                type="checkbox"
                                                                :checked="s.transform.tilt_first"
                                                                @change="
                                                                    patch(i, {
                                                                        transform: {
                                                                            ...s.transform!,
                                                                            tilt_first: (
                                                                                $event.target as HTMLInputElement
                                                                            ).checked,
                                                                        },
                                                                    })
                                                                "
                                                            />
                                                            {{ tr('rx.tiltFirst') }}
                                                        </label>
                                                        <label class="inline">
                                                            <input
                                                                type="checkbox"
                                                                :checked="s.transform.return_after"
                                                                @change="
                                                                    patch(i, {
                                                                        transform: {
                                                                            ...s.transform!,
                                                                            return_after: (
                                                                                $event.target as HTMLInputElement
                                                                            ).checked,
                                                                        },
                                                                    })
                                                                "
                                                            />
                                                            {{ tr('rx.returnAfter') }}
                                                        </label>
                                                        <button
                                                            type="button"
                                                            class="small"
                                                            :aria-label="
                                                                tr('rx.profileRemove', { n: i + 1 })
                                                            "
                                                            @click.stop="
                                                                patch(i, { transform: null })
                                                            "
                                                        >
                                                            ×
                                                        </button>
                                                    </div>
                                                </div>
                                                <div v-else class="param-actions">
                                                    <button
                                                        type="button"
                                                        class="small"
                                                        :aria-label="
                                                            tr('rx.transformAdd', { n: i + 1 })
                                                        "
                                                        @click.stop="
                                                            patch(i, {
                                                                transform: {
                                                                    decenter: [0, 0],
                                                                    tilt: [0, 0, 0],
                                                                    tilt_first: false,
                                                                    return_after: false,
                                                                },
                                                            })
                                                        "
                                                    >
                                                        +
                                                    </button>
                                                </div>
                                            </section>
                                        </div>
                                    </div>
                                </td>
                            </tr>
                        </template>
                    </tbody>
                </table>
            </div>
            <div class="subpanel">
                <div class="controls">
                    <label for="insert-before"
                        >{{ tr('rx.insertBeforeLabel') }}
                        <select id="insert-before" v-model.number="insertBefore">
                            <option v-for="i in surfaces.length" :key="i" :value="i">
                                {{ i }}
                            </option>
                            <option :value="surfaces.length + 1">{{ tr('rx.insertImage') }}</option>
                        </select>
                    </label>
                    <label for="insert-count"
                        >{{ tr('rx.insertCount') }}
                        <input
                            id="insert-count"
                            type="number"
                            min="1"
                            step="1"
                            :value="insertCountRaw"
                            :aria-invalid="insertCount === null"
                            @input="insertCountRaw = ($event.target as HTMLInputElement).value"
                        />
                    </label>
                    <button
                        type="button"
                        :disabled="!canInsert"
                        @click="
                            runEdit({
                                kind: 'insert',
                                before: insertBefore - 1,
                                count: insertCount!,
                            })
                        "
                    >
                        {{ tr('rx.insertRows') }}
                    </button>
                </div>
                <div class="controls">
                    <label for="remove-first"
                        >{{ tr('rx.removeFirst') }}
                        <select id="remove-first" v-model.number="removeFirst">
                            <option v-for="i in surfaces.length" :key="i" :value="i">
                                {{ i }}
                            </option>
                        </select>
                    </label>
                    <label for="remove-count"
                        >{{ tr('rx.removeCount') }}
                        <input
                            id="remove-count"
                            type="number"
                            min="1"
                            step="1"
                            :value="removeCountRaw"
                            :aria-invalid="removeCount === null"
                            @input="removeCountRaw = ($event.target as HTMLInputElement).value"
                        />
                    </label>
                    <button
                        type="button"
                        :disabled="!canRemove"
                        @click="
                            runEdit({ kind: 'remove', first: removeFirst - 1, count: removeCount! })
                        "
                    >
                        {{ tr('rx.removeRows') }}
                    </button>
                </div>
                <p id="surface-edit-status" role="status" class="note">{{ statusText }}</p>
                <p id="surface-edit-error" role="alert" class="json-error">{{ editError }}</p>
            </div>
        </fieldset>
        <div class="legend">
            <span>{{ tr('rx.legendBasics') }}</span>
            <span>{{ tr('rx.legendEdits') }}</span>
            <span>{{ tr('rx.legendReflect') }}</span>
            <span>{{ tr('rx.legendClip') }}</span>
            <span v-if="dirty" class="unsaved">{{ tr('rx.unsaved') }}</span>
        </div>
    </section>
</template>

<style scoped>
.cell-line {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: flex-start;
}
.field-stack {
    flex: 1 1 90px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
}
.var-badge {
    flex: none;
    width: 22px;
    min-height: 22px;
    padding: 0;
    font-size: 10px;
    font-weight: 700;
    line-height: 1;
    color: var(--muted);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 3px;
}
.var-badge:hover {
    color: var(--accent);
    border-color: var(--accent);
}
.var-badge.on {
    color: var(--accent-ink);
    background: var(--accent);
    border-color: var(--accent);
}
.cfg-tag {
    flex: none;
    align-self: center;
    font-size: 9px;
    line-height: 1;
    padding: 2px 4px;
    border-radius: 3px;
    border: 1px solid var(--border);
    color: var(--muted);
    background: var(--surface);
    white-space: nowrap;
}
.cfg-tag.active {
    color: var(--accent);
    border-color: var(--accent);
}
.row-head {
    min-width: 96px;
}
.row-idline {
    display: flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
}
.row-idline input[type='radio'] {
    width: auto;
    min-width: 0;
    margin: 0;
}
.row-toggle {
    min-height: 0;
    padding: 0 3px;
    font-size: 10px;
    line-height: 1.4;
    color: var(--muted);
    background: transparent;
    border-color: transparent;
}
.row-toggle:hover {
    color: var(--accent);
    background: transparent;
}
.profile-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    margin-top: 4px;
}
.profile-chip {
    min-height: 0;
    padding: 1px 6px;
    font-size: 9px;
    line-height: 1.6;
    border-radius: 999px;
    color: var(--accent);
    background: var(--accent-soft);
    border: 1px solid var(--accent);
}
.profile-chip:hover {
    color: var(--accent-ink);
    background: var(--accent);
}
.rx-row.hovered > td,
.rx-row.hovered > th {
    background: var(--hover);
}
.rx-row.selected > td,
.rx-row.selected > th {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 0 var(--accent);
}
.rx-row.selected .surface-index {
    color: var(--accent-ink);
    background: var(--accent);
}
.rx-detail-row > td {
    padding: 0 12px 12px;
    background: var(--surface);
}
.detail-panel {
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: 4px;
}
.detail-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
    gap: 10px;
}
.param-card {
    min-width: 0;
    padding: 8px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 4px;
}
.param-title {
    margin: 0 0 6px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--muted);
}
.param-card > input + input {
    margin-top: 4px;
}
.param-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
}
.transform-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(72px, 1fr));
    gap: 4px;
}
.solve-mark {
    display: inline-block;
    align-self: flex-start;
    padding: 1px 5px;
    color: var(--accent);
    background: var(--accent-soft);
    border: 1px solid var(--accent);
    border-radius: 3px;
    white-space: nowrap;
}
@media (max-width: 900px) {
    .detail-grid {
        grid-template-columns: 1fr;
    }
    .row-head {
        min-width: 0;
    }
}
</style>
