<script setup lang="ts">
import { tr } from '../i18n';
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import NumberField from './NumberField.vue';
import NumberListField from './NumberListField.vue';
import type {
    Interaction,
    OpticalSystem,
    Surface,
    SurfaceEditOperation,
    SurfaceEditResult,
} from '../types';
import { INTERACTIONS, canonicalJson, glassKey } from '../validation';
import { callTool, errorMessage } from '../api';

const props = defineProps<{
    modelValue: OpticalSystem;
    materials: string[];
    dirty: boolean;
    online: boolean;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: OpticalSystem] }>();

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
            <div class="table-scroll" role="region" aria-labelledby="rx-title" tabindex="0">
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
                            <th scope="col">{{ tr('rx.colConic') }}</th>
                            <th scope="col">{{ tr('rx.colEven') }}</th>
                            <th scope="col">{{ tr('rx.colOdd') }}</th>
                            <th scope="col">{{ tr('rx.colStop') }}</th>
                            <th scope="col">
                                <span class="sr-only">{{ tr('common.actions') }}</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="(s, i) in surfaces" :key="i">
                            <th scope="row">
                                <span class="surface-index">{{ i + 1 }}</span>
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
                                <NumberField
                                    :aria-label="tr('rx.surfaceRadius', { n: i + 1 })"
                                    :model-value="s.radius"
                                    @update:model-value="(v: number) => patch(i, { radius: v })"
                                />
                            </td>
                            <td>
                                <NumberField
                                    v-if="s.gap_solve.kind === 'fixed'"
                                    :aria-label="tr('rx.surfaceThickness', { n: i + 1 })"
                                    :model-value="s.thickness"
                                    @update:model-value="(v: number) => patch(i, { thickness: v })"
                                />
                                <template v-else>
                                    <NumberField
                                        readonly
                                        :title="tr('rx.solvedTitle', { solve: solveLabel(s) })"
                                        :aria-label="
                                            tr('rx.solvedThickness', {
                                                n: i + 1,
                                                solve: solveLabel(s),
                                            })
                                        "
                                        :model-value="s.thickness"
                                    />
                                    <span class="solve-tag">{{ solveLabel(s) }}</span>
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
                                        travel[i]! < 0 ? tr('rx.travelNeg') : tr('rx.travelPos')
                                    }}</span
                                >
                            </td>
                            <td>
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
                                            material: ($event.target as HTMLInputElement).value,
                                        })
                                    "
                                />
                            </td>
                            <td>
                                <NumberField
                                    min="0"
                                    :aria-label="tr('rx.surfaceClear', { n: i + 1 })"
                                    :model-value="s.aperture"
                                    @update:model-value="(v: number) => patch(i, { aperture: v })"
                                />
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
                            <td>
                                <NumberField
                                    :aria-label="tr('rx.surfaceConic', { n: i + 1 })"
                                    :model-value="s.conic"
                                    @update:model-value="(v: number) => patch(i, { conic: v })"
                                />
                            </td>
                            <td>
                                <NumberListField
                                    allow-empty
                                    :aria-label="tr('rx.surfaceEven', { n: i + 1 })"
                                    :model-value="s.even"
                                    @update:model-value="(v: number[]) => patch(i, { even: v })"
                                />
                            </td>
                            <td>
                                <NumberListField
                                    allow-empty
                                    :aria-label="tr('rx.surfaceOdd', { n: i + 1 })"
                                    :model-value="s.odd"
                                    @update:model-value="(v: number[]) => patch(i, { odd: v })"
                                />
                            </td>
                            <td class="center">
                                <input
                                    type="radio"
                                    name="stop-surface"
                                    :aria-label="tr('rx.surfaceStop', { n: i + 1 })"
                                    :checked="modelValue.stop_index === i"
                                    @change="setStop(i)"
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
