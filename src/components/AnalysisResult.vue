<script setup lang="ts">
import { tr } from '../i18n';
import { computed, ref } from 'vue';
import { fmt, niceTicks, type ChartSeries } from '../chart';
import { rgbCss } from '../color';
import { describeSampling, describeSpectrum, percent } from '../sampling';
import type { AnalysisOutcome, RGBA } from '../types';
import FieldCurvaturePlot from './FieldCurvaturePlot.vue';
import HeatMap from './HeatMap.vue';
import LineChart from './LineChart.vue';
import SpectralLedger, { type LedgerRow } from './SpectralLedger.vue';

const props = defineProps<{
    outcome: AnalysisOutcome;
    /** Field the result was computed for, e.g. "2: (0, 5)°". */ fieldText?: string;
    /** Saved color of the selected wavelength entry captured at dispatch. */ color?: RGBA;
}>();

const BLUE = '#286c8d',
    ORANGE = '#c76a3d';
const o = computed(() => props.outcome);

/** Field-local basis of the analyses that sample or project in the tangential/sagittal frame. */
const frameVector = (a: [number, number]): string => `(${fmt(a[0], 4)}, ${fmt(a[1], 4)})`;
const frameText = computed(() => {
    const r = o.value;
    if (!('frame' in r.data)) return '';
    const { tangential: t, sagittal: s } = r.data.frame;
    return tr('result.frame', {
        field: props.fieldText ? `Field ${props.fieldText} · ` : '',
        t: frameVector(t),
        s: frameVector(s),
    });
});

interface Problem {
    where: string;
    at: string;
    error: string;
}

// ---- ray fan --------------------------------------------------------------
const fan = computed(() => {
    const r = o.value;
    if (r.kind !== 'ray_fan') return null;
    const build = (
        pts: typeof r.data.tangential,
    ): { series: ChartSeries[]; invalid: number[] } => ({
        series: [
            {
                get label() {
                    return tr('result.fanSeriesT');
                },
                color: BLUE,
                points: pts.map((p) => ({
                    x: p.pupil,
                    y: p.tangential_error === null ? null : p.tangential_error * 1000,
                })),
            },
            {
                get label() {
                    return tr('result.fanSeriesS');
                },
                color: ORANGE,
                dashed: true,
                points: pts.map((p) => ({
                    x: p.pupil,
                    y: p.sagittal_error === null ? null : p.sagittal_error * 1000,
                })),
            },
        ],
        invalid: pts
            .filter(
                (p) => p.error !== null || p.tangential_error === null || p.sagittal_error === null,
            )
            .map((p) => p.pupil),
    });
    return { tangential: build(r.data.tangential), sagittal: build(r.data.sagittal) };
});

// ---- OPD fan --------------------------------------------------------------
const opd = computed(() => {
    const r = o.value;
    if (r.kind !== 'opd_fan') return null;
    const build = (
        pts: typeof r.data.tangential,
    ): { series: ChartSeries[]; invalid: number[] } => ({
        series: [
            {
                get label() {
                    return tr('result.opdY');
                },
                color: BLUE,
                points: pts.map((p) => ({ x: p.pupil, y: p.opd })),
            },
        ],
        invalid: pts.filter((p) => p.opd === null || p.error !== null).map((p) => p.pupil),
    });
    return { tangential: build(r.data.tangential), sagittal: build(r.data.sagittal) };
});

// ---- chromatic focus / longitudinal ----------------------------------------
const chromatic = computed(() => {
    const r = o.value;
    if (r.kind !== 'chromatic_focus') return null;
    return {
        series: [
            {
                get label() {
                    return tr('result.focusShiftSeries');
                },
                color: BLUE,
                points: r.data.points.map((p) => ({ x: p.wavelength, y: p.shift })),
            },
        ] as ChartSeries[],
        invalid: r.data.points.filter((p) => p.shift === null).map((p) => p.wavelength),
    };
});
const longitudinal = computed(() => {
    const r = o.value;
    if (r.kind !== 'longitudinal_aberration') return null;
    return {
        series: [
            {
                get label() {
                    return tr('result.focusShiftSeries');
                },
                color: BLUE,
                points: r.data.points.map((p) => ({ x: p.pupil, y: p.shift })),
            },
        ] as ChartSeries[],
        invalid: r.data.points.filter((p) => p.shift === null).map((p) => p.pupil),
    };
});

