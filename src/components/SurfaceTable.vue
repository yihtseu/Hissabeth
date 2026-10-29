<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import NumberField from './NumberField.vue';
import NumberListField from './NumberListField.vue';
import type { Interaction, OpticalSystem, Surface, SurfaceEditOperation, SurfaceEditResult } from '../types';
import { INTERACTIONS, canonicalJson, glassKey } from '../validation';
import { callTool, errorMessage } from '../api';

const props = defineProps<{ modelValue: OpticalSystem; materials: string[]; dirty: boolean; online: boolean }>();
const emit = defineEmits<{ 'update:modelValue': [value: OpticalSystem] }>();

const surfaces = computed(() => props.modelValue.surfaces);
const known = computed(() => {
  const set = new Set(props.materials);
  for (const g of props.modelValue.custom_glasses) { set.add(glassKey(g)); set.add(g.name); }
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
watch(() => props.modelValue, () => { draftGeneration++; }, { flush: 'sync', deep: true });
/** Visible while a request runs so the lock is announced; otherwise the last outcome. */
const statusText = computed(() => (pending.value ? 'Computing the surface edit on the engine. The table is locked until it finishes.' : editStatus.value));

/** Positive integer within 1..max, else null; the engine enforces every other rule. */
function parseCount(raw: string, max: number): number | null {
  const t = raw.trim();
  if (!/^\d+$/.test(t)) return null;
  const n = Number(t);
  return n >= 1 && n <= max ? n : null;
}
const insertCount = computed(() => parseCount(insertCountRaw.value, 128 - surfaces.value.length));
const removeMax = computed(() => Math.min(surfaces.value.length - (removeFirst.value - 1), surfaces.value.length - 1));
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
  for (let k = sel; k < map.length; k++) { const m = map[k]; if (m !== null && m !== undefined) return m + 1; }
  for (let k = sel - 2; k >= 0; k--) { const m = map[k]; if (m !== null && m !== undefined) return m + 1; }
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
    if (draftGeneration !== capturedGeneration || canonicalJson(props.modelValue) !== captured) {
      editStatus.value = 'The draft changed while the edit was computed, so the engine reply was discarded. Run it again.';
      return;
    }
    insertBefore.value = remapSelection(insertBefore.value, r);
    removeFirst.value = Math.min(remapSelection(removeFirst.value, r), r.system.surfaces.length);
    const n = operation.count;
    let notice =
      operation.kind === 'insert'
        ? `Inserted ${n} plane dummy ${n === 1 ? 'surface' : 'surfaces'} (zero gap, unclipped, incident medium) before ${operation.before >= oldCount ? 'the image' : `surface ${operation.before + 1}`}.`
        : `Removed ${n} ${n === 1 ? 'surface' : 'surfaces'} starting at surface ${operation.first + 1}.`;
    if (r.stop_reassigned) notice += ` The stop moved to surface ${r.system.stop_index + 1}.`;
    emit('update:modelValue', r.system);
    editStatus.value = notice; // set after the emit so the resulting draft update cannot clear it
  } catch (e) {
    if (token === editToken) editError.value = errorMessage(e); // engine rejections keep the draft and the form
  } finally {
    if (token === editToken) pending.value = false;
  }
}
onBeforeUnmount(() => { editToken++; });

