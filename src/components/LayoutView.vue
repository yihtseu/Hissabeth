<script setup lang="ts">
import { computed, ref } from 'vue';
import { FALLBACK_COLOR, rgbCss } from '../color';
import { drawRadius, profileCurve, sag, type Profile } from '../profile';
import type { Layout, OpticalSystem, Point3, Ray } from '../types';
import { fieldLabel } from '../validation';

const props = defineProps<{ system: OpticalSystem; layout: Layout; wavelength: number }>();

/** Saved field color by field number; rays use the returned field index, so duplicate fields keep their own color. */
const fieldColor = (i: number | undefined): string => rgbCss((i === undefined ? undefined : props.system.fields[i]?.color) ?? FALLBACK_COLOR);

const W = 900, H = 460, MX = 45, CY = H / 2, HALF = 190;

/** The compact view fits the optics and image; the source joins the fit only when asked for. */
const includeObject = ref(false);
const distance = computed(() => props.system.object_distance);
const showObject = computed(() => includeObject.value && distance.value !== null);
const objectZ = computed(() => -(distance.value ?? 0));

const objectCaption = computed(() => `${props.system.object_label || 'Object'} in ${props.system.object_material}`);
const imageCaption = computed(() => props.system.image_label || 'Image');
/** Text for the source position, so a source outside the fit is still located. */
const objectPlace = computed(() =>
  distance.value === null ? 'at infinity' : `vertex at z = −${distance.value} mm${showObject.value ? '' : ' (not drawn)'}`);

/** Axial vertex position of each surface; the image vertex follows the last thickness (signed). */
const vertices = computed(() => {
  let z = 0;
  return props.system.surfaces.map((s) => {
    const v = z;
    z += s.thickness;
    return v;
  });
});
const imageZ = computed(() => props.system.surfaces.reduce((n, s) => n + s.thickness, 0));

/** Largest half-height of the optics and traced rays, used to size endpoint curves that have no envelope yet. */
const opticsYmax = computed(() => {
  let y = Math.max(...props.system.surfaces.map(drawRadius), 1e-6);
  for (const r of props.layout.rays) for (const p of r.points) y = Math.max(y, Math.abs(p[1]));
  return y;
});
/** Endpoint curve drawn over its envelope, or over the optics height when the envelope is 0 (a plane with no derived size). */
const curveOf = (p: Profile, vertexZ: number): [number, number][] => {
  const h = drawRadius(p) > 0 ? drawRadius(p) : opticsYmax.value;
  return profileCurve({ ...p, semi_diameter: h, clear_aperture: false }, vertexZ);
};
const imageCurve = computed(() => curveOf(props.system.image_surface, imageZ.value));
const objectCurve = computed(() => (showObject.value ? curveOf(props.system.object_surface, objectZ.value) : []));

const drawnPoints = (r: Ray): Point3[] => (showObject.value && r.object_point ? [r.object_point, ...r.points] : r.points);

/** Actual internal profile curves, so bounds follow vertices that move back after a reflection. */
const surfaceCurves = computed(() => props.system.surfaces.map((s, i) => profileCurve(s, vertices.value[i]!)));

const bounds = computed(() => {
  let zmin = Math.min(0, imageZ.value), zmax = Math.max(0, imageZ.value), ymax = opticsYmax.value;
  const take = (z: number, y: number): void => {
    zmin = Math.min(zmin, z);
    zmax = Math.max(zmax, z);
    ymax = Math.max(ymax, Math.abs(y));
  };
  for (const v of vertices.value) take(v, 0);
  for (const curve of surfaceCurves.value) for (const [z, y] of curve) take(z, y);
  for (const [z, y] of imageCurve.value) take(z, y);
  for (const [z, y] of objectCurve.value) take(z, y);
  for (const r of props.layout.rays) for (const p of drawnPoints(r)) take(p[2], p[1]);
  return { zmin, zmax: Math.max(zmax, zmin + 1e-6), ymax: ymax * 1.05 };
});
const sx = computed(() => (W - 2 * MX) / (bounds.value.zmax - bounds.value.zmin));
const sy = computed(() => Math.min(HALF / bounds.value.ymax, 4 * sx.value));
const exaggeration = computed(() => sy.value / sx.value);
const px = (z: number): number => MX + (z - bounds.value.zmin) * sx.value;
const py = (y: number): number => CY - y * sy.value;

