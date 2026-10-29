<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, toRaw, watch } from 'vue';
import type { RGBA } from '../types';

const props = defineProps<{
  points: [number, number][];
  centroid: [number, number];
  /** Saved RGBA per point, aligned with `points`; when absent every point uses `color`. */
  colors?: RGBA[];
  color?: string;
  opacity: number;
  label: string;
}>();

// Logical drawing space matches the former 900×360 SVG viewBox.
const W = 900, H = 360, CX = 450, CY = 180, RADIUS = 1.8, FIT = 110;
/** Above this many points, dots are squares of the same size: far cheaper and indistinguishable at this scale. */
const CIRCLE_LIMIT = 20000;
const TAU = Math.PI * 2;

const canvas = ref<HTMLCanvasElement | null>(null);
const cssWidth = ref(W);

let frame = 0;
let observer: ResizeObserver | undefined;

function draw(): void {
  frame = 0;
  const el = canvas.value;
  const ctx = el?.getContext('2d');
  if (!el || !ctx) return;
  const dpr = window.devicePixelRatio || 1;
  const bw = Math.max(1, Math.round(cssWidth.value * dpr));
  const bh = Math.max(1, Math.round((bw * H) / W));
  if (el.width !== bw || el.height !== bh) { el.width = bw; el.height = bh; }
  ctx.setTransform(bw / W, 0, 0, bh / H, 0, 0);
  ctx.clearRect(0, 0, W, H);
  ctx.strokeStyle = '#ccd9df';
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(300, CY); ctx.lineTo(600, CY);
  ctx.moveTo(CX, 30); ctx.lineTo(CX, 330);
  ctx.stroke();

  // Raw arrays: reactive proxies would make every element access of a very large reply slow.
  const pts = toRaw(props.points);
  const cols = props.colors ? toRaw(props.colors) : undefined;
  // Fill styles keyed by packed RGB, rebuilt per draw so nothing outlives the frame.
  const styles = new Map<number, string>();
  let currentKey = -1;
  const [mx, my] = props.centroid;
  let maxR = 0.001; // streaming maximum: spreading hundreds of thousands of values exceeds argument limits
  for (let i = 0; i < pts.length; i++) {
    const p = pts[i]!;
    const r = Math.hypot(p[0] - mx, p[1] - my);
    if (r > maxR) maxR = r;
  }
  const k = FIT / maxR;
  const round = pts.length <= CIRCLE_LIMIT;
  ctx.globalAlpha = props.opacity;
  let current = '';
  for (let i = 0; i < pts.length; i++) {
    if (cols) {
      const c = cols[i]!;
      const r = Math.round(c[0] * 255), g = Math.round(c[1] * 255), b = Math.round(c[2] * 255);
      const key = (r << 16) | (g << 8) | b;
      if (key !== currentKey) {
        let fill = styles.get(key);
        if (fill === undefined) { fill = `rgb(${r},${g},${b})`; styles.set(key, fill); }
        ctx.fillStyle = fill;
        currentKey = key;
      }
    } else if (current === '') {
      ctx.fillStyle = props.color ?? '#327c9f';
      current = 'set';
    }
    const p = pts[i]!;
    const x = CX + (p[0] - mx) * k, y = CY - (p[1] - my) * k;
    if (round) { ctx.beginPath(); ctx.arc(x, y, RADIUS, 0, TAU); ctx.fill(); }
    else ctx.fillRect(x - RADIUS, y - RADIUS, 2 * RADIUS, 2 * RADIUS);
  }
  ctx.globalAlpha = 1;
}

function schedule(): void {
  if (frame) cancelAnimationFrame(frame);
  frame = requestAnimationFrame(draw);
}

onMounted(() => {
  const el = canvas.value;
  if (el) {
    cssWidth.value = el.clientWidth || W;
    if (typeof ResizeObserver !== 'undefined') {
      observer = new ResizeObserver(() => { cssWidth.value = el.clientWidth || W; });
      observer.observe(el);
    }
  }
  schedule();
});
onBeforeUnmount(() => {
  if (frame) cancelAnimationFrame(frame);
  frame = 0;
  observer?.disconnect();
});
watch(
  [() => props.points, () => props.centroid, () => props.colors, () => props.color, () => props.opacity, cssWidth],
  schedule,
);
</script>

<template>
  <canvas ref="canvas" class="spot-canvas" role="img" :aria-label="label">{{ label }}</canvas>
</template>
