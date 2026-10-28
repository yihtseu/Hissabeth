<script setup lang="ts">
import { computed, ref } from 'vue';
import { fmt, niceTicks, type ChartSeries } from '../chart';
import { describeSampling, describeSpectrum, percent } from '../sampling';
import type { AnalysisOutcome } from '../types';
import HeatMap from './HeatMap.vue';
import LineChart from './LineChart.vue';
import SpectralLedger, { type LedgerRow } from './SpectralLedger.vue';

const props = defineProps<{ outcome: AnalysisOutcome; /** Field the result was computed for, e.g. "2: (0, 5)°". */ fieldText?: string }>();

const BLUE = '#286c8d', ORANGE = '#c76a3d';
const o = computed(() => props.outcome);

/** Field-local basis of the analyses that sample or project in the tangential/sagittal frame. */
const frameText = computed(() => {
  const r = o.value;
  if (!('frame' in r.data)) return '';
  const { tangential: t, sagittal: s } = r.data.frame;
  const v = (a: [number, number]): string => `(${fmt(a[0], 4)}, ${fmt(a[1], 4)})`;
  return `${props.fieldText ? `Field ${props.fieldText} · ` : ''}Local basis: t̂ = ${v(t)}, ŝ = ${v(s)} (global x, y)`;
});

interface Problem { where: string; at: string; error: string }

// ---- ray fan --------------------------------------------------------------
const fan = computed(() => {
  const r = o.value;
  if (r.kind !== 'ray_fan') return null;
  const build = (pts: typeof r.data.tangential): { series: ChartSeries[]; invalid: number[] } => ({
    series: [
      { label: 'Tangential error (t̂, µm)', color: BLUE, points: pts.map((p) => ({ x: p.pupil, y: p.tangential_error === null ? null : p.tangential_error * 1000 })) },
      { label: 'Sagittal error (ŝ, µm)', color: ORANGE, dashed: true, points: pts.map((p) => ({ x: p.pupil, y: p.sagittal_error === null ? null : p.sagittal_error * 1000 })) },
    ],
    invalid: pts.filter((p) => p.error !== null || p.tangential_error === null || p.sagittal_error === null).map((p) => p.pupil),
  });
  return { tangential: build(r.data.tangential), sagittal: build(r.data.sagittal) };
});

// ---- OPD fan --------------------------------------------------------------
const opd = computed(() => {
  const r = o.value;
  if (r.kind !== 'opd_fan') return null;
  const build = (pts: typeof r.data.tangential): { series: ChartSeries[]; invalid: number[] } => ({
    series: [{ label: 'OPD (waves)', color: BLUE, points: pts.map((p) => ({ x: p.pupil, y: p.opd })) }],
    invalid: pts.filter((p) => p.opd === null || p.error !== null).map((p) => p.pupil),
  });
  return { tangential: build(r.data.tangential), sagittal: build(r.data.sagittal) };
});

// ---- chromatic focus / longitudinal ----------------------------------------
const chromatic = computed(() => {
  const r = o.value;
  if (r.kind !== 'chromatic_focus') return null;
  return {
    series: [{ label: 'Focus shift (mm)', color: BLUE, points: r.data.points.map((p) => ({ x: p.wavelength, y: p.shift })) }] as ChartSeries[],
    invalid: r.data.points.filter((p) => p.shift === null).map((p) => p.wavelength),
  };
});
const longitudinal = computed(() => {
  const r = o.value;
  if (r.kind !== 'longitudinal_aberration') return null;
  return {
    series: [{ label: 'Focus shift (mm)', color: BLUE, points: r.data.points.map((p) => ({ x: p.pupil, y: p.shift })) }] as ChartSeries[],
    invalid: r.data.points.filter((p) => p.shift === null).map((p) => p.pupil),
  };
});

// ---- astigmatism ------------------------------------------------------------
const astig = computed(() => {
  const r = o.value;
  if (r.kind !== 'astigmatism') return null;
  const s = r.data.sagittal_shift, t = r.data.tangential_shift;
  const m = Math.max(Math.abs(s), Math.abs(t), 1e-9) * 1.3;
  const ticks = niceTicks(-m, m, 6);
  const x = (v: number): number => 450 + (v / m) * 380;
  return { s, t, ticks, x };
});

