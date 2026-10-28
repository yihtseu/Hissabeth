<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { extent, fmt } from '../chart';

const props = withDefaults(
  defineProps<{
    title: string;
    /** Row-major n×n samples; row 0 is drawn at the bottom (+y up). null = no value. */
    values: (number | null)[];
    size: number;
    /** Cells outside the pupil by construction (null there is expected, not a failure). */
    outside?: boolean[];
    diverging?: boolean;
    log?: boolean;
    unit?: string;
  }>(),
  { outside: () => [], diverging: false, log: false, unit: '' },
);

const canvas = ref<HTMLCanvasElement | null>(null);
const cell = computed(() => Math.max(3, Math.floor(480 / props.size)));

const transformed = computed(() =>
  props.values.map((v) => (v === null ? null : props.log ? Math.log10(Math.max(v, 1e-12)) : v)),
);
const range = computed<[number, number]>(() => {
  const e = extent(transformed.value) ?? [0, 1];
  if (props.diverging) { const m = Math.max(Math.abs(e[0]), Math.abs(e[1]), 1e-12); return [-m, m]; }
  return e[0] === e[1] ? [e[0], e[0] + 1] : e;
});
const failed = computed(() => props.values.filter((v, i) => v === null && !props.outside[i]).length);
const shown = computed(() => props.values.filter((v) => v !== null).length);

function lerp(a: number[], b: number[], t: number): number[] {
  return a.map((x, i) => x + (b[i]! - x) * t);
}
const SEQ = [[16, 24, 60], [40, 110, 160], [70, 170, 150], [240, 220, 110], [255, 252, 235]];
function color(t: number): string {
  t = Math.min(1, Math.max(0, t));
  let c: number[];
  if (props.diverging) {
    c = t < 0.5 ? lerp([50, 100, 190], [250, 250, 250], t * 2) : lerp([250, 250, 250], [200, 70, 50], (t - 0.5) * 2);
  } else {
    const k = t * (SEQ.length - 1);
    const i = Math.min(SEQ.length - 2, Math.floor(k));
    c = lerp(SEQ[i]!, SEQ[i + 1]!, k - i);
  }
  return `rgb(${c.map(Math.round).join(',')})`;
}
const ramp = computed(() => `linear-gradient(to right, ${[0, 0.25, 0.5, 0.75, 1].map(color).join(',')})`);

function draw(): void {
  const el = canvas.value;
  const ctx = el?.getContext('2d');
  if (!el || !ctx) return;
  const n = props.size, c = cell.value;
  el.width = n * c;
  el.height = n * c;
  const [lo, hi] = range.value;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < n; j++) {
      const idx = i * n + j;
      const v = transformed.value[idx];
      const x = j * c, y = (n - 1 - i) * c;
      if (v === null || v === undefined) {
        const out = props.outside[idx];
        ctx.fillStyle = out ? '#eef2f5' : '#fbe3dc';
        ctx.fillRect(x, y, c, c);
        if (!out) {
          // Hatch marks a sample that should have been valid but failed to trace.
          ctx.strokeStyle = '#b5532f';
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(x, y + c);
          ctx.lineTo(x + c, y);
          ctx.stroke();
        }
      } else {
        ctx.fillStyle = color((v - lo) / (hi - lo));
        ctx.fillRect(x, y, c, c);
      }
    }
  }
}
onMounted(draw);
watch([() => props.values, () => props.size, () => props.outside, () => props.diverging, () => props.log], draw);
</script>

<template>
  <figure class="chart heat">
    <figcaption>{{ title }}</figcaption>
    <canvas ref="canvas" role="img" :aria-label="`${title}: ${size} by ${size} samples, ${shown} valid, ${failed} failed`" />
    <div class="ramp" :style="{ background: ramp }" aria-hidden="true"></div>
    <div class="ramp-labels"><span>{{ log ? '10^' : '' }}{{ fmt(range[0]) }}</span><span>{{ unit }}</span><span>{{ log ? '10^' : '' }}{{ fmt(range[1]) }}</span></div>
    <ul class="legend" :aria-label="`${title} sample mask`">
      <li>{{ shown }} valid samples</li>
      <li :class="{ 'reflect-key': failed > 0 }">▨ {{ failed }} invalid (failed) samples</li>
      <li>▢ outside pupil / unused</li>
    </ul>
  </figure>
</template>