// ---- spherochromatism ----------------------------------------------------------------
const sphero = computed(() => {
    const r = o.value;
    if (r.kind !== 'spherochromatism') return null;
    const referenceFocus = r.data.reference_focus;
    const label = (c: (typeof r.data.curves)[number]): string =>
        `${c.wavelength_index + 1}: ${c.wavelength} nm${c.weight === 0 ? tr('result.weightZero') : ''}`;
    const series: ChartSeries[] = r.data.curves.map((c) => ({
        label: label(c),
        color: rgbCss(c.color),
        dashed: c.weight === 0,
        // Offset from the common reference focus, not the curve's own shift, so longitudinal color stays visible.
        points: c.points.map((p) => ({
            x: p.pupil,
            y: p.focus === null ? null : p.focus - referenceFocus,
        })),
    }));
    const invalid = [
        ...new Set(
            r.data.curves.flatMap((c) =>
                c.points.filter((p) => p.focus === null || p.error !== null).map((p) => p.pupil),
            ),
        ),
    ];
    return { series, invalid, label };
});

// ---- field curvature -----------------------------------------------------------------------
const FAILURE_ROW_CAP = 200;
interface CurvatureFailure {
    wavelengthIndex: number;
    wavelength: number;
    field: [number, number];
    radius: number;
    branch: string;
    stage: string;
    where: string;
    message: string;
    point: string;
}
const curvature = computed(() => {
    const r = o.value;
    if (r.kind !== 'field_curvature') return null;
    const rows: CurvatureFailure[] = [];
    const stages: Record<string, number> = { chief: 0, bundle: 0, focus: 0 };
    let valid = 0,
        total = 0;
    for (const c of r.data.curves) {
        for (const p of c.points) {
            for (const branch of ['sagittal', 'tangential'] as const) {
                total++;
                const v = p[branch];
                if (v.shift !== null) valid++;
                const e = v.error;
                if (!e) continue;
                stages[e.stage] = (stages[e.stage] ?? 0) + 1;
                const where =
                    e.endpoint === 'object'
                        ? tr('result.whereObject')
                        : e.endpoint === 'image'
                          ? tr('result.whereImage')
                          : e.surface !== null
                            ? tr('result.whereSurface', { n: e.surface + 1 })
                            : e.stage === 'focus'
                              ? tr('result.whereFocus')
                              : e.stage === 'chief'
                                ? tr('result.whereChief')
                                : tr('result.whereBundle');
                rows.push({
                    wavelengthIndex: c.wavelength_index,
                    wavelength: c.wavelength,
                    field: p.field,
                    radius: p.radius,
                    branch,
                    stage: e.stage,
                    where,
                    get message() {
                        return tr('agents.connected', { name: e.kind, status: e.message });
                    },
                    point: e.point
                        ? `(${e.point.map((q) => fmt(q, 5)).join(', ')})`
                        : tr('common.dash'),
                });
            }
        }
    }
    return { rows, stages, valid, total };
});
const showFailures = ref(false);

// ---- astigmatism ------------------------------------------------------------
const astig = computed(() => {
    const r = o.value;
    if (r.kind !== 'astigmatism') return null;
    const s = r.data.sagittal_shift,
        t = r.data.tangential_shift;
    const m = Math.max(Math.abs(s), Math.abs(t), 1e-9) * 1.3;
    const ticks = niceTicks(-m, m, 6);
    const x = (v: number): number => 450 + (v / m) * 380;
    return { s, t, ticks, x };
});

// ---- distortion ---------------------------------------------------------------
const dist = computed(() => {
    const r = o.value;
    if (r.kind !== 'distortion') return null;
    const dx = (r.data.actual[0] - r.data.ideal[0]) * 1000,
        dy = (r.data.actual[1] - r.data.ideal[1]) * 1000;
    const m = Math.max(Math.abs(dx), Math.abs(dy), 1e-9) * 1.3;
    return { dx, dy, m, ticks: niceTicks(-m, m, 4) };
});

// ---- wavefront / PSF ---------------------------------------------------------------
const wave = computed(() => {
    const r = o.value;
    if (r.kind !== 'wavefront') return null;
    return { outside: r.data.pupils.map(([x, y]) => x * x + y * y > 1) };
});
const logPsf = ref(true);
const psfHalf = computed(() => {
    const r = o.value;
    return r.kind === 'diffraction_psf' || r.kind === 'huygens_psf'
        ? (r.data.grid * r.data.pixel_size * 1000) / 2
        : 0;
});

