<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import { rgbCss } from '../color';
import { checkRevision, savedSystemChanged } from '../provenance';
import { describeMapping, describeSampling, percent } from '../sampling';
import type { OpticalSystem, PolychromaticSpot, PupilMapping, PupilSampling, RunInfo, SpotDiagram } from '../types';
import { fieldLabel } from '../validation';
import RaySamplingControl from './RaySamplingControl.vue';
import SpotCanvas from './SpotCanvas.vue';
import SpectralLedger, { type LedgerRow } from './SpectralLedger.vue';

const props = defineProps<{
  system: OpticalSystem;
  spot: SpotDiagram | null;
  /** Provenance of `spot`, captured when its request started. */
  spotRun: RunInfo | null;
  spotPending: boolean;
  sampling: PupilSampling;
  fieldIndex: number;
  wavelengthIndex: number;
  revision: number;
  online: boolean;
}>();
const emit = defineEmits<{
  'update:fieldIndex': [value: number];
  'update:wavelengthIndex': [value: number];
  'update:sampling': [value: PupilSampling];
}>();

const mode = ref<'single' | 'all'>('single');
const poly = ref<PolychromaticSpot | null>(null);
const polyRun = ref<{ field: string; revision: number; mapping: PupilMapping } | null>(null);
const polyPending = ref(false);
const polyError = ref('');
let token = 0;

/** Drops the previous result first, so nothing old stays under changed controls. */
async function loadPoly(): Promise<void> {
  const mine = ++token;
  poly.value = null;
  polyRun.value = null;
  polyError.value = '';
  polyPending.value = false;
  if (mode.value !== 'all' || !props.online) return;
  const field = props.fieldIndex;
  const sampling = { ...props.sampling };
  const run = { field: fieldLabel(props.system, field), revision: props.revision, mapping: props.system.pupil_mapping };
  polyPending.value = true;
  try {
    const r = await callTool('polychromatic_spot', { field_index: field, sampling });
    if (mine !== token) return;
    const check = checkRevision(run.revision, props.revision, r.revision);
    if (check === 'superseded') return;
    if (check === 'newer') {
      polyError.value = savedSystemChanged('Polychromatic spot', run.revision, r.revision);
      return;
    }
    poly.value = r;
    polyRun.value = run;
  } catch (e) {
    if (mine !== token) return;
    polyError.value = errorMessage(e);
  } finally {
    if (mine === token) polyPending.value = false;
  }
}
onBeforeUnmount(() => { token++; });
watch([mode, () => props.fieldIndex, () => props.revision, () => props.online, () => props.sampling], () => void loadPoly(), { immediate: true });

const polyRows = computed<LedgerRow[]>(() =>
  (poly.value?.failures ?? []).map((f) => ({
    wavelength: f.wavelength,
    weight: f.weight,
    transmitted: f.transmitted,
    sampled: f.transmitted + f.rejected,
    fraction: f.transmitted_fraction,
    effective: f.effective_weight,
    color: f.color,
  })),
);

const um = (v: number): string => (v * 1000).toFixed(2);
</script>

<template>
  <section class="panel" aria-labelledby="spot-title">
    <div class="panel-head">
      <h3 id="spot-title">Image-plane intercepts</h3>
      <div class="controls">
        <label>View
          <select v-model="mode">
            <option value="single">Selected wavelength</option>
            <option value="all">All wavelengths</option>
          </select>
        </label>
        <label>Field
          <select :value="fieldIndex" @change="emit('update:fieldIndex', Number(($event.target as HTMLSelectElement).value))">
            <option v-for="(_, i) in system.fields" :key="i" :value="i">{{ fieldLabel(system, i) }}</option>
          </select>
        </label>
        <label v-if="mode === 'single'">Wavelength
          <select :value="wavelengthIndex" @change="emit('update:wavelengthIndex', Number(($event.target as HTMLSelectElement).value))">
            <option v-for="(w, i) in system.wavelengths" :key="i" :value="i">{{ i + 1 }}: {{ w.value }} nm</option>
          </select>
        </label>
        <RaySamplingControl :model-value="sampling" @update:model-value="emit('update:sampling', $event)" />
      </div>
    </div>
    <div v-if="mode === 'all' && polyError" role="alert" class="banner error inset">{{ polyError }}</div>
    <SpotCanvas
      v-if="mode === 'single' && spot && spotRun"
      :points="spot.points"
      :centroid="spot.centroid"
      :color="rgbCss(spotRun.color)"
      :opacity="0.65"
      :label="`Spot diagram of ${spot.points.length} transmitted rays centered on the area-weighted centroid, RMS radius ${um(spot.rms_radius)} micrometers`"
    />
    <SpotCanvas
      v-else-if="mode === 'all' && poly && polyRun"
      :points="poly.points"
      :centroid="poly.centroid"
      :colors="poly.colors"
      :opacity="0.6"
      :label="`Polychromatic spot diagram of ${poly.points.length} transmitted rays centered on the weighted centroid, RMS radius ${um(poly.rms_radius)} micrometers, colored by the saved wavelength colors`"
    />
    <p v-else class="empty" role="status">{{ (mode === 'all' ? polyPending : spotPending) ? 'Computing…' : 'No spot data yet.' }}</p>

    <div v-if="mode === 'single' && spot && spotRun" class="legend" role="status">
      <span>Revision {{ spotRun.revision }} · Field {{ spotRun.field }} · {{ spotRun.wavelength }} nm · {{ describeSampling(spot.sampling) }} · {{ describeMapping(spotRun.mapping) }}</span>
      <span>{{ spot.points.length }} transmitted of {{ spot.sampled_rays }} sampled rays · {{ percent(spot.transmitted_weight) }} of nominal pupil area transmitted</span>
      <span>Invalid: {{ spot.blocked }} blocked · {{ spot.missed }} missed/failed · {{ spot.total_internal_reflection }} total internal reflection</span>
      <span>Area-weighted RMS {{ um(spot.rms_radius) }} µm · geometric radius {{ um(spot.geometric_radius) }} µm · centered on the weighted centroid</span>
    </div>
    <template v-if="mode === 'all' && poly && polyRun">
      <div class="legend" role="status">
        <span>Revision {{ polyRun.revision }} · Field {{ polyRun.field }} · {{ describeSampling(poly.sampling) }} · {{ describeMapping(polyRun.mapping) }} · all positive-weight wavelengths</span>
        <span>{{ poly.points.length }} transmitted of {{ poly.sampled_rays }} sampled rays</span>
        <span>Weighted RMS {{ um(poly.rms_radius) }} µm about the common weighted centroid (no per-wavelength recentering)</span>
        <span>Transmitted spectral-area weight {{ poly.transmitted_weight.toPrecision(4) }}. Zero-weight wavelengths are skipped.</span>
      </div>
      <div class="ledger-wrap">
        <SpectralLedger :rows="polyRows" caption="Spectral and pupil-transmission ledger (sampled aperture transmission only; no coating or Fresnel loss)" />
      </div>
    </template>
  </section>
</template>
