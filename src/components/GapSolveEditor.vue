<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { callTool } from '../api';
import NumberField from './NumberField.vue';
import { canonicalJson } from '../validation';
import type { GapSolve, GapSolveEntry, OpticalSystem, SolveKind, SolveRecalculateResult, SolveResult } from '../types';

const props = defineProps<{
  /** The saved system: solves act on it, never on an unapplied draft. */
  system: OpticalSystem;
  online: boolean;
  dirty: boolean;
  /** The saved view is outdated (changed elsewhere, or a committed change has not refreshed yet). */
  stale: boolean;
  busy: boolean;
  /** Rejects unsaved drafts, runs the operation with the current revision, then reloads. */
  mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
}>();

type ActiveKind = Exclude<SolveKind, 'fixed'>;

const KINDS: { id: ActiveKind; label: string; help: string }[] = [
  { id: 'pickup', label: 'Pickup', help: 'Thickness = source thickness × scale + offset.' },
  { id: 'edge_thickness', label: 'Edge thickness', help: 'Thickness that gives the target edge thickness at a semi-height, using this and the next surface. For the final gap the next surface is the image surface, so its actual sag at that height is included.' },
  { id: 'overall_length', label: 'Overall length', help: 'Thickness that makes the summed thickness of a range of gaps equal the target length.' },
  { id: 'marginal_height', label: 'Marginal ray height', help: 'Thickness after which the marginal ray is at the target height, measured at the next vertex plane (not on a curved profile). Zone is relative to the nominal pupil radius: 0 is the full-aperture paraxial axial ray, ±1 is the nominal rim; a nonzero zone traces the first prescribed field at the reference wavelength. Physical apertures and ray geometry decide whether it is valid.' },
];

// Gap numbers are shown 1-based ("gap after surface n"); converted to zero-based API indexes when sending.
const kind = ref<ActiveKind>('pickup');
const gap = ref(1);
const source = ref(1);
const scale = ref(1);
const offset = ref(0);
const height = ref(5);
const first = ref(1);
const last = ref(1);
const value = ref(0);
const zone = ref(0);
const status = ref('');

const count = computed(() => props.system.surfaces.length);
const surfaceNumbers = computed(() => Array.from({ length: count.value }, (_, i) => i + 1));
const help = computed(() => KINDS.find((k) => k.id === kind.value)!.help);
const solves = computed<GapSolveEntry[]>(() =>
  props.system.surfaces.flatMap((s, gapIndex) =>
    s.gap_solve.kind === 'fixed' ? [] : [{ gap: gapIndex, thickness: s.thickness, definition: s.gap_solve }],
  ),
);
const editing = computed(() => props.system.surfaces[gap.value - 1]?.gap_solve.kind !== 'fixed');

/** Load an active definition into the form fields. */
function load(d: GapSolve): void {
  switch (d.kind) {
    case 'fixed': return;
    case 'pickup': source.value = d.source + 1; scale.value = d.scale; offset.value = d.offset; break;
    case 'edge_thickness': height.value = d.height; value.value = d.value; break;
    case 'overall_length': first.value = d.first + 1; last.value = d.last; value.value = d.value; break;
    case 'marginal_height': value.value = d.value; zone.value = d.zone; break;
  }
  kind.value = d.kind;
}

function select(n: number): void {
  gap.value = n;
  status.value = '';
  // Edit always reloads, even when the gap number is unchanged.
  const d = props.system.surfaces[n - 1]?.gap_solve;
  if (d) load(d);
}

// Load the selected gap's saved relation on mount, on selection change, and when that gap's saved
// definition changes (reload / external edit). The key covers only the definition, so unchanged
// polls and thickness-only edits never reset in-progress typing.
watch(
  () => canonicalJson([gap.value, props.system.surfaces[gap.value - 1]?.gap_solve ?? null]),
  () => {
    const d = props.system.surfaces[gap.value - 1]?.gap_solve;
    if (d) load(d);
  },
  { immediate: true },
);

// Keep choices inside the saved prescription when surfaces are removed or reloaded.
watch(count, (n) => {
  for (const r of [gap, source, first, last]) if (r.value > n) r.value = n;
});

const commonBlock = computed(() => {
  if (!props.online) return 'Engine offline.';
  if (props.stale) return 'The saved system changed elsewhere or has not refreshed. Use "Discard draft and reload" (or wait for the view to refresh) before solving.';
  if (props.dirty) return 'Apply or revert draft changes before solving.';
  return '';
});

const block = computed(() => {
  if (commonBlock.value) return commonBlock.value;
  const g = gap.value;
  if (kind.value === 'pickup' && source.value === g) return 'A pickup must refer to another gap.';
  if (kind.value === 'edge_thickness') {
    if (!(height.value >= 0) || !(value.value >= 0)) return 'Edge height and thickness must be non-negative.';
  }
  if (kind.value === 'overall_length' && !(first.value <= g && g <= last.value))
    return 'The solved gap must lie within the first–last range.';
  return '';
});

function definition(): GapSolve {
  switch (kind.value) {
    case 'pickup': return { kind: 'pickup', source: source.value - 1, scale: scale.value, offset: offset.value };
    case 'edge_thickness': return { kind: 'edge_thickness', height: height.value, value: value.value };
    // The API interval is the half-open gap range [first, last): first..last inclusive when counted from 1.
    case 'overall_length': return { kind: 'overall_length', first: first.value - 1, last: last.value, value: value.value };
    case 'marginal_height': return { kind: 'marginal_height', value: value.value, zone: zone.value };
  }
}

