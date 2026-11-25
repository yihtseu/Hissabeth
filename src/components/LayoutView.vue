<script setup lang="ts">
import { tr } from '../i18n';
import { usePreferencesStore } from '../stores/preferences';
const preferences = usePreferencesStore();
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { select } from 'd3-selection';
import { zoom, zoomIdentity, type D3ZoomEvent, type ZoomBehavior } from 'd3-zoom';
import { FALLBACK_COLOR, rgbCss } from '../color';
import { fmt } from '../chart';
import { drawRadius, profileCurve, sag, type Profile } from '../profile';
import type { Layout, OpticalSystem, Point3, Ray } from '../types';
import { fieldLabel } from '../validation';
import {
    clearSurfaceSelection,
    hoveredSurfaceIndex,
    hoverSurface,
    selectSurface,
    selectedSurfaceIndex,
    syncSurfaceSelection,
} from '../composables/useSurfaceSelection';

const props = defineProps<{ system: OpticalSystem; layout: Layout; wavelength: number }>();

/** Saved field color by field number; rays use the returned field index, so duplicate fields keep their own color. */
const fieldColor = (i: number | undefined): string =>
    rgbCss((i === undefined ? undefined : props.system.fields[i]?.color) ?? FALLBACK_COLOR);

const W = 900,
    H = 460,
    MX = 45,
    CY = H / 2,
    HALF = 190;

/** The compact view fits the optics and image; the source joins the fit only when asked for. */
const includeObject = ref(false);
const distance = computed(() => props.system.object_distance);
const showObject = computed(() => includeObject.value && distance.value !== null);
const objectZ = computed(() => -(distance.value ?? 0));

const objectCaption = computed(() =>
    tr('layout.objectIn', {
        label: props.system.object_label || tr('reportData.object'),
        material: props.system.object_material,
    }),
);
const imageCaption = computed(() => props.system.image_label || tr('reportData.image'));
/** Text for the source position, so a source outside the fit is still located. */
const objectPlace = computed(() =>
    distance.value === null
        ? tr('layout.placeInfinity')
        : tr('layout.placeVertex', { d: distance.value }) +
          (showObject.value ? '' : tr('layout.placeNotDrawn')),
);

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
const objectCurve = computed(() =>
    showObject.value ? curveOf(props.system.object_surface, objectZ.value) : [],
);

const drawnPoints = (r: Ray): Point3[] =>
    showObject.value && r.object_point ? [r.object_point, ...r.points] : r.points;

/** Actual internal profile curves, so bounds follow vertices that move back after a reflection. */
const surfaceCurves = computed(() =>
    props.system.surfaces.map((s, i) => profileCurve(s, vertices.value[i]!)),
);

