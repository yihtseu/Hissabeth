<script setup lang="ts">
import { usePlotAppearance, plotColor } from '../composables/usePlotAppearance';
usePlotAppearance(draw);
import { tr } from '../i18n';
import { computed, onMounted, ref, watch } from 'vue';
import { interpolateRdBu, interpolateViridis } from 'd3-scale-chromatic';
import { extent, fmt } from '../chart';

const props = withDefaults(
    defineProps<{
        title: string;
        /** Row-major samples; row 0 is drawn at the bottom (+y up). null = no value. */
        values: (number | null)[];
        /** Rows. */
        size: number;
        /** Columns; defaults to size (square map). */
        width?: number;
        /** Cells outside the pupil by construction (null there is expected, not a failure). */
        outside?: boolean[];
        diverging?: boolean;
        log?: boolean;
        unit?: string;
    }>(),
    { outside: () => [], diverging: false, log: false, unit: '' },
);

const canvas = ref<HTMLCanvasElement | null>(null);
const cols = computed(() => props.width ?? props.size);
const cell = computed(() => Math.max(3, Math.floor(480 / Math.max(props.size, cols.value))));

const transformed = computed(() =>
    props.values.map((v) => (v === null ? null : props.log ? Math.log10(Math.max(v, 1e-12)) : v)),
);
const range = computed<[number, number]>(() => {
    const e = extent(transformed.value) ?? [0, 1];
    if (props.diverging) {
        const m = Math.max(Math.abs(e[0]), Math.abs(e[1]), 1e-12);
        return [-m, m];
    }
    return e[0] === e[1] ? [e[0], e[0] + 1] : e;
});
const failed = computed(
    () => props.values.filter((v, i) => v === null && !props.outside[i]).length,
);
const shown = computed(() => props.values.filter((v) => v !== null).length);

function color(t: number): string {
    t = Math.min(1, Math.max(0, t));
    // RdBu runs red→blue as t rises; flip it so low values stay blue like before.
    return props.diverging ? interpolateRdBu(1 - t) : interpolateViridis(t);
}
const ramp = computed(
    () => `linear-gradient(to right, ${[0, 0.25, 0.5, 0.75, 1].map(color).join(',')})`,
);

function draw(): void {
    const el = canvas.value;
    const ctx = el?.getContext('2d');
    if (!el || !ctx) return;
    const n = props.size,
        m = cols.value,
        c = cell.value;
    el.width = m * c;
    el.height = n * c;
    const [lo, hi] = range.value;
    for (let i = 0; i < n; i++) {
        for (let j = 0; j < m; j++) {
            const idx = i * m + j;
            const v = transformed.value[idx];
            const x = j * c,
                y = (n - 1 - i) * c;
            if (v === null || v === undefined) {
                const out = props.outside[idx];
                ctx.fillStyle = out ? plotColor('--surface') : plotColor('--danger-bg');
                ctx.fillRect(x, y, c, c);
                if (!out) {
                    // Hatch marks a sample that should have been valid but failed to trace.
                    ctx.strokeStyle = plotColor('--danger');
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
watch(
    [
        () => props.values,
        () => props.size,
        () => props.width,
        () => props.outside,
        () => props.diverging,
        () => props.log,
    ],
    draw,
);
</script>

<template>
    <figure class="chart heat">
        <figcaption>{{ title }}</figcaption>
        <canvas
            ref="canvas"
            role="img"
            :aria-label="
                tr('result.heatAria', {
                    title: title,
                    size: size,
                    shown: shown,
                    failed: failed,
                })
            "
        />
        <div class="ramp" :style="{ background: ramp }" aria-hidden="true"></div>
        <div class="ramp-labels">
            <span>{{ log ? '10^' : '' }}{{ fmt(range[0]) }}</span
            ><span>{{ unit }}</span
            ><span>{{ log ? '10^' : '' }}{{ fmt(range[1]) }}</span>
        </div>
        <ul class="legend" :aria-label="tr('result.heatMask', { title: title })">
            <li>{{ shown }} {{ tr('fragments.validSamples') }}</li>
            <li :class="{ 'reflect-key': failed > 0 }">
                ▨ {{ failed }} {{ tr('fragments.invalidFailedSamples') }}
            </li>
            <li>{{ tr('result.heatOutside') }}</li>
        </ul>
    </figure>
</template>
