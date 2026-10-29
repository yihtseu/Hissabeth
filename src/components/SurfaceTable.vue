<script setup lang="ts">
import { computed } from 'vue';
import NumberField from './NumberField.vue';
import NumberListField from './NumberListField.vue';
import type { Interaction, OpticalSystem, Surface } from '../types';
import { INTERACTIONS, glassKey, newSurface } from '../validation';

const props = defineProps<{ modelValue: OpticalSystem; materials: string[]; dirty: boolean }>();
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
function add(): void {
  setSurfaces([...surfaces.value, newSurface()]);
}
/** Gap indexes a definition refers to besides its own gap: the pickup source or the [first, last) interval. */
function references(s: Surface, gap: number): boolean {
  const g = s.gap_solve;
  if (g.kind === 'pickup') return g.source === gap;
  if (g.kind === 'overall_length') return g.first <= gap && gap < g.last;
  return false;
}
/** Another surface's definition that refers to gap `i`; the removed surface's own definition goes with it. */
function referencedBy(i: number): number | null {
  const k = surfaces.value.findIndex((s, n) => n !== i && references(s, i));
  return k < 0 ? null : k;
}
function removeHint(i: number): string {
  const k = referencedBy(i);
  return k === null
    ? `Remove surface ${i + 1}`
    : `Cannot remove surface ${i + 1}: the gap solve on surface ${k + 1} refers to its gap. Remove or change that solve first.`;
}
/** Gap indexes above the removed gap move down one so every reference still addresses the same physical gap. */
function shiftSolve(s: Surface, removed: number): Surface {
  const g = s.gap_solve;
  const down = (n: number): number => (n > removed ? n - 1 : n);
  if (g.kind === 'pickup') return { ...s, gap_solve: { ...g, source: down(g.source) } };
  if (g.kind === 'overall_length') return { ...s, gap_solve: { ...g, first: down(g.first), last: down(g.last) } };
  return s;
}
/** Removing a surface keeps the stop on the same physical surface where possible. */
function remove(i: number): void {
  if (referencedBy(i) !== null) return;
  const stop = props.modelValue.stop_index;
  setSurfaces(
    surfaces.value.filter((_, k) => k !== i).map((s) => shiftSolve(s, i)),
    i < stop ? stop - 1 : stop,
  );
}
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
  <section class="panel" aria-labelledby="rx-title">
    <div class="panel-head">
      <h3 id="rx-title">Lens prescription</h3>
      <button type="button" class="small" :disabled="surfaces.length >= 128" @click="add">+ Surface</button>
    </div>
    <datalist id="material-options"><option v-for="m in options" :key="m" :value="m" /></datalist>
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
            <td><button type="button" class="small" :disabled="surfaces.length === 1 || referencedBy(i) !== null" :title="removeHint(i)" :aria-label="removeHint(i)" @click="remove(i)">×</button></td>
          </tr>
        </tbody>
      </table>
    </div>
    <div class="legend">
      <span>Radius 0 = plane · Thickness may be negative (the engine checks the real ray path) · Last thickness sets image distance · Solved thicknesses are read-only and show the last evaluated value; they recalculate on Apply, and are edited under Gap solve · Materials (catalog keys or model glass nd:vd) are checked by the engine on apply</span>
      <span>Reflect and dummy surfaces keep the incident medium, so their material is not used. A centered reflection reverses travel along the axis: after an odd number of reflections real propagation is toward −z and gaps are entered negative. Changing a surface type never edits any gap, material or radius. Tilted or decentered folds are not supported.</span>
      <span>Clip rays on: rays beyond the clear radius are blocked. Off: the clear radius never clips. Draw semi-diam. is derived from the reference-wavelength chief and four field-local pupil rim rays across all fields; it is only drawn, never clips, and recalculates on Apply. While edits are unapplied it shows the last evaluated values.</span>
      <span v-if="dirty" class="unsaved">Unapplied changes</span>
    </div>
  </section>
</template>