function solveLabel(s: Surface): string {
  const g = s.gap_solve;
  switch (g.kind) {
    case 'fixed': return '';
    case 'pickup': return `Pickup of gap ${g.source + 1}`;
    case 'edge_thickness': return 'Edge thickness solve';
    case 'overall_length': return `Overall length, gaps ${g.first + 1}–${g.last}`;
    case 'marginal_height': return 'Marginal ray height solve';
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
  for (let k = i - 1; k >= 0; k--) if (surfaces.value[k]!.interaction === 'refract') return surfaces.value[k]!.material;
  return props.modelValue.object_material;
}
const materialTitle = (s: Surface, i: number): string =>
  `${s.interaction === 'reflect' ? 'A mirror' : 'A dummy surface'} keeps the incident medium (${incidentMedium(i)}). The stored material "${s.material}" is kept and used again if the type is set back to refract.`;
const CLEAR_HELP =
  'Checked: the clear radius is a circular physical aperture and rays beyond it are blocked. Unchecked: rays are never clipped.';
const DRAW_HELP =
  'Derived drawing radius from the chief and four field-local pupil rim rays at the reference wavelength across all fields. Read-only; it never clips rays and recalculates on Apply.';
</script>

<template>
  <section class="panel" aria-labelledby="rx-title" :aria-busy="pending">
    <div class="panel-head">
      <h3 id="rx-title">Lens prescription</h3>
      <button type="button" class="small" :disabled="!online || pending || surfaces.length >= 128" @click="runEdit({ kind: 'insert', before: surfaces.length, count: 1 })">+ Surface</button>
    </div>
    <datalist id="material-options"><option v-for="m in options" :key="m" :value="m" /></datalist>
    <fieldset class="plain-fieldset" :disabled="pending">
      <legend class="sr-only">Surface rows and structural edits</legend>
      <div class="table-scroll" role="region" aria-labelledby="rx-title" tabindex="0">
        <table>
          <caption class="sr-only">Surface prescription. Each row is one surface; thickness is the distance to the next surface.</caption>
          <thead>
            <tr>
              <th scope="col">Surface</th>
              <th scope="col">Label</th>
              <th scope="col">Type</th>
              <th scope="col">Radius (mm)</th>
              <th scope="col">Thickness (mm)</th>
              <th scope="col">Material after</th>
              <th scope="col">Clear radius (mm)</th>
              <th scope="col" :title="CLEAR_HELP">Clip rays</th>
              <th scope="col" :title="DRAW_HELP">Draw semi-diam. (mm)<span class="solve-tag">Ref λ, 5-ray envelope</span></th>
              <th scope="col">Conic</th>
              <th scope="col">Even A4, A6…</th>
              <th scope="col">Odd A3, A4…</th>
              <th scope="col">Stop</th>
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(s, i) in surfaces" :key="i">
              <th scope="row"><span class="surface-index">{{ i + 1 }}</span></th>
              <td><input type="text" maxlength="256" :aria-label="`Surface ${i + 1} label`" :value="s.label" @input="patch(i, { label: ($event.target as HTMLInputElement).value })" /></td>
              <td>
                <select :aria-label="`Surface ${i + 1} interaction`" :value="s.interaction" @change="patch(i, { interaction: ($event.target as HTMLSelectElement).value as Interaction })">
                  <option v-for="t in INTERACTIONS" :key="t" :value="t">{{ t }}</option>
                </select>
              </td>
              <td><NumberField :aria-label="`Surface ${i + 1} radius`" :model-value="s.radius" @update:model-value="(v: number) => patch(i, { radius: v })" /></td>
              <td>
                <NumberField v-if="s.gap_solve.kind === 'fixed'" :aria-label="`Surface ${i + 1} thickness`" :model-value="s.thickness" @update:model-value="(v: number) => patch(i, { thickness: v })" />
                <template v-else>
                  <NumberField readonly :title="`Resolved by the ${solveLabel(s)}; change it in Gap solve.`" :aria-label="`Surface ${i + 1} thickness (solved: ${solveLabel(s)})`" :model-value="s.thickness" />
                  <span class="solve-tag">{{ solveLabel(s) }}</span>
                </template>
                <span v-if="hasReflection" class="solve-tag" :title="travel[i]! < 0 ? 'Light propagates toward −z after an odd number of reflections: real propagation uses a negative gap. The opposite sign is legal (a virtual path).' : 'Light propagates toward +z: real propagation uses a positive gap. The opposite sign is legal (a virtual path).'">{{ travel[i]! < 0 ? '← −z (negative gap)' : '→ +z (positive gap)' }}</span>
              </td>
              <td>
                <input v-if="retainsMedium(s)" type="text" disabled :title="materialTitle(s, i)" :aria-label="`Surface ${i + 1} material: keeps the incident medium ${incidentMedium(i)} (stored ${s.material} unused)`" :value="incidentMedium(i)" />
                <input v-else type="text" list="material-options" autocomplete="off" spellcheck="false" :aria-label="`Surface ${i + 1} material`" :value="s.material" @input="patch(i, { material: ($event.target as HTMLInputElement).value })" />
              </td>
              <td><NumberField min="0" :aria-label="`Surface ${i + 1} clear radius`" :model-value="s.aperture" @update:model-value="(v: number) => patch(i, { aperture: v })" /></td>
              <td class="center"><input type="checkbox" :title="CLEAR_HELP" :aria-label="`Surface ${i + 1} clip rays outside the clear radius`" :checked="s.clear_aperture" @change="patch(i, { clear_aperture: ($event.target as HTMLInputElement).checked })" /></td>
              <td><NumberField readonly :title="DRAW_HELP" :aria-label="`Surface ${i + 1} draw semi-diameter (derived, read-only)`" :model-value="s.semi_diameter" /></td>
              <td><NumberField :aria-label="`Surface ${i + 1} conic constant`" :model-value="s.conic" @update:model-value="(v: number) => patch(i, { conic: v })" /></td>
              <td><NumberListField allow-empty :aria-label="`Surface ${i + 1} even asphere coefficients`" :model-value="s.even" @update:model-value="(v: number[]) => patch(i, { even: v })" /></td>
              <td><NumberListField allow-empty :aria-label="`Surface ${i + 1} odd asphere coefficients`" :model-value="s.odd" @update:model-value="(v: number[]) => patch(i, { odd: v })" /></td>
              <td class="center"><input type="radio" name="stop-surface" :aria-label="`Surface ${i + 1} is the aperture stop`" :checked="modelValue.stop_index === i" @change="setStop(i)" /></td>
              <td class="row-actions">
                <button type="button" class="small" :disabled="!online || surfaces.length >= 128" :aria-label="`Insert before surface ${i + 1}`" @click="runEdit({ kind: 'insert', before: i, count: 1 })">+</button>
                <button type="button" class="small" :disabled="!online || surfaces.length === 1" :aria-label="`Remove surface ${i + 1}`" @click="runEdit({ kind: 'remove', first: i, count: 1 })">×</button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <div class="subpanel">
        <div class="controls">
          <label for="insert-before">Insert before surface
            <select id="insert-before" v-model.number="insertBefore">
              <option v-for="i in surfaces.length" :key="i" :value="i">{{ i }}</option>
              <option :value="surfaces.length + 1">Image</option>
            </select>
          </label>
          <label for="insert-count">Rows to insert
            <input id="insert-count" type="number" min="1" step="1" :value="insertCountRaw" :aria-invalid="insertCount === null" @input="insertCountRaw = ($event.target as HTMLInputElement).value" />
          </label>
          <button type="button" :disabled="!canInsert" @click="runEdit({ kind: 'insert', before: insertBefore - 1, count: insertCount! })">Insert rows</button>
        </div>
        <div class="controls">
          <label for="remove-first">First surface to remove
            <select id="remove-first" v-model.number="removeFirst">
              <option v-for="i in surfaces.length" :key="i" :value="i">{{ i }}</option>
            </select>
          </label>
          <label for="remove-count">Rows to remove
            <input id="remove-count" type="number" min="1" step="1" :value="removeCountRaw" :aria-invalid="removeCount === null" @input="removeCountRaw = ($event.target as HTMLInputElement).value" />
          </label>
          <button type="button" :disabled="!canRemove" @click="runEdit({ kind: 'remove', first: removeFirst - 1, count: removeCount! })">Remove rows</button>
        </div>
        <p id="surface-edit-status" role="status" class="note">{{ statusText }}</p>
        <p id="surface-edit-error" role="alert" class="json-error">{{ editError }}</p>
      </div>
    </fieldset>
    <div class="legend">
      <span>Radius 0 = plane · Thickness may be negative (the engine checks the real ray path) · Last thickness sets image distance · Solved thicknesses are read-only and show the last evaluated value; they recalculate on Apply, and are edited under Gap solve · Materials (catalog keys or model glass nd:vd) are checked by the engine on apply</span>
      <span>Insert and remove are computed by the engine on the current draft: inserted rows are plane dummy surfaces with zero gap, unclipped, a fixed solve and the incident medium; the engine remaps the stop and every pickup and overall-length gap reference, and rejects a removal when a kept solve refers to a removed gap. Nothing is stored until Apply, where gap solves and draw radii recalculate.</span>
      <span>Reflect and dummy surfaces keep the incident medium, so their material is not used. A centered reflection reverses travel along the axis: after an odd number of reflections real propagation is toward −z and gaps are entered negative. Changing a surface type never edits any gap, material or radius. Tilted or decentered folds are not supported.</span>
      <span>Clip rays on: rays beyond the clear radius are blocked. Off: the clear radius never clips. Draw semi-diam. is derived from the reference-wavelength chief and four field-local pupil rim rays across all fields; it is only drawn, never clips, and recalculates on Apply. While edits are unapplied it shows the last evaluated values.</span>
      <span v-if="dirty" class="unsaved">Unapplied changes</span>
    </div>
  </section>
</template>