function describe(d: GapSolve): string {
  switch (d.kind) {
    case 'fixed': return 'Fixed';
    case 'pickup': return `Pickup: gap ${d.source + 1} × ${d.scale} + ${d.offset} mm`;
    case 'edge_thickness': return `Edge thickness ${d.value} mm at height ${d.height} mm`;
    case 'overall_length': return `Overall length ${d.value} mm over gaps ${d.first + 1}–${d.last}`;
    case 'marginal_height': return `Marginal ray height ${d.value} mm (zone ${d.zone})`;
  }
}

async function install(g: number, def: GapSolve): Promise<void> {
  status.value = '';
  let solved: SolveResult | null = null;
  const ok = await props.mutate(async (revision) => {
    solved = await callTool('solve', { gap: g, definition: def, revision });
  });
  const result = solved as SolveResult | null;
  if (!ok || !result) return;
  const n = result.solve.gap + 1;
  status.value = result.solve.definition.kind === 'fixed'
    ? `Removed the solve on gap ${n}; it keeps ${result.solve.thickness} mm.`
    : `Gap after surface ${n} resolved to ${result.solve.thickness} mm.`;
}

const apply = (): Promise<void> => install(gap.value - 1, definition());
const remove = (g: number): Promise<void> => install(g, { kind: 'fixed' });

async function recalculate(): Promise<void> {
  status.value = '';
  let out: SolveRecalculateResult | null = null;
  const ok = await props.mutate(async (revision) => {
    out = await callTool('solve_recalculate', { revision });
  });
  const result = out as SolveRecalculateResult | null;
  if (ok && result) status.value = `Recalculated ${result.solves.length} active ${result.solves.length === 1 ? 'solve' : 'solves'}.`;
}
</script>

<template>
  <section class="panel" aria-labelledby="solve-title">
    <div class="panel-head"><h3 id="solve-title">Gap solve</h3><span>Acts on the saved system</span></div>
    <div class="subpanel">
      <h4 id="solve-list-title">Saved solves</h4>
      <p v-if="!solves.length" class="note">No active solves. Every thickness is edited directly in the prescription table.</p>
      <ul v-else class="solve-list" aria-labelledby="solve-list-title">
        <li v-for="s in solves" :key="s.gap">
          <span>Gap after surface {{ s.gap + 1 }}: {{ describe(s.definition) }} → resolved thickness <strong>{{ s.thickness }} mm</strong></span>
          <button type="button" class="small" :disabled="busy" :aria-label="`Edit solve on gap after surface ${s.gap + 1}`" @click="select(s.gap + 1)">Edit</button>
          <button type="button" class="small" :disabled="!!commonBlock || busy" :aria-label="`Remove solve on gap after surface ${s.gap + 1}`" @click="remove(s.gap)">Remove</button>
        </li>
      </ul>
      <div class="controls">
        <button type="button" :disabled="!!commonBlock || busy || !solves.length" @click="recalculate">Recalculate all</button>
      </div>
    </div>
    <div class="subpanel">
      <h4>{{ editing ? 'Edit solve' : 'New solve' }}</h4>
      <div class="controls">
        <label>Gap after surface
          <select v-model.number="gap">
            <option v-for="n in surfaceNumbers" :key="n" :value="n">{{ n }}</option>
          </select>
        </label>
        <label>Solve
          <select v-model="kind">
            <option v-for="k in KINDS" :key="k.id" :value="k.id">{{ k.label }}</option>
          </select>
        </label>
        <label v-if="kind === 'pickup'">Source gap after surface
          <select v-model.number="source">
            <option v-for="n in surfaceNumbers" :key="n" :value="n">{{ n }}</option>
          </select>
        </label>
        <template v-if="kind === 'overall_length'">
          <label>First gap after surface
            <select v-model.number="first">
              <option v-for="n in surfaceNumbers" :key="n" :value="n">{{ n }}</option>
            </select>
          </label>
          <label>Last gap after surface (inclusive)
            <select v-model.number="last">
              <option v-for="n in surfaceNumbers" :key="n" :value="n">{{ n }}</option>
            </select>
          </label>
        </template>
        <label v-if="kind === 'pickup'">Scale <NumberField v-model="scale" aria-label="Pickup scale" /></label>
        <label v-if="kind === 'pickup'">Offset (mm) <NumberField v-model="offset" aria-label="Pickup offset" /></label>
        <label v-if="kind === 'edge_thickness'">Height (mm) <NumberField v-model="height" aria-label="Edge semi-height" /></label>
        <label v-if="kind === 'marginal_height'">Pupil zone (relative to nominal radius) <NumberField v-model="zone" aria-label="Pupil zone" /></label>
        <label v-if="kind !== 'pickup'">{{
          kind === 'edge_thickness' ? 'Target edge thickness (mm)'
          : kind === 'overall_length' ? 'Target overall length (mm)'
          : 'Target ray height (mm)'
        }} <NumberField v-model="value" aria-label="Solve target" /></label>
        <button type="button" class="primary" :disabled="!!block || busy" @click="apply">{{ editing ? 'Update solve' : 'Install solve' }}</button>
      </div>
      <p class="note">{{ help }} The solved thickness may be negative (signed axial gap); ray intersections determine whether the system traces. The engine reports singular or inconsistent constraints.</p>
      <p class="note">Removing a solve keeps its last resolved thickness as an editable value. The final gap's solve also blocks paraxial focus.</p>
      <p v-if="block" class="note">{{ block }}</p>
      <p v-if="status" role="status" class="note">{{ status }}</p>
    </div>
  </section>
</template>
