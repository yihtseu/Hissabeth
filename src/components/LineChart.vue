<script setup lang="ts">
import { tr } from '../i18n';
import { usePreferencesStore } from '../stores/preferences';
const preferences = usePreferencesStore();
import { computed, useId } from 'vue';
import { extent, fmt, niceTicks, type ChartSeries } from '../chart';

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

const uid = useId();
const L = 78,
    R = 870,
    T = 18,
    B = 288;

const allPoints = computed(() => props.series.flatMap((s) => s.points));
const xr = computed<[number, number]>(() => {
    const e = extent([...allPoints.value.map((p) => p.x), ...props.invalidX]) ?? [0, 1];
    return e[0] === e[1] ? [e[0] - 1, e[1] + 1] : e;
});
const yr = computed<[number, number]>(() => {
    const e = extent(allPoints.value.map((p) => p.y)) ?? [0, 1];
    let [lo, hi] = e;
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
    const pad = props.yMin === undefined || props.yMax === undefined ? (hi - lo) * 0.05 : 0;
    return [props.yMin !== undefined ? lo : lo - pad, props.yMax !== undefined ? hi : hi + pad];
});
const xTicks = computed(() => niceTicks(xr.value[0], xr.value[1], 6));
const yTicks = computed(() => niceTicks(yr.value[0], yr.value[1], 5));
const px = (x: number): number => L + ((x - xr.value[0]) / (xr.value[1] - xr.value[0])) * (R - L);
const py = (y: number): number => B - ((y - yr.value[0]) / (yr.value[1] - yr.value[0])) * (B - T);

/** One polyline per run of valid samples. */
function runs(s: ChartSeries): string[] {
    const out: string[] = [];
    let cur: string[] = [];
    for (const p of s.points) {
        if (p.y === null || !Number.isFinite(p.y)) {
            if (cur.length) out.push(cur.join(' '));
            cur = [];
        } else cur.push(`${px(p.x)},${py(p.y)}`);
    }
    if (cur.length) out.push(cur.join(' '));
    return out;
}
const lines = computed(() => props.series.map((s) => ({ s, runs: runs(s) })));
const summary = computed(() => {
    const valid = allPoints.value.filter((p) => p.y !== null).length;
    return tr('result.chartSummary', {
        title: props.title,
        series: props.series.map((s) => s.label).join(', '),
        valid: valid,
        invalid: props.invalidX.length,
    });
});
</script>

<template>
    <figure class="chart">
        <figcaption>{{ title }}</figcaption>
        <svg viewBox="0 0 900 340" role="img" :aria-labelledby="`${uid}-t`">
            <title :id="`${uid}-t`">{{ summary }}</title>
            <g
                v-if="preferences.preferences.display.plotGrid"
                class="plot-grid"
                stroke="var(--grid)"
            >
                <line v-for="t in yTicks" :key="`gy${t}`" :x1="L" :x2="R" :y1="py(t)" :y2="py(t)" />
                <line v-for="t in xTicks" :key="`gx${t}`" :x1="px(t)" :x2="px(t)" :y1="T" :y2="B" />
            </g>
            <line
                v-if="yr[0] < 0 && yr[1] > 0"
                :x1="L"
                :x2="R"
                :y1="py(0)"
                :y2="py(0)"
                stroke="var(--plot-ink)"
            />
            <line :x1="L" :x2="R" :y1="B" :y2="B" stroke="var(--plot-ink)" />
            <line :x1="L" :x2="L" :y1="T" :y2="B" stroke="var(--plot-ink)" />
            <g font-size="11" fill="var(--plot-ink)">
                <text
                    v-for="t in yTicks"
                    :key="`ty${t}`"
                    :x="L - 8"
                    :y="py(t) + 4"
                    text-anchor="end"
                >
                    {{ fmt(t) }}
                </text>
                <text
                    v-for="t in xTicks"
                    :key="`tx${t}`"
                    :x="px(t)"
                    :y="B + 16"
                    text-anchor="middle"
                >
                    {{ fmt(t) }}
                </text>
                <text :x="(L + R) / 2" :y="B + 36" text-anchor="middle">{{ xLabel }}</text>
                <text
                    x="14"
                    :y="(T + B) / 2"
                    text-anchor="middle"
                    :transform="`rotate(-90 14 ${(T + B) / 2})`"
                >
                    {{ yLabel }}
                </text>
            </g>
            <template v-for="(l, i) in lines" :key="i">
                <polyline
                    v-for="(r, k) in l.runs"
                    class="data-mark"
                    :key="k"
                    :points="r"
                    fill="none"
                    :stroke="l.s.color"
                    stroke-width="2.2"
                    :stroke-dasharray="l.s.dashed ? '6 4' : undefined"
                />
                <template v-for="(p, k) in l.s.points" :key="`p${k}`">
                    <circle
                        v-if="p.y !== null && l.runs.length > 0 && l.s.points.length <= 64"
                        :cx="px(p.x)"
                        :cy="py(p.y)"
                        r="2"
                        :fill="l.s.color"
                    />
                </template>
            </template>
            <g fill="var(--danger)">
                <path
                    v-for="(x, i) in invalidX"
                    :key="`inv${i}`"
                    :d="`M ${px(x) - 5} ${B - 1} L ${px(x) + 5} ${B - 1} L ${px(x)} ${B - 11} Z`"
                />
            </g>
        </svg>
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