// ---- distortion ---------------------------------------------------------------
const dist = computed(() => {
  const r = o.value;
  if (r.kind !== 'distortion') return null;
  const dx = (r.data.actual[0] - r.data.ideal[0]) * 1000, dy = (r.data.actual[1] - r.data.ideal[1]) * 1000;
  const m = Math.max(Math.abs(dx), Math.abs(dy), 1e-9) * 1.3;
  return { dx, dy, m, ticks: niceTicks(-m, m, 4) };
});

// ---- wavefront / PSF ---------------------------------------------------------------
const wave = computed(() => {
  const r = o.value;
  if (r.kind !== 'wavefront') return null;
  return { outside: r.data.pupils.map(([x, y]) => x * x + y * y > 1) };
});
const logPsf = ref(true);
const psfHalf = computed(() => {
  const r = o.value;
  return r.kind === 'diffraction_psf' ? (r.data.grid * r.data.pixel_size * 1000) / 2 : 0;
});

// ---- other curves -------------------------------------------------------------------
const diffMtf = computed(() => {
  const r = o.value;
  if (r.kind !== 'diffraction_mtf') return null;
  const f = r.data.frequencies;
  return [
    { label: 'Tangential (t̂)', color: BLUE, points: f.map((x, i) => ({ x, y: r.data.tangential[i] ?? null })) },
    { label: 'Sagittal (ŝ)', color: ORANGE, dashed: true, points: f.map((x, i) => ({ x, y: r.data.sagittal[i] ?? null })) },
  ] as ChartSeries[];
});
const diffRows = computed<LedgerRow[]>(() => {
  const r = o.value;
  if (r.kind !== 'diffraction_mtf') return [];
  return r.data.components.map((c) => ({
    wavelength: c.wavelength,
    weight: c.weight,
    transmitted: c.transmitted_samples,
    sampled: c.pupil_samples,
    fraction: c.transmitted_fraction,
    effective: c.effective_weight,
    pixelSize: c.pixel_size,
    cutoff: c.sampling_cutoff,
  }));
});
const energy = computed(() => {
  const r = o.value;
  if (r.kind !== 'encircled_energy') return null;
  return [{ label: 'Enclosed fraction', color: BLUE, points: r.data.points.map((p) => ({ x: p.radius * 1000, y: p.fraction })) }] as ChartSeries[];
});

// ---- explicit error list for masked samples ---------------------------------------------
const problems = computed<Problem[]>(() => {
  const r = o.value;
  if (r.kind === 'ray_fan')
    return (['tangential', 'sagittal'] as const).flatMap((axis) =>
      r.data[axis].filter((p) => p.error).map((p) => ({ where: axis, at: `${axis === 'tangential' ? 'p_T' : 'p_S'} ${fmt(p.pupil)}`, error: p.error! })));
  if (r.kind === 'opd_fan')
    return (['tangential', 'sagittal'] as const).flatMap((axis) =>
      r.data[axis].filter((p) => p.error).map((p) => ({ where: axis, at: `${axis === 'tangential' ? 'p_T' : 'p_S'} ${fmt(p.pupil)}`, error: p.error! })));
  if (r.kind === 'chromatic_focus')
    return r.data.points.filter((p) => p.error).map((p) => ({ where: 'wavelength', at: `${fmt(p.wavelength)} nm`, error: p.error! }));
  if (r.kind === 'longitudinal_aberration')
    return r.data.points.filter((p) => p.error).map((p) => ({ where: 'pupil', at: fmt(p.pupil), error: p.error! }));
  return [];
});

/** Raw JSON supplement; long numeric arrays are summarized to keep the page responsive. */
const showRaw = ref(false);
const raw = computed(() =>
  showRaw.value
    ? JSON.stringify(o.value.data, (_k, v: unknown) =>
        Array.isArray(v) && v.length > 256 ? `[${v.length} entries]` : v, 1)
    : '',
);
</script>

