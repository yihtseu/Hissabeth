<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import type { ApertureReport, OpticalSystem, VignettingUpdateResult } from '../types';
import { clone } from '../validation';

const props = defineProps<{
  /** The saved system: these actions and the coverage report act on it, never on an unapplied draft. */
  system: OpticalSystem;
  revision: number;
  online: boolean;
  dirty: boolean;
  /** The saved view is outdated (changed elsewhere, or a committed change has not refreshed yet). */
  stale: boolean;
  busy: boolean;
  /** Rejects unsaved drafts, runs the operation with the current revision, then reloads. */
  mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
}>();

const DIRECTIONS = { upper_y: 'Upper y', lower_y: 'Lower y', upper_x: 'Upper x', lower_x: 'Lower x' } as const;

const open = ref(false);
const report = ref<ApertureReport | null>(null);
const reportError = ref('');
const loading = ref(false);
const status = ref('');
/** Boundary summary of the last Auto vignetting run; hidden once another revision supersedes it. */
const vigResult = ref<VignettingUpdateResult | null>(null);
let reportToken = 0;

const block = computed(() => {
  if (!props.online) return 'Engine offline.';
  if (props.stale) return 'The saved system changed elsewhere or has not refreshed. Use "Discard draft and reload" (or wait for the view to refresh) first.';
  if (props.dirty) return 'Save or revert edits to run saved-system vignetting';
  return '';
});

/** Shown while its revision is the displayed one, or newer than a refresh that is still pending. */
const summary = computed(() => (vigResult.value && vigResult.value.revision >= props.revision ? vigResult.value : null));
const summaryPending = computed(() => !!summary.value && summary.value.revision > props.revision);

watch(
  () => props.revision,
  (r) => {
    if (vigResult.value && vigResult.value.revision < r) vigResult.value = null;
    status.value = '';
  },
);

/** Only a report for the displayed revision is ever shown, and never while offline or while the saved view is stale. */
const shown = computed(() => (props.online && !props.stale && report.value && report.value.revision === props.revision ? report.value : null));
const reportPending = computed(() => open.value && props.online && !props.stale && !shown.value && !reportError.value);

const surfaceLabel = (i: number | null): string => (i === null ? '—' : String(i + 1));
const endpointName = (e: 'object' | 'image'): string => (e === 'object' ? 'Object' : 'Image');
/** Endpoints are named; a null surface is never shown as an internal surface number. */
const whereLabel = (endpoint: 'object' | 'image' | null, surface: number | null): string =>
  endpoint ? `${endpointName(endpoint)} surface` : surfaceLabel(surface);

async function loadReport(): Promise<void> {
  const token = ++reportToken;
  report.value = null;
  reportError.value = '';
  if (!props.online || props.stale) {
    loading.value = false;
    return;
  }
  loading.value = true;
  try {
    const r = await callTool('aperture_report', {});
    if (token !== reportToken) return;
    if (r.revision !== props.revision) {
      report.value = null;
      reportError.value = `Coverage was computed for revision ${r.revision} but revision ${props.revision} is displayed; it will reload once the view refreshes.`;
    } else {
      report.value = r;
      reportError.value = '';
    }
  } catch (e) {
    if (token === reportToken) {
      report.value = null;
      reportError.value = errorMessage(e);
    }
  } finally {
    if (token === reportToken) loading.value = false;
  }
}

watch(
  [open, () => props.revision, () => props.online, () => props.stale],
  () => {
    if (open.value) void loadReport();
    else {
      reportToken++;
      report.value = null;
      reportError.value = '';
      loading.value = false;
    }
  },
  { immediate: true },
);

async function auto(): Promise<void> {
  status.value = '';
  let out: VignettingUpdateResult | null = null;
  const ok = await props.mutate(async (revision) => {
    out = await callTool('vignetting_update', { revision });
  });
  const result = out as VignettingUpdateResult | null;
  if (!ok || !result) return;
  vigResult.value = result;
  const limited = result.fields.reduce((n, f) => n + f.boundaries.filter((b) => b.status === 'aperture').length, 0);
  status.value = `Revision ${result.revision}: vignetting updated in ${result.iterations} ${result.iterations === 1 ? 'iteration' : 'iterations'} (${limited} aperture-limited ${limited === 1 ? 'boundary' : 'boundaries'}).`;
}

async function clear(): Promise<void> {
  status.value = '';
  vigResult.value = null;
  const next = clone(props.system);
  for (const f of next.fields) f.vignetting = [0, 0, 0, 0];
  const ok = await props.mutate(async (revision) => {
    await callTool('system_set', { system: next, revision });
  });
  if (ok) status.value = 'Vignetting cleared on all fields.';
}
</script>