// ---- other curves -------------------------------------------------------------------
const diffMtf = computed(() => {
    const r = o.value;
    if (r.kind !== 'diffraction_mtf' && r.kind !== 'huygens_mtf') return null;
    const f = r.data.frequencies;
    return [
        {
            get label() {
                return tr('result.tangential');
            },
            color: BLUE,
            points: f.map((x, i) => ({ x, y: r.data.tangential[i] ?? null })),
        },
        {
            get label() {
                return tr('result.sagittal');
            },
            color: ORANGE,
            dashed: true,
            points: f.map((x, i) => ({ x, y: r.data.sagittal[i] ?? null })),
        },
    ] as ChartSeries[];
});
const diffRows = computed<LedgerRow[]>(() => {
    const r = o.value;
    if (r.kind !== 'diffraction_mtf' && r.kind !== 'huygens_mtf') return [];
    return r.data.components.map((c) => ({
        wavelength: c.wavelength,
        weight: c.weight,
        transmitted: c.transmitted_samples,
        sampled: c.pupil_samples,
        fraction: c.transmitted_fraction,
        effective: c.effective_weight,
        color: r.data.spectrum.kind === 'monochromatic' && props.color ? props.color : c.color,
        pixelSize: c.pixel_size,
        cutoff: c.sampling_cutoff,
    }));
});
const energy = computed(() => {
    const r = o.value;
    if (r.kind !== 'encircled_energy') return null;
    return [
        {
            get label() {
                return tr('result.energyY');
            },
            color: BLUE,
            points: r.data.points.map((p) => ({ x: p.radius * 1000, y: p.fraction })),
        },
    ] as ChartSeries[];
});

// ---- explicit error list for masked samples ---------------------------------------------
const problems = computed<Problem[]>(() => {
    const r = o.value;
    if (r.kind === 'ray_fan')
        return (['tangential', 'sagittal'] as const).flatMap((axis) =>
            r.data[axis]
                .filter((p) => p.error)
                .map((p) => ({
                    where: axis,
                    at: `${axis === 'tangential' ? 'p_T' : 'p_S'} ${fmt(p.pupil)}`,
                    error: p.error!,
                })),
        );
    if (r.kind === 'opd_fan')
        return (['tangential', 'sagittal'] as const).flatMap((axis) =>
            r.data[axis]
                .filter((p) => p.error)
                .map((p) => ({
                    where: axis,
                    at: `${axis === 'tangential' ? 'p_T' : 'p_S'} ${fmt(p.pupil)}`,
                    error: p.error!,
                })),
        );
    if (r.kind === 'spherochromatism' && sphero.value)
        return r.data.curves.flatMap((c) =>
            c.points
                .filter((p) => p.error)
                .map((p) => ({ where: sphero.value!.label(c), at: fmt(p.pupil), error: p.error! })),
        );
    if (r.kind === 'chromatic_focus')
        return r.data.points
            .filter((p) => p.error)
            .map((p) => ({
                where: 'wavelength',
                get at() {
                    return tr('sampling.spectrumMono', { wavelength: fmt(p.wavelength) });
                },
                error: p.error!,
            }));
    if (r.kind === 'longitudinal_aberration')
        return r.data.points
            .filter((p) => p.error)
            .map((p) => ({ where: 'pupil', at: fmt(p.pupil), error: p.error! }));
    return [];
});

/** Raw JSON supplement; long numeric arrays are summarized to keep the page responsive. */
const showRaw = ref(false);
const raw = computed(() =>
    showRaw.value
        ? JSON.stringify(
              o.value.data,
              (_k, v: unknown) =>
                  Array.isArray(v) && v.length > 256 ? `[${v.length} entries]` : v,
              1,
          )
        : '',
);
</script>