const bounds = computed(() => {
    let zmin = Math.min(0, imageZ.value),
        zmax = Math.max(0, imageZ.value),
        ymax = opticsYmax.value;
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

const rayPath = (ray: Ray): string =>
    drawnPoints(ray)
        .map((p) => `${px(p[2])},${py(p[1])}`)
        .join(' ');
const polyline = (pts: [number, number][]): string =>
    pts.map(([z, y]) => `${px(z)},${py(y)}`).join(' ');

/** Short ticks at ±clear radius where the profile clips rays. */
function clipTicks(p: Profile, vertexZ: number): { x: number; y: number }[] {
    const z = p.clear_aperture && p.aperture > 0 ? sag(p, p.aperture) : null;
    return z === null
        ? []
        : [p.aperture, -p.aperture].map((y) => ({ x: px(vertexZ + z), y: py(y) }));
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
const reflections = computed(
    () => props.system.surfaces.filter((s) => s.interaction === 'reflect').length,
);

const MIN_ARROW_LEG = 48;
const MAX_ARROWS = 200;
/** Physical direction of light on leg `j` of the drawn path (object leg first when the object is drawn). */
function legDirection(ray: Ray, j: number): Point3 | null {
    const objectLeg = showObject.value && ray.object_point ? 1 : 0;
    if (objectLeg && j === 0) return ray.launch_direction;
    const leg = j - objectLeg;
    return leg < ray.segments.length
        ? ray.segments[leg]!.incoming
        : (ray.image_hit?.incoming ?? null);
}

/**
 * Direction arrows, only when a reflection exists, on one representative ray per field (the middle traced ray,
 * the chief when all are traced). Each leg gets its own arrow at the leg midpoint, oriented by the physical
 * incoming ray vector of that leg (so it stays correct along virtual extrapolations), and only when the leg is
 * long enough on screen.
 */
const arrows = computed(() => {
    const out: { x: number; y: number; deg: number; color: string }[] = [];
    if (!preferences.preferences.display.layoutRayArrows || reflections.value === 0) return out;
    const byField = new Map<number, number[]>();
    props.layout.rays.forEach((_r, i) => {
        const f = props.layout.field_indices[i];
        if (f !== undefined) byField.set(f, [...(byField.get(f) ?? []), i]);
    });
    for (const [field, indices] of byField) {
        const ray = props.layout.rays[indices[Math.floor(indices.length / 2)]!]!;
        const pts = drawnPoints(ray);
        for (let k = 0; k + 1 < pts.length && out.length < MAX_ARROWS; k++) {
            const x0 = px(pts[k]![2]),
                y0 = py(pts[k]![1]),
                x1 = px(pts[k + 1]![2]),
                y1 = py(pts[k + 1]![1]);
            const length = Math.hypot(x1 - x0, y1 - y0);
            if (!(length >= MIN_ARROW_LEG)) continue;
            const v = legDirection(ray, k);
            if (!v) continue;
            const dx = v[2] * sx.value,
                dy = -v[1] * sy.value;
            if (!(Math.hypot(dx, dy) > 0)) continue;
            out.push({
                x: (x0 + x1) / 2,
                y: (y0 + y1) / 2,
                deg: (Math.atan2(dy, dx) * 180) / Math.PI,
                color: fieldColor(field),
            });
        }
    }
    return out;
});

const style = (
    i: (typeof props.system.surfaces)[number]['interaction'],
): { stroke: string; width: number; dash?: string } =>
    i === 'reflect'
        ? { stroke: 'var(--optic-reflect)', width: 3.4 }
        : i === 'dummy'
          ? { stroke: 'var(--optic-dummy)', width: 1.4, dash: '5 4' }
          : { stroke: 'var(--optic-refract)', width: 2.8 };

const shapeText = (p: Profile): string =>
    p.radius === 0 && !p.even.length && !p.odd.length
        ? tr('layout.shapePlane')
        : tr('layout.shapeCurved');

/** Pan/zoom over the auto-fit base view; the user's framing survives data updates, double-click resets to fit. */
const svgEl = ref<SVGSVGElement | null>(null);
const zoomTransform = ref('');
/** Zoom transform components, so the HTML HUD can map viewBox coordinates to stage pixels. */
const viewK = ref(1);
const viewX = ref(0);
const viewY = ref(0);
/** Client size of the SVG stage, tracked for the HUD overlay mapping. */
const stageSize = ref({ w: W, h: H });
let zoomBehavior: ZoomBehavior<SVGSVGElement, unknown> | undefined;
let stageObserver: ResizeObserver | undefined;

onMounted(() => {
    const el = svgEl.value;
    if (!el) return;
    const selection = select(el);
    zoomBehavior = zoom<SVGSVGElement, unknown>()
        .scaleExtent([0.4, 40])
        .on('zoom', (event: D3ZoomEvent<SVGSVGElement, unknown>) => {
            zoomTransform.value = event.transform.toString();
            viewK.value = event.transform.k;
            viewX.value = event.transform.x;
            viewY.value = event.transform.y;
        });
    selection.call(zoomBehavior);
    selection.on('dblclick.zoom', (event: MouseEvent) => {
        event.preventDefault();
        selection.call(zoomBehavior!.transform, zoomIdentity);
    });
    stageObserver = new ResizeObserver((entries) => {
        const box = entries[0]?.contentRect;
        if (box && box.width > 0 && box.height > 0)
            stageSize.value = { w: box.width, h: box.height };
    });
    stageObserver.observe(el);
});

onBeforeUnmount(() => {
    if (svgEl.value) select(svgEl.value).on('.zoom', null);
    stageObserver?.disconnect();
    // The pointer can no longer be over a layout surface once this view is gone.
    hoverSurface(null);
});

// ---- shared surface selection (SurfaceTable / PrescriptionView / MaterialsPanel) ----

/** Report the internal surface count so stale shared indexes are dropped on data updates. */
watch(
    () => props.system.surfaces.length,
    (n) => syncSurfaceSelection(n),
    { immediate: true },
);

/**
 * The surface the HUD and ray markers follow: the hovered one wins, otherwise the pinned
 * selection. Every access is guarded against the shared state holding an index that no
 * longer exists after an edit, so no glow, marker or HUD can go stale.
 */
const focusIndex = computed(() => {
    const n = props.system.surfaces.length;
    const h = hoveredSurfaceIndex.value;
    if (h !== null && h < n) return h;
    const s = selectedSurfaceIndex.value;
    return s !== null && s < n ? s : null;
});

/** Medium a reflect/dummy row retains: the material of the last refracting row above it, else the object medium. */
function incidentMedium(i: number): string {
    for (let k = i - 1; k >= 0; k--)
        if (props.system.surfaces[k]!.interaction === 'refract')
            return props.system.surfaces[k]!.material;
    return props.system.object_material;
}

const surfaceAriaLabel = (i: number): string => {
    const s = props.system.surfaces[i]!;
    const parts = [`${tr('rx.colSurface')} ${i + 1}`];
    if (s.label) parts.push(s.label);
    if (i === props.system.stop_index) parts.push(tr('layout.stop'));
    return parts.join(' · ');
};

function clearHover(i: number): void {
    if (hoveredSurfaceIndex.value === i) hoverSurface(null);
}

function pickSurface(i: number): void {
    selectSurface(i);
}

/** Move keyboard focus (and hover) to another internal surface, wrapping around the ends. */
function focusSurface(i: number): void {
    const n = props.system.surfaces.length;
    if (!n || !svgEl.value) return;
    const next = ((i % n) + n) % n;
    svgEl.value.querySelector<SVGGElement>(`[data-surface-index="${next}"]`)?.focus();
    hoverSurface(next);
}

function onSurfaceKeydown(event: KeyboardEvent, i: number): void {
    switch (event.key) {
        case 'Enter':
        case ' ':
            event.preventDefault();
            pickSurface(i);
            break;
        case 'Escape':
            event.preventDefault();
            clearSurfaceSelection();
            break;
        case 'ArrowRight':
            event.preventDefault();
            focusSurface(i + 1);
            break;
        case 'ArrowLeft':
            event.preventDefault();
            focusSurface(i - 1);
            break;
    }
}

/**
 * Per-ray impact markers on the focused surface: points[i+1] is the hit on internal surface i
 * (points[0] is the launch, the tail is the image hit), so endpoints are never mapped as
 * internal surfaces.
 */
const hitMarkers = computed(() => {
    const i = focusIndex.value;
    if (i === null) return [];
    const out: { key: number; x: number; y: number; color: string }[] = [];
    props.layout.rays.forEach((r, k) => {
        const p = r.points[i + 1];
        if (!p) return;
        out.push({
            key: k,
            x: px(p[2]),
            y: py(p[1]),
            color: fieldColor(props.layout.field_indices[k]),
        });
    });
    return out;
});

/** HUD payload for the focused surface; null when nothing valid is hovered or selected. */
const hud = computed(() => {
    const i = focusIndex.value;
    if (i === null) return null;
    const s = props.system.surfaces[i];
    if (!s) return null;
    return {
        index: i,
        title: `S${i + 1}`,
        label: s.label,
        stop: i === props.system.stop_index,
        radius:
            s.radius === 0 && !s.even.length && !s.odd.length
                ? tr('layout.shapePlane')
                : fmt(s.radius),
        material: s.interaction === 'refract' ? s.material : incidentMedium(i),
        semiDia: fmt(s.semi_diameter),
        clear: s.clear_aperture ? fmt(s.aperture) : null,
        vertexZ: vertices.value[i]!,
        topY: drawRadius(s),
    };
});

/** viewBox → stage mapping for `preserveAspectRatio="xMidYMid meet"`. */
const stageFit = computed(() => {
    const { w, h } = stageSize.value;
    const k = Math.min(w / W, h / H);
    return { k, ox: (w - W * k) / 2, oy: (h - H * k) / 2 };
});

const HUD_W = 210;
const HUD_PAD = 8;

/** Place the HUD just above the focused surface, clamped inside the stage. */
const hudStyle = computed(() => {
    const info = hud.value;
    if (!info) return {};
    const vbX = viewX.value + viewK.value * px(info.vertexZ);
    const vbY = viewY.value + viewK.value * (py(info.topY) - 14);
    const x = stageFit.value.ox + stageFit.value.k * vbX;
    const y = stageFit.value.oy + stageFit.value.k * vbY;
    const left = Math.min(
        Math.max(x - HUD_W / 2, HUD_PAD),
        Math.max(HUD_PAD, stageSize.value.w - HUD_W - HUD_PAD),
    );
    // CSS translateY(-100%) lifts the HUD fully above this anchor point.
    return { left: `${left}px`, top: `${Math.max(HUD_PAD, y)}px` };
});
</script>

<template>
    <section class="panel" aria-labelledby="layout-title">
        <div class="panel-head">
            <h3 id="layout-title">{{ tr('layout.title') }}</h3>
            <span>{{ tr('fragments.objectImageMmVertical') }}{{ exaggeration.toFixed(1) }}</span>
            <span class="optical-hint">{{ tr('layout.zoomHint') }}</span>
            <label class="inline"
                ><input v-model="includeObject" type="checkbox" :disabled="distance === null" />
                {{ tr('layout.includeObject')
                }}{{ distance === null ? tr('layout.atInfinity') : ` (${distance} mm)` }}</label
            >
        </div>
        <div class="layout-stage">
            <svg
                ref="svgEl"
                class="zoomable"
                :viewBox="`0 0 ${W} ${H}`"
                role="img"
                aria-labelledby="layout-svg-title"
                @click.self="clearSurfaceSelection()"
            >
                <title id="layout-svg-title">
                    {{ tr('fragments.meridionalRayLayoutWith') }} {{ system.surfaces.length }}
                    {{ tr('fragments.surfacesAnd') }} {{ layout.rays.length }}
                    {{ tr('fragments.rays') }} {{ layout.rejected.length }}
                    {{ tr('fragments.raysRejected') }} {{ shapeText(system.image_surface) }}
                    {{ tr('aperture.limitEndpointImage')
                    }}{{
                        showObject
                            ? tr('layout.svgObjectDrawn', {
                                  shape: shapeText(system.object_surface),
                              })
                            : tr('layout.svgObjectNotDrawn', { place: objectPlace })
                    }}
                </title>
                <defs>
                    <pattern id="grid" width="40" height="40" patternUnits="userSpaceOnUse">
                        <path
                            d="M 40 0 L 0 0 0 40"
                            fill="none"
                            stroke="var(--border)"
                            stroke-width=".5"
                        />
                    </pattern>
                </defs>
                <rect
                    v-if="preferences.preferences.display.plotGrid"
                    class="plot-grid"
                    :width="W"
                    :height="H"
                    fill="url(#grid)"
                />
                <g :transform="zoomTransform || undefined">
                    <line
                        :x1="10"
                        :y1="CY"
                        :x2="W - 10"
                        :y2="CY"
                        stroke="var(--plot-ink)"
                        stroke-dasharray="6 5"
                    />
                    <polyline
                        v-for="(r, i) in layout.rays"
                        class="data-mark"
                        :key="`r${i}`"
                        :points="rayPath(r)"
                        fill="none"
                        :stroke="fieldColor(layout.field_indices[i])"
                        stroke-width="1.15"
                        opacity=".8"
                    />
                    <polygon
                        v-for="(a, i) in arrows"
                        :key="`a${i}`"
                        points="6,0 -5,3.5 -5,-3.5"
                        :transform="`translate(${a.x} ${a.y}) rotate(${a.deg})`"
                        :fill="a.color"
                    />
                    <g
                        v-for="(s, i) in surfaceShapes"
                        :key="`s${i}`"
                        class="surface"
                        :class="{
                            'is-hovered': hoveredSurfaceIndex === i,
                            'is-selected': selectedSurfaceIndex === i,
                        }"
                        :data-surface-index="i"
                        tabindex="0"
                        role="button"
                        :aria-pressed="selectedSurfaceIndex === i"
                        :aria-label="surfaceAriaLabel(i)"
                        @mouseenter="hoverSurface(i)"
                        @mouseleave="clearHover(i)"
                        @focus="hoverSurface(i)"
                        @blur="clearHover(i)"
                        @click="pickSurface(i)"
                        @keydown="onSurfaceKeydown($event, i)"
                    >
                        <polyline
                            class="surface-hit"
                            aria-hidden="true"
                            tabindex="-1"
                            :points="s.points"
                            fill="none"
                            stroke="transparent"
                            stroke-width="18"
                            pointer-events="stroke"
                        />
                        <polyline
                            :points="s.points"
                            fill="none"
                            :stroke="style(s.interaction).stroke"
                            :stroke-width="style(s.interaction).width"
                            :stroke-dasharray="style(s.interaction).dash"
                        />
                        <line
                            v-for="(c, k) in s.clips"
                            :key="k"
                            :x1="c.x - 5"
                            :y1="c.y"
                            :x2="c.x + 5"
                            :y2="c.y"
                            stroke="var(--optic-aperture)"
                            stroke-width="2.4"
                        />
                        <text
                            :x="s.x"
                            :y="s.top"
                            font-size="9"
                            fill="var(--plot-ink)"
                            text-anchor="middle"
                        >
                            {{ i + 1 }}{{ i === system.stop_index ? tr('layout.stop') : '' }}
                        </text>
                    </g>
                    <g v-if="hitMarkers.length" class="hit-markers" aria-hidden="true">
                        <circle
                            v-for="m in hitMarkers"
                            :key="`m${m.key}`"
                            class="hit-marker"
                            :cx="m.x"
                            :cy="m.y"
                            r="3.2"
                            :fill="m.color"
                            stroke="var(--panel)"
                            stroke-width="1"
                        />
                    </g>
                    <g v-if="showObject">
                        <polyline
                            :points="objectShape.points"
                            fill="none"
                            stroke="var(--optic-endpoint)"
                            stroke-width="2"
                        />
                        <line
                            v-for="(c, k) in objectShape.clips"
                            :key="k"
                            :x1="c.x - 5"
                            :y1="c.y"
                            :x2="c.x + 5"
                            :y2="c.y"
                            stroke="var(--optic-aperture)"
                            stroke-width="2.4"
                        />
                        <text
                            :x="px(objectZ)"
                            :y="CY - HALF - 6"
                            font-size="11"
                            fill="var(--text)"
                            text-anchor="middle"
                        >
                            {{ objectCaption }}
                        </text>
                    </g>
                    <text v-else x="10" y="16" font-size="11" fill="var(--text)">
                        {{ objectCaption }} · {{ objectPlace }}
                    </text>
                    <polyline
                        :points="imageShape.points"
                        fill="none"
                        stroke="var(--optic-endpoint)"
                        stroke-width="2"
                    />
                    <line
                        v-for="(c, k) in imageShape.clips"
                        :key="`ic${k}`"
                        :x1="c.x - 5"
                        :y1="c.y"
                        :x2="c.x + 5"
                        :y2="c.y"
                        stroke="var(--optic-aperture)"
                        stroke-width="2.4"
                    />
                    <text
                        :x="px(imageZ)"
                        :y="CY - HALF - 6"
                        font-size="11"
                        fill="var(--text)"
                        text-anchor="middle"
                    >
                        {{ imageCaption }}
                    </text>
                </g>
            </svg>
            <div v-if="hud" class="surface-hud" :style="hudStyle" aria-hidden="true">
                <header class="surface-hud-head">
                    <span class="surface-hud-title">{{ hud.title }}</span>
                    <span v-if="hud.label" class="surface-hud-label">{{ hud.label }}</span>
                    <span v-if="hud.stop" class="surface-hud-stop">{{ tr('layout.stop') }}</span>
                </header>
                <dl class="surface-hud-grid">
                    <dt>{{ tr('rx.colRadius') }}</dt>
                    <dd>{{ hud.radius }}</dd>
                    <dt>{{ tr('rx.colMaterial') }}</dt>
                    <dd>{{ hud.material }}</dd>
                    <dt>{{ tr('rx.colDraw') }}</dt>
                    <dd>{{ hud.semiDia }}</dd>
                    <template v-if="hud.clear !== null">
                        <dt>{{ tr('rx.colClear') }}</dt>
                        <dd>{{ hud.clear }}</dd>
                    </template>
                </dl>
            </div>
        </div>
        <ul class="legend" :aria-label="tr('layout.legend')">
            <li v-for="(_f, i) in system.fields" :key="i" :style="{ color: fieldColor(i) }">
                {{ tr('fragments.fieldValue') }} {{ fieldLabel(system, i) }}
            </li>
            <li>
                {{ tr('fragments.object') }} {{ objectCaption }}, {{ objectPlace }}
                {{ tr('fragments.image') }} {{ imageCaption }}{{ tr('fragments.vertexAtZ') }}
                {{ imageZ }} mm
            </li>
            <li>λ {{ wavelength }} nm</li>
            <li>
                <span class="reflect-key">{{ tr('fragments.redTicks') }}</span
                >{{
                    tr(
                        'fragments.physicalClearRadiusOnlyWhereClipRaysIsOnCurvesExtendToTheDerivedDrawSemiDiameterWhich',
                    )
                }}
            </li>
            <li v-if="reflections > 0">
                {{ tr('layout.legendArrows') }}
            </li>
            <li v-if="reflections > 0">
                {{ reflections }} {{ tr('fragments.centeredReflection')
                }}{{ reflections === 1 ? '' : 's'
                }}{{ tr('fragments.travelReversesAlongTheAxisSoLightLeavesTheLastSurfaceToward') }}
                {{ reflections % 2 ? tr('layout.dirNeg') : tr('layout.dirPos')
                }}{{ tr('fragments.tiltedOrDecenteredFoldsAreNotSupported') }}
            </li>
            <li>
                {{ tr('fragments.solidRefract') }}
                <span class="reflect-key">{{ tr('fragments.thickReflect') }}</span>
                {{ tr('fragments.dashedDummy') }}
            </li>
        </ul>
        <details v-if="layout.rejected.length" class="data-table wide" open>
            <summary>
                {{ layout.rejected.length }} {{ tr('fragments.rejectedRaysNotDrawn') }}
            </summary>
            <table>
                <caption class="sr-only">
                    {{
                        tr('layout.rejectedCaption')
                    }}
                </caption>
                <thead>
                    <tr>
                        <th scope="col">{{ tr('layout.colField') }}</th>
                        <th scope="col">{{ tr('layout.colPupil') }}</th>
                        <th scope="col">{{ tr('layout.colError') }}</th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="(r, i) in layout.rejected" :key="i">
                        <td>
                            {{ system.fields[r.field] ? fieldLabel(system, r.field) : r.field + 1 }}
                        </td>
                        <td>{{ r.pupil.toFixed(3) }}</td>
                        <td>{{ r.error }}</td>
                    </tr>
                </tbody>
            </table>
        </details>
    </section>
</template>

<style scoped>
.layout-stage {
    position: relative;
}

/* Interactive surfaces: a transparent fat stroke catches pointer/keyboard events while the
   visible curve keeps its exact paint attributes (tests select on stroke colors/widths). */
.surface {
    cursor: pointer;
}
.surface .surface-hit {
    fill: none;
    stroke: transparent;
    pointer-events: stroke;
}
.surface:focus {
    outline: none;
}
.surface:focus-visible .surface-hit {
    stroke: var(--accent);
    stroke-opacity: 0.45;
}
.surface.is-hovered {
    filter: drop-shadow(0 0 3px var(--accent));
}
.surface.is-selected {
    filter: drop-shadow(0 0 2px var(--accent)) drop-shadow(0 0 6px var(--accent));
}
.surface.is-selected.is-hovered {
    filter: drop-shadow(0 0 3px var(--accent)) drop-shadow(0 0 9px var(--accent));
}

.hit-marker {
    pointer-events: none;
}

.surface-hud {
    position: absolute;
    z-index: 2;
    width: v-bind('`${HUD_W}px`');
    box-sizing: border-box;
    padding: 7px 10px 8px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    background: var(--panel);
    box-shadow: var(--shadow-sm);
    color: var(--text);
    font-size: 12px;
    line-height: 1.55;
    pointer-events: none;
    transform: translateY(calc(-100% - 6px));
    animation: surface-hud-in 120ms ease-out;
}
.surface-hud-head {
    display: flex;
    align-items: baseline;
    gap: 6px;
    margin-bottom: 2px;
}
.surface-hud-title {
    font-weight: 650;
    font-variant-numeric: tabular-nums;
}
.surface-hud-label {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
}
.surface-hud-stop {
    margin-left: auto;
    color: var(--accent);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.06em;
}
.surface-hud-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: 12px;
    margin: 0;
}
.surface-hud-grid dt {
    color: var(--muted);
}
.surface-hud-grid dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
}

@keyframes surface-hud-in {
    from {
        opacity: 0;
    }
    to {
        opacity: 1;
    }
}
</style>
