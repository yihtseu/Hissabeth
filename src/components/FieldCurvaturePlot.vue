<script setup lang="ts">
import { computed, ref, useId } from 'vue';
import { fmt, niceTicks } from '../chart';
import { rgbCss } from '../color';
import type { FieldCurvature, FieldCurvatureCurve, FocusValue } from '../types';

const props = defineProps<{ data: FieldCurvature }>();

const uid = useId();
const L = 78, R = 870, T = 18, B = 300;

/** Symmetric horizontal half range in mm; 'auto' fits the finite values and always includes 0. */
const HALF_RANGES = ['auto', '0.05', '0.1', '0.25', '0.5', '1', '2.5', '5', '10', '25'];
const half = ref('auto');

type Branch = 'sagittal' | 'tangential';
const BRANCHES: { id: Branch; label: string; dash?: string }[] = [
  { id: 'sagittal', label: 'S' },
  { id: 'tangential', label: 'T', dash: '7 4' },
];

const shiftOf = (v: FocusValue): number | null => (v.shift !== null && Number.isFinite(v.shift) ? v.shift : null);

const finiteShifts = computed(() =>
  props.data.curves.flatMap((c) => c.points.flatMap((p) => [shiftOf(p.sagittal), shiftOf(p.tangential)])).filter((v): v is number => v !== null));
const totalBranchSamples = computed(() => props.data.curves.reduce((n, c) => n + 2 * c.points.length, 0));

const xr = computed<[number, number]>(() => {
  if (half.value !== 'auto') {
    const h = Number(half.value);
    return [-h, h];
  }
  let lo = Math.min(0, ...finiteShifts.value), hi = Math.max(0, ...finiteShifts.value);
  if (lo === hi) { lo = -1; hi = 1; }
  const pad = (hi - lo) * 0.05;
  return [lo - pad, hi + pad];
});
const yMax = computed(() => {
  const radii = props.data.curves.flatMap((c) => c.points.map((p) => p.radius));
  const m = Math.max(props.data.max_field, ...radii, 0);
  return m > 0 ? m : 1;
});
const xTicks = computed(() => niceTicks(xr.value[0], xr.value[1], 6));
const yTicks = computed(() => niceTicks(0, yMax.value, 5).filter((t) => t >= 0 && t <= yMax.value * (1 + 1e-9)));
const px = (x: number): number => L + ((x - xr.value[0]) / (xr.value[1] - xr.value[0])) * (R - L);
const py = (r: number): number => B - (r / yMax.value) * (B - T);

interface Run { points: { x: number; y: number }[]; openStart: boolean; openEnd: boolean }
/** Separate runs of valid samples; a failed sample breaks the line and the neighboring ends get open markers. */
function runsOf(curve: FieldCurvatureCurve, branch: Branch): Run[] {
  const out: Run[] = [];
  let cur: Run | null = null;
  for (let i = 0; i < curve.points.length; i++) {
    const p = curve.points[i]!;
    const v = shiftOf(p[branch]);
    if (v === null) {
      if (cur) { cur.openEnd = true; out.push(cur); cur = null; }
      continue;
    }
    if (!cur) cur = { points: [], openStart: i > 0, openEnd: false };
    cur.points.push({ x: v, y: p.radius });
  }
  if (cur) out.push(cur);
  return out;
}
const lines = computed(() =>
  props.data.curves.flatMap((c) =>
    BRANCHES.map((b) => ({
      key: `${c.wavelength_index}-${b.id}`,
      color: rgbCss(c.color),
      dash: b.dash,
      muted: c.weight === 0,
      runs: runsOf(c, b.id).map((r) => ({ ...r, path: r.points.map((q) => `${px(q.x)},${py(q.y)}`).join(' ') })),
    })),
  ),
);

const validCount = computed(() => finiteShifts.value.length);
/** Valid values beyond the shown horizontal range: clipped from the plot, but not failures. */
const outsideCount = computed(() => finiteShifts.value.filter((v) => v < xr.value[0] || v > xr.value[1]).length);
const summary = computed(() =>
  `Field curvature: sagittal solid and tangential dashed focus shift against field radius, ${props.data.curves.length} wavelength curves, ${validCount.value} of ${totalBranchSamples.value} focus values valid`);
