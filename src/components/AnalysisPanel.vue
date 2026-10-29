<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import { checkRevision, savedSystemChanged } from '../provenance';
import { describeSampling, GEOMETRIC_SAMPLING, type SpectralMode } from '../sampling';
import { FALLBACK_COLOR } from '../color';
import type { AnalysisKind, AnalysisOutcome, OpticalSystem, PupilSampling, RGBA, Spectrum } from '../types';
import { fieldLabel } from '../validation';
import AnalysisResult from './AnalysisResult.vue';
import RaySamplingControl from './RaySamplingControl.vue';

const props = defineProps<{ system: OpticalSystem; revision: number; online: boolean; dirty: boolean }>();

// Sampling values follow the product specification.
const CURVE_SAMPLES = 33;
const WAVEFRONT_GRID = 17;
const DIFFRACTION_GRID = 32;

const CURVATURE_MIN = 2;
const CURVATURE_MAX = 257;
const FIELD_CURVATURE_ID: AnalysisKind = 'field_curvature';

interface Params {
  fieldIndex: number; wavelength: number; lower: number; upper: number; sampling: PupilSampling; spectrum: Spectrum;
  /** Field curvature only: sweep samples and the optional endpoint (omitted means +Y at the largest field). */
  curvatureSamples: number; fieldEnd: [number, number] | undefined;
}
interface Def {
  id: AnalysisKind;
  label: string;
  help: string;
  field: boolean;
  wavelength: boolean;
  /** Uses the shared Mono/Weighted spectral mode instead of a bare wavelength (wavelength is then only shown in mono). */
  spectral?: boolean;
  /** Geometric ray sampling control (grid or hexapolar); FFT analyses never offer it. */
  rays?: boolean;
  /** lower/upper wavelengths come from the saved wavelength range */
  range?: boolean;
  sampling: string;
  run: (p: Params) => Promise<AnalysisOutcome>;
}