const rayPath = (ray: Ray): string => drawnPoints(ray).map((p) => `${px(p[2])},${py(p[1])}`).join(' ');
const polyline = (pts: [number, number][]): string => pts.map(([z, y]) => `${px(z)},${py(y)}`).join(' ');

/** Short ticks at ±clear radius where the profile clips rays. */
function clipTicks(p: Profile, vertexZ: number): { x: number; y: number }[] {
  const z = p.clear_aperture && p.aperture > 0 ? sag(p, p.aperture) : null;
  return z === null ? [] : [p.aperture, -p.aperture].map((y) => ({ x: px(vertexZ + z), y: py(y) }));
}

const surfaceShapes = computed(() =>
  props.system.surfaces.map((s, i) => {
    const vz = vertices.value[i]!;
    return {
      points: polyline(surfaceCurves.value[i]!),
      interaction: s.interaction,
      label: s.label,
      top: py(drawRadius(s)) - 6,
      x: px(vz),
      // Physical clip edge: short ticks at ±clear radius, distinct from the derived drawing envelope.
      clips: clipTicks(s, vz),
    };
  }),
);
const imageShape = computed(() => ({
  points: polyline(imageCurve.value),
  clips: clipTicks(props.system.image_surface, imageZ.value),
}));
const objectShape = computed(() => ({
  points: polyline(objectCurve.value),
  clips: clipTicks(props.system.object_surface, objectZ.value),
}));

/** Centered reflections reverse real travel along z; the count gives the image-space direction. */
const reflections = computed(() => props.system.surfaces.filter((s) => s.interaction === 'reflect').length);

const MIN_ARROW_LEG = 48;
const MAX_ARROWS = 200;
/** Physical direction of light on leg `j` of the drawn path (object leg first when the object is drawn). */
function legDirection(ray: Ray, j: number): Point3 | null {
  const objectLeg = showObject.value && ray.object_point ? 1 : 0;
  if (objectLeg && j === 0) return ray.launch_direction;
  const leg = j - objectLeg;
  return leg < ray.segments.length ? ray.segments[leg]!.incoming : (ray.image_hit?.incoming ?? null);
}

/**
 * Direction arrows, only when a reflection exists, on one representative ray per field (the middle traced ray,
 * the chief when all are traced). Each leg gets its own arrow at the leg midpoint, oriented by the physical
 * incoming ray vector of that leg (so it stays correct along virtual extrapolations), and only when the leg is
 * long enough on screen.
 */
const arrows = computed(() => {
  const out: { x: number; y: number; deg: number; color: string }[] = [];
  if (reflections.value === 0) return out;
  const byField = new Map<number, number[]>();
  props.layout.rays.forEach((_r, i) => {
    const f = props.layout.field_indices[i];
    if (f !== undefined) byField.set(f, [...(byField.get(f) ?? []), i]);
  });
  for (const [field, indices] of byField) {
    const ray = props.layout.rays[indices[Math.floor(indices.length / 2)]!]!;
    const pts = drawnPoints(ray);
    for (let k = 0; k + 1 < pts.length && out.length < MAX_ARROWS; k++) {
      const x0 = px(pts[k]![2]), y0 = py(pts[k]![1]), x1 = px(pts[k + 1]![2]), y1 = py(pts[k + 1]![1]);
      const length = Math.hypot(x1 - x0, y1 - y0);
      if (!(length >= MIN_ARROW_LEG)) continue;
      const v = legDirection(ray, k);
      if (!v) continue;
      const dx = v[2] * sx.value, dy = -v[1] * sy.value;
      if (!(Math.hypot(dx, dy) > 0)) continue;
      out.push({ x: (x0 + x1) / 2, y: (y0 + y1) / 2, deg: (Math.atan2(dy, dx) * 180) / Math.PI, color: fieldColor(field) });
    }
  }
  return out;
});

const style = (i: (typeof props.system.surfaces)[number]['interaction']): { stroke: string; width: number; dash?: string } =>
  i === 'reflect' ? { stroke: '#b5532f', width: 3.4 } : i === 'dummy' ? { stroke: '#8da2b0', width: 1.4, dash: '5 4' } : { stroke: '#24526c', width: 2.8 };

const shapeText = (p: Profile): string => (p.radius === 0 && !p.even.length && !p.odd.length ? 'plane' : 'curved');
</script>

