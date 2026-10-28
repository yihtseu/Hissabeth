<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, toRaw, watch } from 'vue';
import { wavelengthColor } from '../chart';

const props = defineProps<{
  points: [number, number][];
  centroid: [number, number];
  /** Per-point wavelength (nm) for coloring; when absent every point uses `color`. */
  wavelengths?: number[];
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
const colorCache = new Map<number, string>();
const colorOf = (nm: number): string => {
  let c = colorCache.get(nm);
  if (c === undefined) { c = wavelengthColor(nm); colorCache.set(nm, c); }
  return c;
};

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
  const wl = props.wavelengths ? toRaw(props.wavelengths) : undefined;
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
    const fill = wl ? colorOf(wl[i]!) : (props.color ?? '#327c9f');
    if (fill !== current) { ctx.fillStyle = fill; current = fill; }
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
  [() => props.points, () => props.centroid, () => props.wavelengths, () => props.color, () => props.opacity, cssWidth],
  schedule,
);
</script>

<template>
  <canvas ref="canvas" class="spot-canvas" role="img" :aria-label="label">{{ label }}</canvas>
</template>
