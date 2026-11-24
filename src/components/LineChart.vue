<script setup lang="ts">
import { tr } from '../i18n';
import { usePreferencesStore } from '../stores/preferences';
const preferences = usePreferencesStore();
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useResizeObserver } from '@vueuse/core';
import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';
import { fmt, type ChartSeries } from '../chart';
import { plotColor, usePlotAppearance } from '../composables/usePlotAppearance';

const props = withDefaults(
    defineProps<{
        title: string;
        series: ChartSeries[];
        xLabel: string;
        yLabel: string;
        yMin?: number;
        yMax?: number;
        includeZero?: boolean;
        /** x positions of samples that failed; drawn as red markers on the x axis. */
        invalidX?: number[];
    }>(),
    { yMin: undefined, yMax: undefined, includeZero: false, invalidX: () => [] },
);

const holder = ref<HTMLElement>();
let plot: uPlot | undefined;

/** uPlot needs one shared sorted x column; each series fills null where it has no sample. */
const data = computed<uPlot.AlignedData>(() => {
    const xs = [...new Set(props.series.flatMap((s) => s.points.map((p) => p.x)))].toSorted(
        (a, b) => a - b,
    );
    const columns = props.series.map((s) => {
        const byX = new Map(s.points.map((p) => [p.x, p.y]));
        return xs.map((x) => byX.get(x) ?? null);
    });
    return [xs, ...columns];
});

const summary = computed(() => {
    const valid = props.series.flatMap((s) => s.points).filter((p) => p.y !== null).length;
    return tr('result.chartSummary', {
        title: props.title,
        series: props.series.map((s) => s.label).join(', '),
        valid: valid,
        invalid: props.invalidX.length,
    });
});

function build(): void {
    plot?.destroy();
    plot = undefined;
    const el = holder.value;
    if (!el) return;
    const ink = plotColor('--plot-ink') || '#888';
    const grid = preferences.preferences.display.plotGrid
        ? { stroke: plotColor('--grid'), width: 1 }
        : { show: false };
    const width = el.clientWidth || 900;
    plot = new uPlot(
        {
            width,
            height: Math.round((width * 340) / 900),
            legend: { show: false },
            cursor: { show: false },
            scales: {
                x: {
                    time: false,
                    range: (_u, min, max) => {
                        const lo = min ?? 0,
                            hi = max ?? 1;
                        return lo === hi ? [lo - 1, hi + 1] : [lo, hi];
                    },
                },
                y: {
                    range: (_u, min, max) => {
                        let lo = min ?? 0,
                            hi = max ?? 1;
                        if (props.includeZero) {
                            lo = Math.min(lo, 0);
                            hi = Math.max(hi, 0);
                        }
                        if (props.yMin !== undefined) lo = props.yMin;
                        if (props.yMax !== undefined) hi = props.yMax;
                        if (lo === hi) {
                            lo -= 1;
                            hi += 1;
                        }
                        const pad =
                            props.yMin === undefined || props.yMax === undefined
                                ? (hi - lo) * 0.05
                                : 0;
                        return [
                            props.yMin !== undefined ? lo : lo - pad,
                            props.yMax !== undefined ? hi : hi + pad,
                        ];
                    },
                },
            },
            axes: [
                {
                    stroke: ink,
                    label: props.xLabel,
                    grid,
                    values: (_u, splits) => splits.map((v) => fmt(v)),
                },
                {
                    stroke: ink,
                    label: props.yLabel,
                    size: 70,
                    grid,
                    values: (_u, splits) => splits.map((v) => fmt(v)),
                },
            ],
            series: [
                {},
                ...props.series.map((s): uPlot.Series => ({
                    stroke: s.color,
                    width: 2.2,
                    dash: s.dashed ? [6, 4] : undefined,
                    spanGaps: false,
                    points: { show: s.points.length <= 64, size: 5 },
                })),
            ],
            hooks: {
                draw: [
                    (u) => {
                        const ctx = u.ctx;
                        const yMin = u.scales.y.min ?? 0,
                            yMax = u.scales.y.max ?? 0;
                        if (yMin < 0 && yMax > 0) {
                            const y = u.valToPos(0, 'y', true);
                            ctx.strokeStyle = ink;
                            ctx.lineWidth = 1;
                            ctx.beginPath();
                            ctx.moveTo(u.bbox.left, y);
                            ctx.lineTo(u.bbox.left + u.bbox.width, y);
                            ctx.stroke();
                        }
                        if (props.invalidX.length) {
                            const base = u.bbox.top + u.bbox.height;
                            ctx.fillStyle = plotColor('--danger') || 'red';
                            for (const x of props.invalidX) {
                                const cx = u.valToPos(x, 'x', true);
                                ctx.beginPath();
                                ctx.moveTo(cx - 5, base);
                                ctx.lineTo(cx + 5, base);
                                ctx.lineTo(cx, base - 10);
                                ctx.closePath();
                                ctx.fill();
                            }
                        }
                    },
                ],
            },
        },
        data.value,
        el,
    );
}

onMounted(build);
onBeforeUnmount(() => {
    plot?.destroy();
    plot = undefined;
});
watch(
    () => [
        props.series,
        props.invalidX,
        props.xLabel,
        props.yLabel,
        props.yMin,
        props.yMax,
        props.includeZero,
    ],
    build,
    { deep: true },
);
useResizeObserver(holder, () => {
    const width = holder.value?.clientWidth;
    if (plot && width) plot.setSize({ width, height: Math.round((width * 340) / 900) });
});
usePlotAppearance(build);
</script>

<template>
    <figure class="chart">
        <figcaption>{{ title }}</figcaption>
        <div ref="holder" class="chart-plot" role="img" :aria-label="summary"></div>
        <ul class="legend" :aria-label="tr('result.chartLegend', { title: title })">
            <li v-for="s in series" :key="s.label" :style="{ color: s.color }">
                {{ s.dashed ? '╌╌' : '━━' }} {{ s.label }}
            </li>
            <li v-if="invalidX.length" class="reflect-key">
                ▲ {{ invalidX.length }} {{ tr('fragments.invalidSample')
                }}{{ invalidX.length === 1 ? '' : 's' }} {{ tr('fragments.lineBroken') }}
            </li>
        </ul>
    </figure>
</template>