<template>
  <section class="panel" aria-labelledby="aperture-title">
    <div class="panel-head"><h3 id="aperture-title">Aperture &amp; vignetting</h3><span>Acts on the saved system</span></div>
    <div class="subpanel">
      <h4>Vignetting</h4>
      <div class="controls">
        <button type="button" :disabled="!!block || busy" @click="auto">Auto vignetting</button>
        <button type="button" :disabled="!!block || busy" @click="clear">Clear vignetting</button>
      </div>
      <p class="note">Auto vignetting derives each field's factors from the physical (Clip rays) apertures, ignoring the current factors. Clear vignetting sets every factor to 0. Vignetting factors &lt; 1: positive trims boundary, negative enlarges (no negative floor), 0 = unvignetted.</p>
      <p v-if="block" class="note">{{ block }}</p>
      <p v-if="status" role="status" class="note">{{ status }}</p>
      <div v-if="summary" class="table-scroll" role="region" aria-labelledby="vig-summary-title" tabindex="0">
        <h4 id="vig-summary-title">Computed boundaries (revision {{ summary.revision }}, λ {{ summary.wavelength }} nm, tolerance {{ summary.tolerance }})</h4>
        <p v-if="summaryPending" class="note">The prescription shown is still revision {{ revision }}; it will update once the view refreshes.</p>
        <table>
          <caption class="sr-only">Pupil scale and limiting surface per field and vignetting direction</caption>
          <thead><tr><th scope="col">Field #</th><th v-for="(label, d) in DIRECTIONS" :key="d" scope="col">{{ label }} (scale · limit)</th></tr></thead>
          <tbody>
            <tr v-for="f in summary.fields" :key="f.field_index">
              <th scope="row"><span class="surface-index">{{ f.field_index + 1 }}</span></th>
              <td v-for="(_label, d) in DIRECTIONS" :key="d">
                <template v-for="b in f.boundaries.filter((x) => x.direction === d)" :key="b.direction">
                  {{ b.pupil_scale.toFixed(4) }} · {{ b.status !== 'aperture' ? 'unlimited' : b.limiting_endpoint ? whereLabel(b.limiting_endpoint, null).toLowerCase() : `surface ${surfaceLabel(b.limiting_surface)}` }}
                </template>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
    <details class="data-table wide" :open="open" @toggle="open = ($event.target as HTMLDetailsElement).open">
      <summary>Aperture coverage</summary>
      <p class="note">Saved system only. Draw semi-diameters use the reference wavelength and a 5-ray envelope (chief plus four field-local pupil rim rays) across all fields, ignoring all physical clipping and using actual image-surface intersections. The object size is the radial extent of the field points; the image size is the radial extent of the sampled ray intercepts. Unapplied edits are not reflected.</p>
      <p v-if="!online" class="note">Engine offline; coverage is unavailable.</p>
      <p v-else-if="stale" role="status" class="note">Saved view is updating; coverage will load once it refreshes.</p>
      <p v-else-if="reportPending" role="status" class="note">Loading coverage for revision {{ revision }}…</p>
      <p v-if="reportError" role="alert" class="note warn-text">{{ reportError }}</p>
      <template v-if="shown">
        <p class="note">Revision {{ shown.revision }} · λ {{ shown.wavelength }} nm · {{ shown.sampling }} · {{ shown.sampled_rays - shown.failed_rays }} of {{ shown.sampled_rays }} rays traced completely<span v-if="shown.failed_rays">; {{ shown.failed_rays }} failed (partial rays count only for the surfaces they reached)</span></p>
        <table>
          <caption class="sr-only">Saved drawing semi-diameter, clear radius and reached rays for the object, each surface and the image</caption>
          <thead><tr>
            <th scope="col">Surface / endpoint</th><th scope="col">Draw semi-diam. (mm)</th><th scope="col">Clear radius (mm)</th><th scope="col">Clip rays</th><th scope="col">Reached rays (of {{ shown.sampled_rays }})</th>
          </tr></thead>
          <tbody>
            <tr>
              <th scope="row">Object</th>
              <td>{{ shown.object.semi_diameter }}</td><td>{{ shown.object.aperture }}</td><td>{{ shown.object.clear_aperture ? 'Yes' : 'No' }}</td>
              <td>{{ shown.object.reached_rays }} / {{ shown.sampled_rays }}</td>
            </tr>
            <tr v-for="s in shown.surfaces" :key="s.surface">
              <th scope="row"><span class="surface-index">{{ s.surface + 1 }}</span></th>
              <td>{{ s.semi_diameter }}</td><td>{{ s.aperture }}</td><td>{{ s.clear_aperture ? 'Yes' : 'No' }}</td>
              <td>{{ s.reached_rays }} / {{ shown.sampled_rays }}</td>
            </tr>
            <tr>
              <th scope="row">Image</th>
              <td>{{ shown.image.semi_diameter }}</td><td>{{ shown.image.aperture }}</td><td>{{ shown.image.clear_aperture ? 'Yes' : 'No' }}</td>
              <td>{{ shown.image.reached_rays }} / {{ shown.sampled_rays }}</td>
            </tr>
          </tbody>
        </table>
        <p v-if="!shown.failures.length" class="note">No geometric failures.</p>
        <template v-else>
          <h4 id="ap-fail-title">Geometric failures ({{ shown.failures.length }})</h4>
          <table aria-labelledby="ap-fail-title">
            <caption class="sr-only">Rays that failed before reaching every surface</caption>
            <thead><tr>
              <th scope="col">Field #</th><th scope="col">Pupil (p_T, p_S)</th><th scope="col">Failed surface</th><th scope="col">Traced prefix</th><th scope="col">Failure reason</th>
            </tr></thead>
            <tbody>
              <tr v-for="(f, i) in shown.failures" :key="i">
                <td>{{ f.field_index + 1 }}</td>
                <td>({{ f.pupil[0] }}, {{ f.pupil[1] }})</td>
                <td>{{ whereLabel(f.endpoint, f.surface) }}</td>
                <td>{{ f.reached_surfaces === 0 ? 'none' : `surfaces 1–${f.reached_surfaces}` }}</td>
                <td>{{ f.kind }}: {{ f.error }}</td>
              </tr>
            </tbody>
          </table>
        </template>
      </template>
    </details>
  </section>
</template>
