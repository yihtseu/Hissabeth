<script setup lang="ts">
import { computed } from 'vue';
import type { Layout, OpticalSystem, Ray, Surface } from '../types';
import { fieldLabel } from '../validation';

const props = defineProps<{ system: OpticalSystem; layout: Layout; wavelength: number }>();

const COLORS = ['#c76a3d', '#327c9f', '#8d70b3', '#4c9d88', '#b08a2e', '#a94f78'];
const colorFor = (i: number): string => COLORS[i % COLORS.length] ?? COLORS[0]!;

const W = 900, H = 460, MX = 45, CY = H / 2, HALF = 190;

/** Axial vertex position of each surface; image plane follows the last thickness. */
const vertices = computed(() => {
  let z = 0;
  return props.system.surfaces.map((s) => {
    const v = z;
    z += s.thickness;
    return v;
  });
});
const imageZ = computed(() => props.system.surfaces.reduce((n, s) => n + s.thickness, 0));

/** Drawn half-height: the derived envelope, or the physical clear radius when it clips and is larger. */
const drawRadius = (s: Surface): number => Math.max(s.semi_diameter, s.clear_aperture ? s.aperture : 0);

const bounds = computed(() => {
  let zmin = 0, zmax = imageZ.value, ymax = Math.max(...props.system.surfaces.map(drawRadius), 1e-6);
  for (const r of props.layout.rays) {
    for (const p of r.points) {
      zmin = Math.min(zmin, p[2]);
      zmax = Math.max(zmax, p[2]);
      ymax = Math.max(ymax, Math.abs(p[1]));
    }
  }
  return { zmin, zmax: Math.max(zmax, zmin + 1e-6), ymax: ymax * 1.05 };
});
const sx = computed(() => (W - 2 * MX) / (bounds.value.zmax - bounds.value.zmin));
const sy = computed(() => Math.min(HALF / bounds.value.ymax, 4 * sx.value));
const exaggeration = computed(() => sy.value / sx.value);
const px = (z: number): number => MX + (z - bounds.value.zmin) * sx.value;
const py = (y: number): number => CY - y * sy.value;

const fieldColorIndex = (ray: Ray): number => {
  const i = props.system.fields.findIndex((f) => f.y === ray.field && f.x === ray.field_x);
  return Math.max(0, i);
};
const rayPath = (ray: Ray): string => ray.points.map((p) => `${px(p[2])},${py(p[1])}`).join(' ');

/** Sag matching the backend: conic base + even terms r^(4+2i) + odd terms r^(3+i). */
function sag(s: Surface, r: number): number | null {
  const c = s.radius === 0 ? 0 : 1 / s.radius;
  const q = 1 - (1 + s.conic) * c * c * r * r;
  if (q <= 0) return null;
  let z = (c * r * r) / (1 + Math.sqrt(q));
  s.even.forEach((a, k) => (z += a * Math.pow(r, 4 + 2 * k)));
  s.odd.forEach((a, k) => (z += a * Math.pow(r, 3 + k)));
  return Number.isFinite(z) ? z : null;
}

const surfaceShapes = computed(() =>
  props.system.surfaces.map((s, i) => {
    const pts: string[] = [];
    for (let k = -40; k <= 40; k++) {
      const y = (drawRadius(s) * k) / 40;
      const z = sag(s, Math.abs(y));
      if (z !== null) pts.push(`${px(vertices.value[i]! + z)},${py(y)}`);
    }
    const marker = s.clear_aperture && s.aperture > 0 ? sag(s, s.aperture) : null;
    const vz = vertices.value[i]!;
    return {
      points: pts.join(' '),
      interaction: s.interaction,
      label: s.label,
      top: py(drawRadius(s)) - 6,
      x: px(vz),
      // Physical clip edge: short ticks at ±clear radius, distinct from the derived drawing envelope.
      clips: marker === null ? [] : [s.aperture, -s.aperture].map((y) => ({ x: px(vz + marker), y: py(y) })),
    };
  }),
);
const style = (i: Surface['interaction']): { stroke: string; width: number; dash?: string } =>
  i === 'reflect' ? { stroke: '#b5532f', width: 3.4 } : i === 'dummy' ? { stroke: '#8da2b0', width: 1.4, dash: '5 4' } : { stroke: '#24526c', width: 2.8 };
</script>

<template>
  <section class="panel" aria-labelledby="layout-title">
    <div class="panel-head"><h3 id="layout-title">Meridional ray layout</h3><span>Object → image · mm · vertical ×{{ exaggeration.toFixed(1) }}</span></div>
    <svg :viewBox="`0 0 ${W} ${H}`" role="img" aria-labelledby="layout-svg-title">
      <title id="layout-svg-title">Meridional ray layout with {{ system.surfaces.length }} surfaces and {{ layout.rays.length }} rays; {{ layout.rejected.length }} rays rejected</title>
      <defs><pattern id="grid" width="40" height="40" patternUnits="userSpaceOnUse"><path d="M 40 0 L 0 0 0 40" fill="none" stroke="#dfe7ed" stroke-width=".5" /></pattern></defs>
      <rect :width="W" :height="H" fill="url(#grid)" />
      <line :x1="10" :y1="CY" :x2="W - 10" :y2="CY" stroke="#a7bac7" stroke-dasharray="6 5" />
      <polyline v-for="(r, i) in layout.rays" :key="`r${i}`" :points="rayPath(r)" fill="none" :stroke="colorFor(fieldColorIndex(r))" stroke-width="1.15" opacity=".8" />
      <g v-for="(s, i) in surfaceShapes" :key="`s${i}`">
        <polyline :points="s.points" fill="none" :stroke="style(s.interaction).stroke" :stroke-width="style(s.interaction).width" :stroke-dasharray="style(s.interaction).dash" />
        <line v-for="(c, k) in s.clips" :key="k" :x1="c.x - 5" :y1="c.y" :x2="c.x + 5" :y2="c.y" stroke="#c0392b" stroke-width="2.4" />
        <text :x="s.x" :y="s.top" font-size="9" fill="#5d7584" text-anchor="middle">{{ i + 1 }}{{ i === system.stop_index ? ' STOP' : '' }}</text>
      </g>
      <line :x1="px(imageZ)" :y1="CY - HALF" :x2="px(imageZ)" :y2="CY + HALF" stroke="#75989c" stroke-width="2" />
    </svg>
    <ul class="legend" aria-label="Layout legend">
      <li v-for="(_f, i) in system.fields" :key="i" :style="{ color: colorFor(i) }">━ field {{ fieldLabel(system, i) }}</li>
      <li>λ {{ wavelength }} nm</li>
      <li><span class="reflect-key">Red ticks</span>: physical clear radius (only where Clip rays is on); surface curves extend to the derived draw semi-diameter, which never clips</li>
      <li>Solid: refract · <span class="reflect-key">Thick: reflect</span> · Dashed: dummy</li>
    </ul>
    <details v-if="layout.rejected.length" class="data-table wide" open>
      <summary>{{ layout.rejected.length }} rejected rays (not drawn)</summary>
      <table>
        <caption class="sr-only">Rays the engine could not trace</caption>
        <thead><tr><th scope="col">Field</th><th scope="col">Pupil</th><th scope="col">Error</th></tr></thead>
        <tbody>
          <tr v-for="(r, i) in layout.rejected" :key="i">
            <td>{{ system.fields[r.field] ? fieldLabel(system, r.field) : r.field + 1 }}</td>
            <td>{{ r.pupil.toFixed(3) }}</td>
            <td>{{ r.error }}</td>
          </tr>
        </tbody>
      </table>
    </details>
  </section>
</template>