const DEFS: Def[] = [
  { id: 'ray_fan', label: 'Ray fan', help: 'Image-plane error along the field-local tangential and sagittal axes versus pupil coordinate, relative to the chief ray.', field: true, wavelength: true, sampling: `${CURVE_SAMPLES} samples per fan`,
    run: async (p) => ({ kind: 'ray_fan', data: await callTool('ray_fan', { field_index: p.fieldIndex, wavelength: p.wavelength, samples: CURVE_SAMPLES }) }) },
  { id: 'opd_fan', label: 'OPD fan', help: 'Optical path difference in waves along the field-local tangential and sagittal pupil axes.', field: true, wavelength: true, sampling: `${CURVE_SAMPLES} samples per fan`,
    run: async (p) => ({ kind: 'opd_fan', data: await callTool('opd_fan', { field_index: p.fieldIndex, wavelength: p.wavelength, samples: CURVE_SAMPLES }) }) },
  { id: 'chromatic_focus', label: 'Chromatic focus shift', help: 'Paraxial focus versus wavelength over the saved wavelength range.', field: false, wavelength: false, range: true, sampling: `${CURVE_SAMPLES} wavelengths`,
    run: async (p) => ({ kind: 'chromatic_focus', data: await callTool('chromatic_focus', { lower: p.lower, upper: p.upper, samples: CURVE_SAMPLES }) }) },
  { id: 'longitudinal_aberration', label: 'Longitudinal aberration', help: 'On-axis focus shift versus pupil height.', field: false, wavelength: true, sampling: `${CURVE_SAMPLES} pupil heights`,
    run: async (p) => ({ kind: 'longitudinal_aberration', data: await callTool('longitudinal_aberration', { wavelength: p.wavelength, samples: CURVE_SAMPLES }) }) },
  { id: 'spherochromatism', label: 'Spherochromatism', help: 'Longitudinal focus of every stored wavelength, zero weights included, against normalized pupil height. Curves share one reference focus, so longitudinal chromatic focus stays visible. This is a diagnostic, not a weighted spectrum.', field: false, wavelength: false, sampling: `${CURVE_SAMPLES} pupil heights per wavelength`,
    run: async () => ({ kind: 'spherochromatism', data: await callTool('spherochromatism', { samples: CURVE_SAMPLES }) }) },
  { id: 'field_curvature', label: 'Field curvature', help: 'Sagittal (solid) and tangential (dashed) focus shift against field radius for every stored wavelength, zero weights and duplicates included. The focus is global z relative to the stored image vertex, with no refocusing per wavelength. Computed from the real chief ray with an analytic infinitesimal bundle; the chief ray is clipped by physical apertures, off-chief rays are not.', field: false, wavelength: false, sampling: 'analytic bundle along a radial sweep',
    run: async (p) => ({ kind: 'field_curvature', data: await callTool('field_curvature', p.fieldEnd ? { samples: p.curvatureSamples, field_end: p.fieldEnd } : { samples: p.curvatureSamples }) }) },
  { id: 'astigmatism', label: 'Astigmatism', help: 'Sagittal and tangential focus for the chief ray, from an analytic infinitesimal bundle that differentiates the real intersections, normals and refraction or reflection laws; no neighboring rays are traced.', field: true, wavelength: true, sampling: 'analytic infinitesimal bundle',
    run: async (p) => ({ kind: 'astigmatism', data: await callTool('astigmatism', { field_index: p.fieldIndex, wavelength: p.wavelength }) }) },
  { id: 'distortion', label: 'Distortion', help: 'Chief-ray image position against the paraxial chief-ray intercept at the stored image plane.', field: true, wavelength: true, sampling: 'chief ray',
    run: async (p) => ({ kind: 'distortion', data: await callTool('distortion', { field_index: p.fieldIndex, wavelength: p.wavelength }) }) },
  { id: 'wavefront', label: 'Wavefront map', help: 'OPD map over the pupil; failed samples are hatched.', field: true, wavelength: true, sampling: `${WAVEFRONT_GRID}×${WAVEFRONT_GRID} pupil grid`,
    run: async (p) => ({ kind: 'wavefront', data: await callTool('wavefront', { field_index: p.fieldIndex, wavelength: p.wavelength, grid: WAVEFRONT_GRID }) }) },
  { id: 'diffraction_psf', label: 'Diffraction PSF', help: 'Fraunhofer point spread function from the wavefront.', field: true, wavelength: true, sampling: `${DIFFRACTION_GRID}×${DIFFRACTION_GRID} pupil grid`,
    run: async (p) => ({ kind: 'diffraction_psf', data: await callTool('diffraction_psf', { field_index: p.fieldIndex, wavelength: p.wavelength, grid: DIFFRACTION_GRID }) }) },
  { id: 'diffraction_mtf', label: 'Diffraction MTF', help: 'Modulation transfer function from the PSF. Weighted mode sums complex OTFs of every positive-weight wavelength on common physical frequencies; max frequency is chosen automatically from the sampling limit.', field: true, wavelength: false, spectral: true, sampling: `${DIFFRACTION_GRID}×${DIFFRACTION_GRID} pupil grid`,
    run: async (p) => ({ kind: 'diffraction_mtf', data: await callTool('diffraction_mtf', { field_index: p.fieldIndex, spectrum: p.spectrum, grid: DIFFRACTION_GRID }) }) },
  { id: 'encircled_energy', label: 'Encircled energy', help: 'Area-weighted fraction of transmitted pupil samples within a radius of the weighted centroid.', field: true, wavelength: true, rays: true, sampling: `${CURVE_SAMPLES} radii`,
    run: async (p) => ({ kind: 'encircled_energy', data: await callTool('encircled_energy', { field_index: p.fieldIndex, wavelength: p.wavelength, sampling: p.sampling, samples: CURVE_SAMPLES }) }) },
];

const kind = ref<AnalysisKind>('ray_fan');
const fieldIndex = ref(0);
const wavelengthIndex = ref(props.system.reference_wavelength);
const spectralMode = ref<SpectralMode>('mono');
/** Kept across revisions and analysis kinds; changes only on user action. */
const energySampling = ref<PupilSampling>({ ...GEOMETRIC_SAMPLING });
/** Field slice: 'default' omits the endpoint; 'x+', 'y-' etc. are maxima on an axis; 'f<n>' is stored field n. */
const slice = ref('default');
/** Applied sweep sample count (drives requests) and the raw text being edited; only Apply moves text into the count. */
const curvatureSamples = ref(33);
const samplesText = ref(String(curvatureSamples.value));
const parsedSamples = computed(() => {
  const t = samplesText.value.trim();
  const n = t === '' ? NaN : Number(t);
  return Number.isInteger(n) && n >= CURVATURE_MIN && n <= CURVATURE_MAX ? n : null;
});
const samplesValid = computed(() => parsedSamples.value !== null);
const samplesUnapplied = computed(() => samplesValid.value && parsedSamples.value !== curvatureSamples.value);
/** Commits a valid whole number; the existing key watcher then runs the analysis once. Invalid text sends nothing. */
function applySamples(): void {
  const n = parsedSamples.value;
  if (n === null) return;
  curvatureSamples.value = n;
  samplesText.value = String(n);
}
const outcome = ref<AnalysisOutcome | null>(null);
const error = ref('');
const loading = ref(false);
const ranField = ref('');
const ranRevision = ref(props.revision);
const ranColor = ref<RGBA>(FALLBACK_COLOR);
let token = 0;