<template>
    <div class="result">
        <!-- ray fan -->
        <template v-if="fan && o.kind === 'ray_fan'">
            <p class="note">{{ frameText }}</p>
            <LineChart
                :title="tr('result.fanTangential')"
                :series="fan.tangential.series"
                :x-label="tr('result.fanXT')"
                :y-label="tr('result.fanY')"
                include-zero
                :invalid-x="fan.tangential.invalid"
            />
            <LineChart
                :title="tr('result.fanSagittal')"
                :series="fan.sagittal.series"
                :x-label="tr('result.fanXS')"
                :y-label="tr('result.fanY')"
                include-zero
                :invalid-x="fan.sagittal.invalid"
            />
            <p class="note">
                {{
                    tr(
                        'fragments.errorsAreProjectedOnTheTangentialTAndSagittalAxesRelativeToTheChiefRayImageIntercept',
                    )
                }}{{ fmt(o.data.chief_image[0]) }}, {{ fmt(o.data.chief_image[1])
                }}{{ tr('fragments.mmAt') }} {{ o.data.wavelength }} nm.
            </p>
        </template>

        <!-- OPD fan -->
        <template v-else-if="opd && o.kind === 'opd_fan'">
            <p class="note">{{ frameText }}</p>
            <LineChart
                :title="tr('result.opdTangential')"
                :series="opd.tangential.series"
                :x-label="tr('result.fanXT')"
                :y-label="tr('result.opdY')"
                include-zero
                :invalid-x="opd.tangential.invalid"
            />
            <LineChart
                :title="tr('result.opdSagittal')"
                :series="opd.sagittal.series"
                :x-label="tr('result.fanXS')"
                :y-label="tr('result.opdY')"
                include-zero
                :invalid-x="opd.sagittal.invalid"
            />
            <p class="note">
                {{ tr('fragments.reference') }} {{ o.data.reference
                }}{{ tr('fragments.wavelength') }} {{ o.data.wavelength }} nm.
            </p>
        </template>

        <!-- chromatic focus -->
        <template v-else-if="chromatic && o.kind === 'chromatic_focus'">
            <LineChart
                :title="tr('result.chromaticTitle')"
                :series="chromatic.series"
                :x-label="tr('result.chromaticX')"
                :y-label="tr('result.chromaticY')"
                include-zero
                :invalid-x="chromatic.invalid"
            />
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.referenceWavelength') }}</dt>
                    <dd>{{ o.data.reference_wavelength }} nm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.referenceFocus') }}</dt>
                    <dd>{{ fmt(o.data.reference_focus, 6) }} mm</dd>
                </div>
            </dl>
        </template>

        <!-- longitudinal aberration -->
        <template v-else-if="longitudinal && o.kind === 'longitudinal_aberration'">
            <LineChart
                :title="tr('result.longitudinalTitle')"
                :series="longitudinal.series"
                :x-label="tr('result.pupilHeightX')"
                :y-label="tr('result.longitudinalY')"
                include-zero
                :invalid-x="longitudinal.invalid"
            />
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.paraxialFocus') }}</dt>
                    <dd>{{ fmt(o.data.paraxial_focus, 6) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.wavelengthDt') }}</dt>
                    <dd>{{ o.data.wavelength }} nm</dd>
                </div>
            </dl>
        </template>

        <!-- spherochromatism -->
        <template v-else-if="sphero && o.kind === 'spherochromatism'">
            <LineChart
                :title="tr('result.spheroTitle')"
                :series="sphero.series"
                :x-label="tr('result.pupilHeightX')"
                :y-label="tr('result.spheroY')"
                include-zero
                :invalid-x="sphero.invalid"
            />
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.referenceWavelength') }}</dt>
                    <dd>{{ o.data.reference_wavelength }} nm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.referenceFocus') }}</dt>
                    <dd>{{ fmt(o.data.reference_focus, 6) }} mm</dd>
                </div>
            </dl>
            <table class="wide-table ledger">
                <caption>
                    {{
                        tr('result.spheroCaption')
                    }}
                </caption>
                <thead>
                    <tr>
                        <th scope="col">#</th>
                        <th scope="col">{{ tr('result.spheroColLambda') }}</th>
                        <th scope="col">{{ tr('result.spheroColWeight') }}</th>
                        <th scope="col">{{ tr('result.spheroColFocus') }}</th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="c in o.data.curves" :key="c.wavelength_index">
                        <td>
                            <svg width="10" height="10" aria-hidden="true">
                                <circle cx="5" cy="5" r="4" :fill="rgbCss(c.color)" />
                            </svg>
                            {{ c.wavelength_index + 1 }}
                        </td>
                        <td>{{ c.wavelength }}</td>
                        <td>{{ fmt(c.weight, 4) }}</td>
                        <td>{{ fmt(c.paraxial_focus - o.data.reference_focus, 4) }}</td>
                    </tr>
                </tbody>
            </table>
            <p class="note">
                {{ tr('result.spheroNote') }}
            </p>
        </template>

        <!-- field curvature -->
        <template v-else-if="curvature && o.kind === 'field_curvature'">
            <FieldCurvaturePlot :data="o.data" />
            <dl class="kv">
                <div>
                    <dt>{{ tr('fragments.sweepEnd') }}{{ o.data.field_units }})</dt>
                    <dd>({{ fmt(o.data.field_end[0], 5) }}, {{ fmt(o.data.field_end[1], 5) }})</dd>
                </div>
                <div>
                    <dt>{{ tr('result.curvatureLargestField') }}</dt>
                    <dd>{{ fmt(o.data.max_field, 5) }} {{ o.data.field_units }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.curvatureSamples') }}</dt>
                    <dd>{{ o.data.samples }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.curvatureImageVertex') }}</dt>
                    <dd>{{ fmt(o.data.image_vertex, 6) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.curvatureCurves') }}</dt>
                    <dd>{{ o.data.curves.length }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.curvatureValid') }}</dt>
                    <dd>{{ curvature.valid }} of {{ curvature.total }}</dd>
                </div>
            </dl>
            <p class="note">{{ o.data.model }}</p>
            <details
                class="data-table wide"
                :open="curvature.rows.length > 0 && curvature.valid === 0"
                @toggle="showFailures = ($event.target as HTMLDetailsElement).open"
            >
                <summary>
                    {{ tr('fragments.fieldCurvatureFailures') }}{{ curvature.rows.length }}:
                    {{ curvature.stages.chief }} {{ tr('fragments.chiefRay') }}
                    {{ curvature.stages.bundle }} {{ tr('fragments.bundle') }}
                    {{ curvature.stages.focus }} {{ tr('fragments.focus') }}
                </summary>
                <p v-if="curvature.rows.length === 0" class="note">{{ tr('result.noFailures') }}</p>
                <template v-else-if="showFailures || curvature.valid === 0">
                    <p v-if="curvature.rows.length > FAILURE_ROW_CAP" class="note">
                        {{ tr('fragments.showingTheFirst') }} {{ FAILURE_ROW_CAP }} of
                        {{ curvature.rows.length }}
                        {{ tr('fragments.failuresTheRawResponseListsAllOfThem') }}
                    </p>
                    <div
                        class="table-scroll"
                        role="region"
                        :aria-label="tr('result.failuresRegion')"
                        tabindex="0"
                    >
                        <table class="wide-table">
                            <caption>
                                {{
                                    tr('result.failuresRegion')
                                }}
                            </caption>
                            <thead>
                                <tr>
                                    <th scope="col">{{ tr('result.wavelengthDt') }}</th>
                                    <th scope="col">
                                        {{ tr('fragments.fieldValue') }}{{ o.data.field_units }})
                                    </th>
                                    <th scope="col">{{ tr('result.colRadius') }}</th>
                                    <th scope="col">{{ tr('result.colBranch') }}</th>
                                    <th scope="col">{{ tr('result.colStage') }}</th>
                                    <th scope="col">{{ tr('result.colWhere') }}</th>
                                    <th scope="col">{{ tr('result.colMessage') }}</th>
                                    <th scope="col">{{ tr('result.colPoint') }}</th>
                                </tr>
                            </thead>
                            <tbody>
                                <tr
                                    v-for="(f, i) in curvature.rows.slice(0, FAILURE_ROW_CAP)"
                                    :key="i"
                                >
                                    <td>{{ f.wavelengthIndex + 1 }}: {{ f.wavelength }} nm</td>
                                    <td>({{ fmt(f.field[0], 4) }}, {{ fmt(f.field[1], 4) }})</td>
                                    <td>{{ fmt(f.radius, 4) }}</td>
                                    <td>{{ f.branch }}</td>
                                    <td>{{ f.stage }}</td>
                                    <td>{{ f.where }}</td>
                                    <td>{{ f.message }}</td>
                                    <td>{{ f.point }}</td>
                                </tr>
                            </tbody>
                        </table>
                    </div>
                </template>
            </details>
        </template>

        <!-- astigmatism -->
        <template v-else-if="astig && o.kind === 'astigmatism'">
            <p class="note">{{ frameText }}</p>
            <figure class="chart">
                <figcaption>{{ tr('result.astigCaption') }}</figcaption>
                <svg viewBox="0 0 900 150" role="img" :aria-label="tr('result.astigAria')">
                    <title>
                        {{ tr('fragments.sagittalFocusShift') }} {{ fmt(astig.s) }}
                        {{ tr('fragments.mmTangentialFocusShift') }} {{ fmt(astig.t) }} mm
                    </title>
                    <line x1="70" x2="830" y1="90" y2="90" stroke="var(--plot-ink)" />
                    <g font-size="11" fill="var(--plot-ink)">
                        <template v-for="t in astig.ticks" :key="t">
                            <line
                                :x1="astig.x(t)"
                                :x2="astig.x(t)"
                                y1="85"
                                y2="95"
                                stroke="var(--plot-ink)"
                            />
                            <text :x="astig.x(t)" y="112" text-anchor="middle">{{ fmt(t) }}</text>
                        </template>
                        <text x="450" y="140" text-anchor="middle">
                            {{ tr('result.astigAxis') }}
                        </text>
                    </g>
                    <line
                        :x1="astig.x(0)"
                        :x2="astig.x(0)"
                        y1="20"
                        y2="90"
                        stroke="#75989c"
                        stroke-width="2"
                    />
                    <circle :cx="astig.x(astig.s)" cy="60" r="7" :fill="BLUE" />
                    <text
                        :x="astig.x(astig.s)"
                        y="45"
                        text-anchor="middle"
                        font-size="12"
                        :fill="BLUE"
                    >
                        {{ tr('result.sagittal') }}
                    </text>
                    <circle :cx="astig.x(astig.t)" cy="75" r="7" :fill="ORANGE" />
                    <text
                        :x="astig.x(astig.t)"
                        y="30"
                        text-anchor="middle"
                        font-size="12"
                        :fill="ORANGE"
                    >
                        {{ tr('result.tangential') }}
                    </text>
                </svg>
            </figure>
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.sagittalFocus') }}</dt>
                    <dd>{{ fmt(o.data.sagittal_focus, 6) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.tangentialFocus') }}</dt>
                    <dd>{{ fmt(o.data.tangential_focus, 6) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.sagittalShift') }}</dt>
                    <dd>{{ fmt(o.data.sagittal_shift) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.tangentialShift') }}</dt>
                    <dd>{{ fmt(o.data.tangential_shift) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.astigmatism') }}</dt>
                    <dd>{{ fmt(o.data.astigmatism) }} mm</dd>
                </div>
            </dl>
        </template>

        <!-- distortion -->
        <template v-else-if="dist && o.kind === 'distortion'">
            <figure class="chart">
                <figcaption>{{ tr('result.distortionCaption') }}</figcaption>
                <svg viewBox="0 0 360 300" role="img" :aria-label="tr('result.distortionAria')">
                    <title>
                        {{ tr('fragments.actualMinusIdealImagePoint') }} {{ fmt(dist.dx) }},
                        {{ fmt(dist.dy) }} µm
                    </title>
                    <line x1="180" x2="180" y1="20" y2="270" stroke="#ccd9df" />
                    <line x1="30" x2="330" y1="145" y2="145" stroke="#ccd9df" />
                    <circle
                        cx="180"
                        cy="145"
                        r="5"
                        fill="none"
                        stroke="var(--plot-ink)"
                        stroke-width="1.5"
                    />
                    <line
                        x1="180"
                        y1="145"
                        :x2="180 + (dist.dx / dist.m) * 140"
                        :y2="145 - (dist.dy / dist.m) * 120"
                        :stroke="ORANGE"
                        stroke-width="2"
                    />
                    <circle
                        :cx="180 + (dist.dx / dist.m) * 140"
                        :cy="145 - (dist.dy / dist.m) * 120"
                        r="5"
                        :fill="ORANGE"
                    />
                    <text x="334" y="140" font-size="11" fill="var(--plot-ink)" text-anchor="end">
                        x ±{{ fmt(dist.m) }} µm
                    </text>
                    <text x="186" y="30" font-size="11" fill="var(--plot-ink)">
                        y ±{{ fmt(dist.m) }} µm
                    </text>
                </svg>
            </figure>
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.distortion') }}</dt>
                    <dd>
                        {{
                            o.data.distortion_percent === null
                                ? tr('result.distortionUndefined')
                                : fmt(o.data.distortion_percent) + ' %'
                        }}
                    </dd>
                </div>
                <div>
                    <dt>{{ tr('result.idealPoint') }}</dt>
                    <dd>{{ fmt(o.data.ideal[0], 5) }}, {{ fmt(o.data.ideal[1], 5) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.actualPoint') }}</dt>
                    <dd>{{ fmt(o.data.actual[0], 5) }}, {{ fmt(o.data.actual[1], 5) }} mm</dd>
                </div>
            </dl>
            <p class="note">
                {{ tr('fragments.reference') }} {{ o.data.reference
                }}{{ tr('fragments.imagePlaneXYAreGlobal') }}
            </p>
        </template>

        <!-- wavefront -->
        <template v-else-if="wave && o.kind === 'wavefront'">
            <HeatMap
                :title="tr('result.wavefrontTitle')"
                :values="o.data.opd"
                :size="o.data.grid"
                :outside="wave.outside"
                diverging
                unit="waves"
            />
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.rms') }}</dt>
                    <dd>{{ fmt(o.data.rms) }} {{ tr('result.waves') }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.peakToValley') }}</dt>
                    <dd>{{ fmt(o.data.peak_to_valley) }} {{ tr('result.waves') }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.mean') }}</dt>
                    <dd>{{ fmt(o.data.mean) }} {{ tr('result.waves') }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.validFailed') }}</dt>
                    <dd>{{ o.data.valid }} / {{ o.data.failed }}</dd>
                </div>
            </dl>
            <p class="note">
                {{ tr('fragments.reference') }} {{ o.data.reference
                }}{{ tr('fragments.rowsArePupilYUpColumnsPupilX') }}
            </p>
        </template>

        <!-- PSF -->
        <template v-else-if="o.kind === 'diffraction_psf' || o.kind === 'huygens_psf'">
            <label class="inline"
                ><input v-model="logPsf" type="checkbox" /> {{ tr('result.logScale') }}</label
            >
            <HeatMap
                :title="tr('result.psfTitle')"
                :values="o.data.intensity"
                :size="o.data.grid"
                :log="logPsf"
                unit="normalized intensity"
            />
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.strehl') }}</dt>
                    <dd>{{ fmt(o.data.strehl, 4) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.pixelSize') }}</dt>
                    <dd>{{ fmt(o.data.pixel_size * 1000) }} µm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.extent') }}</dt>
                    <dd>±{{ fmt(psfHalf) }} µm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.failedPupilRays') }}</dt>
                    <dd>{{ o.data.failed_pupil_rays }}</dd>
                </div>
            </dl>
            <p class="note">
                {{ o.data.model }}; {{ o.data.normalization }}{{ tr('fragments.imageSurface') }}
                {{ o.data.image_sampling }}.
            </p>
        </template>

        <!-- diffraction MTF -->
        <template v-else-if="diffMtf && (o.kind === 'diffraction_mtf' || o.kind === 'huygens_mtf')">
            <p class="note">{{ frameText }}</p>
            <LineChart
                :title="tr('result.diffMtfTitle')"
                :series="diffMtf"
                :x-label="tr('mtf.xLabel')"
                :y-label="tr('mtf.yLabel')"
                :y-min="0"
                :y-max="1"
            />
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.spectrum') }}</dt>
                    <dd>{{ describeSpectrum(o.data.spectrum) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.maxFrequency') }}</dt>
                    <dd>{{ fmt(o.data.max_frequency, 4) }} cycles/mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.samplingLimit') }}</dt>
                    <dd>{{ fmt(o.data.sampling_limit, 4) }} cycles/mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.fftGrid') }}</dt>
                    <dd>{{ o.data.pupil_grid }}×{{ o.data.pupil_grid }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.transmittedWeight') }}</dt>
                    <dd>{{ fmt(o.data.transmitted_weight, 4) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.failedPupilRays') }}</dt>
                    <dd>{{ o.data.failed_pupil_rays }}</dd>
                </div>
            </dl>
            <SpectralLedger :rows="diffRows" :caption="tr('result.diffLedgerCaption')" />
            <p class="note">
                {{ o.data.model }}{{ tr('fragments.imageSurface') }} {{ o.data.image_sampling
                }}{{
                    tr(
                        'fragments.cutsFollowTheFieldLocalTangentialAndSagittalDirectionsOtfsAreEvaluatedAtCommonPhysica',
                    )
                }}
                {{ fmt(o.data.origin[0], 5) }}, {{ fmt(o.data.origin[1], 5) }}
                {{ tr('fragments.mmGlobalXYAndCombinedAsComplexSumsWithEffectiveSpectralWeights') }}
                {{ o.data.normalization }}.
            </p>
        </template>

        <!-- encircled energy -->
        <template v-else-if="energy && o.kind === 'encircled_energy'">
            <LineChart
                :title="tr('result.energyTitle')"
                :series="energy"
                :x-label="tr('result.energyX')"
                :y-label="tr('result.energyY')"
                :y-min="0"
                :y-max="1"
            />
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.energySampling') }}</dt>
                    <dd>{{ describeSampling(o.data.sampling) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.energyRays') }}</dt>
                    <dd>{{ o.data.rays }} / {{ o.data.sampled_rays }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.energyWeight') }}</dt>
                    <dd>{{ percent(o.data.transmitted_weight) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.energyCentre') }}</dt>
                    <dd>{{ fmt(o.data.center[0], 5) }}, {{ fmt(o.data.center[1], 5) }} mm</dd>
                </div>
            </dl>
            <p class="note">
                {{ tr('fragments.fractionIsTheCumulativeTransmittedPupilAreaWeightNormalizedTo') }}
                {{ o.data.normalization }}.
            </p>
        </template>

        <!-- Zernike decomposition -->
        <template v-else-if="o.kind === 'wavefront_zernike'">
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.zernikeResidual') }}</dt>
                    <dd>{{ fmt(o.data.fit_residual_rms) }} {{ tr('result.waves') }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.validFailed') }}</dt>
                    <dd>{{ o.data.valid }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.zernikeOrdering') }}</dt>
                    <dd>{{ o.data.ordering }}</dd>
                </div>
            </dl>
            <table class="wide-table">
                <caption class="sr-only">
                    {{
                        tr('result.zernikeCaption')
                    }}
                </caption>
                <thead>
                    <tr>
                        <th scope="col">j</th>
                        <th scope="col">{{ tr('result.zernikeName') }}</th>
                        <th scope="col">n</th>
                        <th scope="col">m</th>
                        <th scope="col">{{ tr('result.zernikeCoefficient') }}</th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="t in o.data.terms" :key="t.j">
                        <td>{{ t.j }}</td>
                        <td>{{ t.name }}</td>
                        <td>{{ t.n }}</td>
                        <td>{{ t.m }}</td>
                        <td>{{ fmt(t.coefficient) }}</td>
                    </tr>
                </tbody>
            </table>
            <p class="note">{{ o.data.normalization }}.</p>
        </template>

        <!-- Gaussian beam -->
        <template v-else-if="o.kind === 'gaussian_beam'">
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.gaussImageRadius') }}</dt>
                    <dd>{{ fmt(o.data.image.radius * 1000) }} µm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.gaussWaist') }}</dt>
                    <dd>{{ fmt(o.data.image.waist * 1000) }} µm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.gaussWaistOffset') }}</dt>
                    <dd>{{ fmt(o.data.image.waist_offset) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.gaussRayleigh') }}</dt>
                    <dd>{{ fmt(o.data.image.rayleigh_range) }} mm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.gaussDivergence') }}</dt>
                    <dd>{{ fmt(o.data.image.divergence * 1000) }} mrad</dd>
                </div>
                <div>
                    <dt>{{ tr('result.gaussCurvature') }}</dt>
                    <dd>
                        {{
                            o.data.image.curvature === null
                                ? tr('result.gaussFlat')
                                : fmt(o.data.image.curvature) + ' mm'
                        }}
                    </dd>
                </div>
            </dl>
            <table class="wide-table">
                <caption class="sr-only">
                    {{
                        tr('result.gaussCaption')
                    }}
                </caption>
                <thead>
                    <tr>
                        <th scope="col">{{ tr('result.gaussSurface') }}</th>
                        <th scope="col">z (mm)</th>
                        <th scope="col">{{ tr('result.gaussRadius') }}</th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="row in o.data.surfaces" :key="row.surface">
                        <td>{{ row.surface + 1 }}</td>
                        <td>{{ fmt(row.z) }}</td>
                        <td>{{ fmt(row.radius) }}</td>
                    </tr>
                </tbody>
            </table>
            <p class="note">{{ o.data.model }}.</p>
        </template>

        <!-- fiber coupling -->
        <template v-else-if="o.kind === 'fiber_coupling'">
            <dl class="kv">
                <div>
                    <dt>{{ tr('result.fiberEfficiency') }}</dt>
                    <dd>{{ percent(o.data.efficiency) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('result.fiberMfd') }}</dt>
                    <dd>{{ fmt(o.data.mode_field_diameter * 1000) }} µm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.gaussImageRadius') }}</dt>
                    <dd>{{ fmt(o.data.beam_radius * 1000) }} µm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.gaussCurvature') }}</dt>
                    <dd>
                        {{
                            o.data.wavefront_curvature === null
                                ? tr('result.gaussFlat')
                                : fmt(o.data.wavefront_curvature) + ' mm'
                        }}
                    </dd>
                </div>
                <div>
                    <dt>{{ tr('result.fiberOptimalMfd') }}</dt>
                    <dd>{{ fmt(o.data.optimal_mode_field_diameter * 1000) }} µm</dd>
                </div>
                <div>
                    <dt>{{ tr('result.fiberOptimalEfficiency') }}</dt>
                    <dd>{{ percent(o.data.optimal_efficiency) }}</dd>
                </div>
            </dl>
            <p class="note">{{ o.data.model }}.</p>
        </template>

        <div v-if="problems.length" role="status" class="banner warn problem-list">
            <strong
                >{{ problems.length }} {{ tr('fragments.invalidSample')
                }}{{ problems.length === 1 ? '' : 's' }}</strong
            >
            <table class="wide-table">
                <caption class="sr-only">
                    {{
                        tr('result.invalidCaption')
                    }}
                </caption>
                <thead>
                    <tr>
                        <th scope="col">{{ tr('result.colSet') }}</th>
                        <th scope="col">{{ tr('result.colSample') }}</th>
                        <th scope="col">{{ tr('result.colError') }}</th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="(p, i) in problems" :key="i">
                        <td>{{ p.where }}</td>
                        <td>{{ p.at }}</td>
                        <td>{{ p.error }}</td>
                    </tr>
                </tbody>
            </table>
        </div>

        <details
            class="data-table wide"
            @toggle="showRaw = ($event.target as HTMLDetailsElement).open"
        >
            <summary>{{ tr('common.rawJson') }}</summary>
            <pre v-if="showRaw" class="raw">{{ raw }}</pre>
        </details>
    </div>
</template>