const legendLabel = (c: FieldCurvatureCurve): string =>
  `${c.wavelength_index + 1}: ${c.wavelength} nm${c.weight === 0 ? ' (weight 0)' : ''}`;
</script>

<template>
  <figure class="chart">
    <figcaption>Field curvature</figcaption>
    <div class="controls">
      <label>Horizontal range
        <select v-model="half">
          <option v-for="h in HALF_RANGES" :key="h" :value="h">{{ h === 'auto' ? 'Automatic (includes 0)' : `±${h} mm` }}</option>
        </select>
      </label>
    </div>
    <div v-if="validCount === 0" role="status" class="banner warn">
      No focus value could be computed for this sweep. The axes are kept for reference; the failure table lists each reason.
    </div>
    <svg viewBox="0 0 900 360" role="img" :aria-labelledby="`${uid}-t`">
      <title :id="`${uid}-t`">{{ summary }}</title>
      <defs><clipPath :id="`${uid}-clip`"><rect :x="L" :y="T" :width="R - L" :height="B - T" /></clipPath></defs>
      <g stroke="#dfe7ed">
        <line v-for="t in yTicks" :key="`gy${t}`" :x1="L" :x2="R" :y1="py(t)" :y2="py(t)" />
        <line v-for="t in xTicks" :key="`gx${t}`" :x1="px(t)" :x2="px(t)" :y1="T" :y2="B" />
      </g>
      <line :x1="L" :x2="R" :y1="B" :y2="B" stroke="#a7bac7" /><line :x1="L" :x2="L" :y1="T" :y2="B" stroke="#a7bac7" />
      <line v-if="xr[0] <= 0 && xr[1] >= 0" :x1="px(0)" :x2="px(0)" :y1="T" :y2="B" stroke="#5d7584" stroke-width="1.6" />
      <g font-size="11" fill="#5d7584">
        <text v-for="t in yTicks" :key="`ty${t}`" :x="L - 8" :y="py(t) + 4" text-anchor="end">{{ fmt(t) }}</text>
        <text v-for="t in xTicks" :key="`tx${t}`" :x="px(t)" :y="B + 16" text-anchor="middle">{{ fmt(t) }}</text>
        <text :x="(L + R) / 2" :y="B + 38" text-anchor="middle">Focus shift from the image vertex (mm, global z)</text>
        <text x="14" :y="(T + B) / 2" text-anchor="middle" :transform="`rotate(-90 14 ${(T + B) / 2})`">Field radius ({{ data.field_units }})</text>
      </g>
      <g :clip-path="`url(#${uid}-clip)`">
      <template v-for="l in lines" :key="l.key">
        <template v-for="(r, k) in l.runs" :key="k">
          <polyline v-if="r.points.length > 1" :points="r.path" fill="none" :stroke="l.color" stroke-width="2.2" :stroke-dasharray="l.dash" :stroke-opacity="l.muted ? 0.6 : 1" />
          <circle v-if="r.points.length === 1" :cx="px(r.points[0]!.x)" :cy="py(r.points[0]!.y)" r="3" :fill="l.color" />
          <circle v-if="r.openStart && r.points.length > 1" :cx="px(r.points[0]!.x)" :cy="py(r.points[0]!.y)" r="3" fill="#fff" :stroke="l.color" stroke-width="1.6" />
          <circle v-if="r.openEnd && r.points.length > 1" :cx="px(r.points[r.points.length - 1]!.x)" :cy="py(r.points[r.points.length - 1]!.y)" r="3" fill="#fff" :stroke="l.color" stroke-width="1.6" />
        </template>
      </template>
      </g>
    </svg>
    <p v-if="outsideCount > 0" class="note" role="status">{{ outsideCount }} valid focus value{{ outsideCount === 1 ? '' : 's' }} lie outside the selected horizontal range and are clipped from the plot. They are not failures; widen the range to see them.</p>
    <ul class="legend" aria-label="Field curvature legend">
      <li v-for="c in data.curves" :key="c.wavelength_index" :style="{ color: rgbCss(c.color) }">
        <span aria-hidden="true">━━ S · ╌╌ T</span> {{ legendLabel(c) }}
      </li>
      <li>Solid: sagittal · Dashed: tangential · Open circle: line broken at a failed sample</li>
    </ul>
  </figure>
</template>