<template>
  <div class="result">
    <!-- ray fan -->
    <template v-if="fan && o.kind === 'ray_fan'">
      <p class="note">{{ frameText }}</p>
      <LineChart title="Tangential ray fan" :series="fan.tangential.series" x-label="Tangential pupil p_T" y-label="Local image-plane error (µm)" include-zero :invalid-x="fan.tangential.invalid" />
      <LineChart title="Sagittal ray fan" :series="fan.sagittal.series" x-label="Sagittal pupil p_S" y-label="Local image-plane error (µm)" include-zero :invalid-x="fan.sagittal.invalid" />
      <p class="note">Errors are projected on the tangential (t̂) and sagittal (ŝ) axes. Relative to the chief-ray image intercept ({{ fmt(o.data.chief_image[0]) }}, {{ fmt(o.data.chief_image[1]) }}) mm at {{ o.data.wavelength }} nm.</p>
    </template>

    <!-- OPD fan -->
    <template v-else-if="opd && o.kind === 'opd_fan'">
      <p class="note">{{ frameText }}</p>
      <LineChart title="Tangential OPD fan" :series="opd.tangential.series" x-label="Tangential pupil p_T" y-label="OPD (waves)" include-zero :invalid-x="opd.tangential.invalid" />
      <LineChart title="Sagittal OPD fan" :series="opd.sagittal.series" x-label="Sagittal pupil p_S" y-label="OPD (waves)" include-zero :invalid-x="opd.sagittal.invalid" />
      <p class="note">Reference: {{ o.data.reference }}. Wavelength {{ o.data.wavelength }} nm.</p>
    </template>

    <!-- chromatic focus -->
    <template v-else-if="chromatic && o.kind === 'chromatic_focus'">
      <LineChart title="Chromatic focal shift" :series="chromatic.series" x-label="Wavelength (nm)" y-label="Focus shift from reference (mm)" include-zero :invalid-x="chromatic.invalid" />
      <dl class="kv"><div><dt>Reference wavelength</dt><dd>{{ o.data.reference_wavelength }} nm</dd></div><div><dt>Reference focus</dt><dd>{{ fmt(o.data.reference_focus, 6) }} mm</dd></div></dl>
    </template>

    <!-- longitudinal aberration -->
    <template v-else-if="longitudinal && o.kind === 'longitudinal_aberration'">
      <LineChart title="Longitudinal spherical aberration" :series="longitudinal.series" x-label="Normalized pupil height" y-label="Axial focus shift (mm)" include-zero :invalid-x="longitudinal.invalid" />
      <dl class="kv"><div><dt>Paraxial focus</dt><dd>{{ fmt(o.data.paraxial_focus, 6) }} mm</dd></div><div><dt>Wavelength</dt><dd>{{ o.data.wavelength }} nm</dd></div></dl>
    </template>

    <!-- astigmatism -->
    <template v-else-if="astig && o.kind === 'astigmatism'">
      <p class="note">{{ frameText }}</p>
      <figure class="chart">
        <figcaption>Sagittal and tangential focus relative to the image plane</figcaption>
        <svg viewBox="0 0 900 150" role="img" aria-label="Focus shift diagram">
          <title>Sagittal focus shift {{ fmt(astig.s) }} mm, tangential focus shift {{ fmt(astig.t) }} mm</title>
          <line x1="70" x2="830" y1="90" y2="90" stroke="#a7bac7" />
          <g font-size="11" fill="#5d7584">
            <template v-for="t in astig.ticks" :key="t"><line :x1="astig.x(t)" :x2="astig.x(t)" y1="85" y2="95" stroke="#a7bac7" /><text :x="astig.x(t)" y="112" text-anchor="middle">{{ fmt(t) }}</text></template>
            <text x="450" y="140" text-anchor="middle">Focus shift along the axis (mm); 0 = image plane</text>
          </g>
          <line :x1="astig.x(0)" :x2="astig.x(0)" y1="20" y2="90" stroke="#75989c" stroke-width="2" />
          <circle :cx="astig.x(astig.s)" cy="60" r="7" :fill="BLUE" /><text :x="astig.x(astig.s)" y="45" text-anchor="middle" font-size="12" :fill="BLUE">Sagittal (ŝ)</text>
          <circle :cx="astig.x(astig.t)" cy="75" r="7" :fill="ORANGE" /><text :x="astig.x(astig.t)" y="30" text-anchor="middle" font-size="12" :fill="ORANGE">Tangential (t̂)</text>
        </svg>
      </figure>
      <dl class="kv">
        <div><dt>Sagittal focus</dt><dd>{{ fmt(o.data.sagittal_focus, 6) }} mm</dd></div>
        <div><dt>Tangential focus</dt><dd>{{ fmt(o.data.tangential_focus, 6) }} mm</dd></div>
        <div><dt>Sagittal shift</dt><dd>{{ fmt(o.data.sagittal_shift) }} mm</dd></div>
        <div><dt>Tangential shift</dt><dd>{{ fmt(o.data.tangential_shift) }} mm</dd></div>
        <div><dt>Astigmatism (T − S)</dt><dd>{{ fmt(o.data.astigmatism) }} mm</dd></div>
      </dl>
    </template>

    <!-- distortion -->
    <template v-else-if="dist && o.kind === 'distortion'">
      <figure class="chart">
        <figcaption>Chief-ray displacement from the ideal image point</figcaption>
        <svg viewBox="0 0 360 300" role="img" aria-label="Distortion displacement">
          <title>Actual minus ideal image point: {{ fmt(dist.dx) }}, {{ fmt(dist.dy) }} µm</title>
          <line x1="180" x2="180" y1="20" y2="270" stroke="#ccd9df" /><line x1="30" x2="330" y1="145" y2="145" stroke="#ccd9df" />
          <circle cx="180" cy="145" r="5" fill="none" stroke="#5d7584" stroke-width="1.5" />
          <line x1="180" y1="145" :x2="180 + (dist.dx / dist.m) * 140" :y2="145 - (dist.dy / dist.m) * 120" :stroke="ORANGE" stroke-width="2" />
          <circle :cx="180 + (dist.dx / dist.m) * 140" :cy="145 - (dist.dy / dist.m) * 120" r="5" :fill="ORANGE" />
          <text x="334" y="140" font-size="11" fill="#5d7584" text-anchor="end">x ±{{ fmt(dist.m) }} µm</text>
          <text x="186" y="30" font-size="11" fill="#5d7584">y ±{{ fmt(dist.m) }} µm</text>
        </svg>
      </figure>
      <dl class="kv">
        <div><dt>Distortion</dt><dd>{{ o.data.distortion_percent === null ? 'undefined (ideal height 0)' : fmt(o.data.distortion_percent) + ' %' }}</dd></div>
        <div><dt>Ideal image point (reference)</dt><dd>{{ fmt(o.data.ideal[0], 5) }}, {{ fmt(o.data.ideal[1], 5) }} mm</dd></div>
        <div><dt>Actual image point</dt><dd>{{ fmt(o.data.actual[0], 5) }}, {{ fmt(o.data.actual[1], 5) }} mm</dd></div>
      </dl>
      <p class="note">Reference: {{ o.data.reference }}. Image-plane x, y are global.</p>
    </template>

    <!-- wavefront -->
    <template v-else-if="wave && o.kind === 'wavefront'">
      <HeatMap title="Wavefront OPD over the pupil" :values="o.data.opd" :size="o.data.grid" :outside="wave.outside" diverging unit="waves" />
      <dl class="kv">
        <div><dt>RMS</dt><dd>{{ fmt(o.data.rms) }} waves</dd></div>
        <div><dt>Peak to valley</dt><dd>{{ fmt(o.data.peak_to_valley) }} waves</dd></div>
        <div><dt>Mean</dt><dd>{{ fmt(o.data.mean) }} waves</dd></div>
        <div><dt>Valid / failed samples</dt><dd>{{ o.data.valid }} / {{ o.data.failed }}</dd></div>
      </dl>
      <p class="note">Reference: {{ o.data.reference }}. Rows are pupil y (up), columns pupil x.</p>
    </template>

    <!-- PSF -->
    <template v-else-if="o.kind === 'diffraction_psf'">
      <label class="inline"><input v-model="logPsf" type="checkbox" /> Logarithmic intensity scale</label>
      <HeatMap title="Diffraction PSF" :values="o.data.intensity" :size="o.data.grid" :log="logPsf" unit="normalized intensity" />
      <dl class="kv">
        <div><dt>Strehl ratio</dt><dd>{{ fmt(o.data.strehl, 4) }}</dd></div>
        <div><dt>Pixel size</dt><dd>{{ fmt(o.data.pixel_size * 1000) }} µm</dd></div>
        <div><dt>Extent</dt><dd>±{{ fmt(psfHalf) }} µm</dd></div>
        <div><dt>Failed pupil rays</dt><dd>{{ o.data.failed_pupil_rays }}</dd></div>
      </dl>
      <p class="note">{{ o.data.model }}; {{ o.data.normalization }}.</p>
    </template>

    <!-- diffraction MTF -->
    <template v-else-if="diffMtf && o.kind === 'diffraction_mtf'">
      <p class="note">{{ frameText }}</p>
      <LineChart title="Diffraction MTF" :series="diffMtf" x-label="Spatial frequency (cycles/mm)" y-label="Modulation" :y-min="0" :y-max="1" />
      <dl class="kv">
        <div><dt>Spectrum</dt><dd>{{ describeSpectrum(o.data.spectrum) }}</dd></div>
        <div><dt>Max frequency</dt><dd>{{ fmt(o.data.max_frequency, 4) }} cycles/mm</dd></div>
        <div><dt>Sampling limit</dt><dd>{{ fmt(o.data.sampling_limit, 4) }} cycles/mm</dd></div>
        <div><dt>FFT pupil grid</dt><dd>{{ o.data.pupil_grid }}×{{ o.data.pupil_grid }}</dd></div>
        <div><dt>Transmitted weight</dt><dd>{{ fmt(o.data.transmitted_weight, 4) }}</dd></div>
        <div><dt>Failed pupil rays</dt><dd>{{ o.data.failed_pupil_rays }}</dd></div>
      </dl>
      <SpectralLedger :rows="diffRows" caption="Spectral ledger (each wavelength has its own PSF pixel pitch and contributes zero above its own cutoff)" />
      <p class="note">{{ o.data.model }}. Cuts follow the field-local tangential and sagittal directions. OTFs are evaluated at common physical frequencies, placed at each real chief-ray intercept (origin {{ fmt(o.data.origin[0], 5) }}, {{ fmt(o.data.origin[1], 5) }} mm, global x, y) and combined as complex sums with effective spectral weights. {{ o.data.normalization }}.</p>
    </template>

    <!-- encircled energy -->
    <template v-else-if="energy && o.kind === 'encircled_energy'">
      <LineChart title="Encircled energy" :series="energy" x-label="Radius from centroid (µm)" y-label="Enclosed fraction" :y-min="0" :y-max="1" />
      <dl class="kv">
        <div><dt>Sampling</dt><dd>{{ describeSampling(o.data.sampling) }}</dd></div>
        <div><dt>Transmitted / sampled rays</dt><dd>{{ o.data.rays }} / {{ o.data.sampled_rays }}</dd></div>
        <div><dt>Transmitted area weight</dt><dd>{{ percent(o.data.transmitted_weight) }}</dd></div>
        <div><dt>Centre</dt><dd>{{ fmt(o.data.center[0], 5) }}, {{ fmt(o.data.center[1], 5) }} mm</dd></div>
      </dl>
      <p class="note">Fraction is the cumulative transmitted pupil-area weight, normalized to {{ o.data.normalization }}.</p>
    </template>

    <div v-if="problems.length" role="status" class="banner warn problem-list">
      <strong>{{ problems.length }} invalid sample{{ problems.length === 1 ? '' : 's' }}</strong>
      <table class="wide-table">
        <caption class="sr-only">Samples the engine rejected</caption>
        <thead><tr><th scope="col">Set</th><th scope="col">Sample</th><th scope="col">Error</th></tr></thead>
        <tbody><tr v-for="(p, i) in problems" :key="i"><td>{{ p.where }}</td><td>{{ p.at }}</td><td>{{ p.error }}</td></tr></tbody>
      </table>
    </div>

    <details class="data-table wide" @toggle="showRaw = ($event.target as HTMLDetailsElement).open">
      <summary>Raw response JSON</summary>
      <pre v-if="showRaw" class="raw">{{ raw }}</pre>
    </details>
  </div>
</template>