const def = computed(() => DEFS.find((d) => d.id === kind.value)!);
const showWavelength = computed(() => def.value.wavelength || (def.value.spectral && spectralMode.value === 'mono'));
const samplingText = computed(() =>
  def.value.rays ? `${describeSampling(energySampling.value)}, ${def.value.sampling}`
  : kind.value === FIELD_CURVATURE_ID ? `${curvatureSamples.value} samples, ${sliceText(props.system)}, ${def.value.sampling}`
  : def.value.sampling);

// Keep the chosen wavelength by value when a revision reorders or edits the wavelength list.
let selectedValue = props.system.wavelengths[wavelengthIndex.value]?.value;
watch(wavelengthIndex, (i) => { selectedValue = props.system.wavelengths[i]?.value; });
watch(() => props.system, (sys) => {
  // Keep the current entry when it still holds the selected value, so duplicate values keep their own index and color.
  if (sys.wavelengths[wavelengthIndex.value]?.value === selectedValue) return;
  const kept = sys.wavelengths.findIndex((w) => w.value === selectedValue);
  wavelengthIndex.value = kept >= 0 ? kept : sys.reference_wavelength;
});
const isCurvature = computed(() => kind.value === FIELD_CURVATURE_ID);
const maxField = (sys: OpticalSystem): number => Math.max(...sys.fields.map((f) => Math.hypot(f.x, f.y)));
/** Endpoint for the request; undefined omits it so the backend uses +Y at the largest field. */
function sliceEnd(sys: OpticalSystem): [number, number] | undefined {
  const m = maxField(sys);
  switch (slice.value) {
    case 'x+': return [m, 0];
    case 'x-': return [-m, 0];
    case 'y-': return [0, -m];
    case 'default': return undefined;
    default: {
      const f = sys.fields[Number(slice.value.slice(1))];
      return f ? [f.x, f.y] : undefined;
    }
  }
}
const sliceText = (sys: OpticalSystem): string => {
  if (slice.value.startsWith('f')) return `stored field ${fieldLabel(sys, Number(slice.value.slice(1)))}`;
  return ({ default: '+Y maximum', 'x+': '+X maximum', 'x-': '-X maximum', 'y-': '-Y maximum' } as Record<string, string>)[slice.value] ?? '+Y maximum';
};
// A stored-field slice must keep addressing an existing field after the prescription changes.
watch(() => props.system, (sys) => {
  if (slice.value.startsWith('f') && Number(slice.value.slice(1)) >= sys.fields.length) slice.value = 'default';
});
const curvatureKey = computed(() => (isCurvature.value ? `${slice.value}|${curvatureSamples.value}` : ''));
const range = computed(() => {
  const v = props.system.wavelengths.map((w) => w.value);
  return { lower: Math.min(...v), upper: Math.max(...v) };
});

async function run(): Promise<void> {
  if (!props.online) return;
  const mine = ++token;
  const sys = props.system;
  if (fieldIndex.value >= sys.fields.length) fieldIndex.value = 0;
  if (wavelengthIndex.value >= sys.wavelengths.length) wavelengthIndex.value = sys.reference_wavelength;
  const d = def.value;
  if (d.range && !(range.value.upper > range.value.lower)) {
    outcome.value = null;
    error.value = 'Chromatic focus needs at least two distinct wavelengths in the saved system.';
    return;
  }
  // Everything the response is labeled with is captured here, before dispatch.
  const revisionAtRequest = props.revision;
  const field = fieldIndex.value;
  const fieldText = fieldLabel(sys, field);
  const wavelength = sys.wavelengths[wavelengthIndex.value]!.value;
  const color = sys.wavelengths[wavelengthIndex.value]!.color;
  loading.value = true;
  try {
    const result = await d.run({
      fieldIndex: field,
      wavelength,
      lower: range.value.lower,
      upper: range.value.upper,
      sampling: { ...energySampling.value },
      curvatureSamples: curvatureSamples.value,
      fieldEnd: sliceEnd(sys),
      spectrum: spectralMode.value === 'poly'
        ? { kind: 'polychromatic' }
        : { kind: 'monochromatic', wavelength },
    });
    if (mine !== token) return;
    const check = checkRevision(revisionAtRequest, props.revision, result.data.revision);
    if (check === 'superseded') return;
    if (check === 'newer') {
      outcome.value = null;
      error.value = savedSystemChanged(d.label, revisionAtRequest, result.data.revision);
      return;
    }
    outcome.value = result;
    ranRevision.value = revisionAtRequest;
    ranField.value = fieldText;
    ranColor.value = color;
    error.value = '';
  } catch (e) {
    if (mine !== token) return;
    outcome.value = null;
    error.value = errorMessage(e);
  } finally {
    if (mine === token) loading.value = false;
  }
}