<template>
  <section class="panel" aria-labelledby="layout-title">
    <div class="panel-head">
      <h3 id="layout-title">Meridional ray layout</h3>
      <span>Object → image · mm · vertical ×{{ exaggeration.toFixed(1) }}</span>
      <label class="inline"><input v-model="includeObject" type="checkbox" :disabled="distance === null" /> Include object{{ distance === null ? ' (at infinity)' : ` (${distance} mm)` }}</label>
    </div>
    <svg :viewBox="`0 0 ${W} ${H}`" role="img" aria-labelledby="layout-svg-title">
      <title id="layout-svg-title">Meridional ray layout with {{ system.surfaces.length }} surfaces and {{ layout.rays.length }} rays; {{ layout.rejected.length }} rays rejected; {{ shapeText(system.image_surface) }} image surface{{ showObject ? `, ${shapeText(system.object_surface)} object surface drawn` : `, object ${objectPlace}` }}</title>
      <defs><pattern id="grid" width="40" height="40" patternUnits="userSpaceOnUse"><path d="M 40 0 L 0 0 0 40" fill="none" stroke="#dfe7ed" stroke-width=".5" /></pattern></defs>
      <rect :width="W" :height="H" fill="url(#grid)" />
      <line :x1="10" :y1="CY" :x2="W - 10" :y2="CY" stroke="#a7bac7" stroke-dasharray="6 5" />
      <polyline v-for="(r, i) in layout.rays" :key="`r${i}`" :points="rayPath(r)" fill="none" :stroke="fieldColor(layout.field_indices[i])" stroke-width="1.15" opacity=".8" />
      <polygon v-for="(a, i) in arrows" :key="`a${i}`" points="6,0 -5,3.5 -5,-3.5" :transform="`translate(${a.x} ${a.y}) rotate(${a.deg})`" :fill="a.color" />
      <g v-for="(s, i) in surfaceShapes" :key="`s${i}`">
        <polyline :points="s.points" fill="none" :stroke="style(s.interaction).stroke" :stroke-width="style(s.interaction).width" :stroke-dasharray="style(s.interaction).dash" />
        <line v-for="(c, k) in s.clips" :key="k" :x1="c.x - 5" :y1="c.y" :x2="c.x + 5" :y2="c.y" stroke="#c0392b" stroke-width="2.4" />
        <text :x="s.x" :y="s.top" font-size="9" fill="#5d7584" text-anchor="middle">{{ i + 1 }}{{ i === system.stop_index ? ' STOP' : '' }}</text>
      </g>
      <g v-if="showObject">
        <polyline :points="objectShape.points" fill="none" stroke="#75989c" stroke-width="2" />
        <line v-for="(c, k) in objectShape.clips" :key="k" :x1="c.x - 5" :y1="c.y" :x2="c.x + 5" :y2="c.y" stroke="#c0392b" stroke-width="2.4" />
        <text :x="px(objectZ)" :y="CY - HALF - 6" font-size="11" fill="#3f5868" text-anchor="middle">{{ objectCaption }}</text>
      </g>
      <text v-else x="10" y="16" font-size="11" fill="#3f5868">{{ objectCaption }} · {{ objectPlace }}</text>
      <polyline :points="imageShape.points" fill="none" stroke="#75989c" stroke-width="2" />
      <line v-for="(c, k) in imageShape.clips" :key="`ic${k}`" :x1="c.x - 5" :y1="c.y" :x2="c.x + 5" :y2="c.y" stroke="#c0392b" stroke-width="2.4" />
      <text :x="px(imageZ)" :y="CY - HALF - 6" font-size="11" fill="#3f5868" text-anchor="middle">{{ imageCaption }}</text>
    </svg>
    <ul class="legend" aria-label="Layout legend">
      <li v-for="(_f, i) in system.fields" :key="i" :style="{ color: fieldColor(i) }">━ field {{ fieldLabel(system, i) }}</li>
      <li>Object: {{ objectCaption }}, {{ objectPlace }} · Image: {{ imageCaption }}, vertex at z = {{ imageZ }} mm</li>
      <li>λ {{ wavelength }} nm</li>
      <li><span class="reflect-key">Red ticks</span>: physical clear radius (only where Clip rays is on); curves extend to the derived draw semi-diameter, which never clips</li>
      <li v-if="reflections > 0">Arrows: physical travel direction on one ray per field, per leg where there is room; shown even along legs drawn as virtual extrapolations</li>
      <li v-if="reflections > 0">{{ reflections }} centered reflection{{ reflections === 1 ? '' : 's' }}: travel reverses along the axis, so light leaves the last surface toward {{ reflections % 2 ? '−z (left)' : '+z (right)' }}. Tilted or decentered folds are not supported.</li>
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