// Any input change drops the previous result and in-flight request immediately so a mismatched view is never shown.
// The wavelength only matters when it is part of the request.
watch(
  [kind, fieldIndex, () => (showWavelength.value ? wavelengthIndex.value : -1), spectralMode, energySampling, curvatureKey, () => props.revision, () => props.online],
  () => { token++; loading.value = false; outcome.value = null; error.value = ''; void run(); },
  { immediate: true },
);
</script>

<template>
  <section class="panel" aria-labelledby="an-title">
    <div class="panel-head">
      <h3 id="an-title">Analysis</h3>
      <span>Computed on the saved system · revision {{ outcome ? ranRevision : revision }}</span>
    </div>
    <div v-if="dirty" role="status" class="banner warn inset">
      Results use the saved system. Unapplied draft changes are not included until you apply them.
    </div>
    <div class="analysis-controls">
      <div class="controls">
        <label>Analysis
          <select v-model="kind" class="wide-select" aria-describedby="an-help">
            <option v-for="d in DEFS" :key="d.id" :value="d.id">{{ d.label }}</option>
          </select>
        </label>
        <label v-if="def.field">Field
          <select v-model.number="fieldIndex">
            <option v-for="(_, i) in system.fields" :key="i" :value="i">{{ fieldLabel(system, i) }}</option>
          </select>
        </label>
        <label v-if="def.spectral">Spectral mode
          <select v-model="spectralMode">
            <option value="mono">Selected wavelength</option>
            <option value="poly">Weighted wavelengths</option>
          </select>
        </label>
        <label v-if="showWavelength">Wavelength
          <select v-model.number="wavelengthIndex">
            <option v-for="(w, i) in system.wavelengths" :key="i" :value="i">{{ i + 1 }}: {{ w.value }} nm</option>
          </select>
        </label>
        <label v-if="isCurvature">Field slice
          <select v-model="slice" class="wide-select">
            <option value="default">+Y maximum</option>
            <option value="x+">+X maximum</option>
            <option value="y-">-Y maximum</option>
            <option value="x-">-X maximum</option>
            <option v-for="(_, i) in system.fields" :key="i" :value="`f${i}`">Stored field {{ fieldLabel(system, i) }}</option>
          </select>
        </label>
        <form v-if="isCurvature" class="controls" @submit.prevent="applySamples">
          <label>Field samples
            <input type="number" min="2" max="257" step="1" required :value="samplesText" @input="samplesText = ($event.target as HTMLInputElement).value" />
          </label>
          <button type="submit" :disabled="!samplesValid || !samplesUnapplied">Apply samples</button>
        </form>
        <RaySamplingControl v-if="def.rays" v-model="energySampling" />
        <span v-if="def.range">Wavelengths {{ range.lower }}–{{ range.upper }} nm (saved range)</span>
        <button type="button" :disabled="!online || loading || (isCurvature && (!samplesValid || samplesUnapplied))" @click="run()">Recompute</button>
      </div>
      <p v-if="isCurvature && !samplesValid" class="note warn-text" role="status">Field samples must be a whole number from {{ CURVATURE_MIN }} to {{ CURVATURE_MAX }}. Nothing is sent until it is valid and applied.</p>
      <p v-else-if="isCurvature && samplesUnapplied" class="note" role="status">Showing {{ curvatureSamples }} samples; press Apply samples (or Enter) to use {{ parsedSamples }}.</p>
      <p id="an-help" class="note">{{ def.help }} Sampling: {{ samplingText }}.</p>
    </div>
    <div v-if="error" role="alert" class="banner error inset">{{ error }}</div>
    <p v-if="loading && !outcome" class="empty" role="status">Computing…</p>
    <p v-else-if="!outcome && !error" class="empty">{{ online ? 'No result yet.' : 'Engine offline.' }}</p>
    <div v-if="outcome" class="analysis-body" :aria-busy="loading">
      <AnalysisResult :outcome="outcome" :field-text="def.field ? ranField : undefined" :color="ranColor" />
    </div>
  </section>
</template>
