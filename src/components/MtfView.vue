<script setup lang="ts">
import { tr } from '../i18n';
import { computed } from 'vue';
import { fmt } from '../chart';
import {
    describeMapping,
    describeSampling,
    describeSpectrum,
    type SpectralMode,
} from '../sampling';
import type { GeometricMtf, MtfAxis, OpticalSystem, PupilSampling, RunInfo } from '../types';
import { fieldLabel } from '../validation';
import RaySamplingControl from './RaySamplingControl.vue';
import SpectralLedger, { type LedgerRow } from './SpectralLedger.vue';

const props = defineProps<{
    system: OpticalSystem;
    mtf: GeometricMtf | null;
    /** Provenance of `mtf`, captured when its request started. */
    run: RunInfo | null;
    pending: boolean;
    sampling: PupilSampling;
    spectralMode: SpectralMode;
    fieldIndex: number;
    wavelengthIndex: number;
    axis: MtfAxis;
    maxFrequency: number;
}>();
const emit = defineEmits<{
    'update:fieldIndex': [value: number];
    'update:wavelengthIndex': [value: number];
    'update:axis': [value: MtfAxis];
    'update:maxFrequency': [value: number];
    'update:sampling': [value: PupilSampling];
    'update:spectralMode': [value: SpectralMode];
}>();

const L = 70,
    R = 870,
    T = 25,
    B = 305;
const maxF = computed(() => Math.max(1e-9, ...(props.mtf?.frequencies ?? [props.maxFrequency])));
const x = (f: number): number => L + (f / maxF.value) * (R - L);
const y = (m: number): number => B - Math.min(1, Math.max(0, m)) * (B - T);
const points = computed(() =>
    props.mtf
        ? props.mtf.frequencies.map((f, i) => `${x(f)},${y(props.mtf?.mtf[i] ?? 0)}`).join(' ')
        : '',
);
const yTicks = [0, 0.25, 0.5, 0.75, 1];
const xTicks = computed(() => [0, 0.25, 0.5, 0.75, 1].map((t) => t * maxF.value));

function onAxis(e: Event): void {
    const v = (e.target as HTMLSelectElement).value;
    if (v === 'x' || v === 'y') emit('update:axis', v);
}
function onMode(e: Event): void {
    const v = (e.target as HTMLSelectElement).value;
    if (v === 'mono' || v === 'poly') emit('update:spectralMode', v);
}
const rows = computed<LedgerRow[]>(() =>
    (props.mtf?.components ?? []).map((c) => ({
        wavelength: c.wavelength,
        weight: c.weight,
        transmitted: c.transmitted_samples,
        sampled: c.pupil_samples,
        fraction: c.transmitted_fraction,
        effective: c.effective_weight,
        // A monochromatic reply may use a generated display color; the selected saved entry's color was captured at dispatch.
        color:
            props.mtf?.spectrum.kind === 'monochromatic' && props.run ? props.run.color : c.color,
    })),
);
function onMax(e: Event): void {
    const input = e.target as HTMLInputElement;
    const n = Number(input.value);
    if (input.value.trim() !== '' && Number.isFinite(n) && n > 0 && n <= 10000)
        emit('update:maxFrequency', n);
    else input.value = String(props.maxFrequency); // invalid entries never reach a request
}
</script>

<template>
    <section class="panel" aria-labelledby="mtf-title">
        <div class="panel-head">
            <h3 id="mtf-title">{{ tr('mtf.title') }}</h3>
            <div class="controls">
                <label
                    >{{ tr('mtf.field') }}
                    <select
                        :value="fieldIndex"
                        @change="
                            emit(
                                'update:fieldIndex',
                                Number(($event.target as HTMLSelectElement).value),
                            )
                        "
                    >
                        <option v-for="(_, i) in system.fields" :key="i" :value="i">
                            {{ fieldLabel(system, i) }}
                        </option>
                    </select>
                </label>
                <label
                    >{{ tr('mtf.spectralMode') }}
                    <select :value="spectralMode" @change="onMode">
                        <option value="mono">{{ tr('mtf.mono') }}</option>
                        <option value="poly">{{ tr('mtf.poly') }}</option>
                    </select>
                </label>
                <label v-if="spectralMode === 'mono'"
                    >{{ tr('mtf.wavelength') }}
                    <select
                        :value="wavelengthIndex"
                        @change="
                            emit(
                                'update:wavelengthIndex',
                                Number(($event.target as HTMLSelectElement).value),
                            )
                        "
                    >
                        <option v-for="(w, i) in system.wavelengths" :key="i" :value="i">
                            {{ i + 1 }}: {{ w.value }} nm
                        </option>
                    </select>
                </label>
                <label :title="tr('mtf.axisTip')"
                    >{{ tr('mtf.axis') }}
                    <select :value="axis" @change="onAxis">
                        <option value="y">{{ tr('mtf.axisY') }}</option>
                        <option value="x">{{ tr('mtf.axisX') }}</option>
                    </select>
                </label>
                <label :title="tr('mtf.maxFreqTip')"
                    >{{ tr('mtf.maxFreq') }}
                    <input
                        type="number"
                        max="10000"
                        step="any"
                        :value="maxFrequency"
                        @change="onMax"
                    />
                </label>
                <RaySamplingControl
                    :model-value="sampling"
                    @update:model-value="emit('update:sampling', $event)"
                />
            </div>
        </div>
        <svg v-if="mtf" viewBox="0 0 900 360" role="img" aria-labelledby="mtf-svg-title">
            <title id="mtf-svg-title">
                {{ tr('fragments.geometricMtfAlongGlobal') }} {{ mtf.axis }},
                {{ describeSpectrum(mtf.spectrum) }}, {{ mtf.frequencies.length }}
                {{ tr('fragments.samplesModulation') }} {{ (mtf.mtf[0] ?? 0).toFixed(2) }}
                {{ tr('fragments.fallingTo') }}
                {{ (mtf.mtf[mtf.mtf.length - 1] ?? 0).toFixed(2) }}
            </title>
            <g stroke="var(--border)">
                <line v-for="t in yTicks" :key="`gy${t}`" :x1="L" :x2="R" :y1="y(t)" :y2="y(t)" />
            </g>
            <line :x1="L" :x2="R" :y1="B" :y2="B" stroke="var(--plot-ink)" />
            <line :x1="L" :x2="L" :y1="T" :y2="B" stroke="var(--plot-ink)" />
            <g font-size="11" fill="var(--plot-ink)">
                <text
                    v-for="t in yTicks"
                    :key="`ty${t}`"
                    :x="L - 8"
                    :y="y(t) + 4"
                    text-anchor="end"
                >
                    {{ t.toFixed(2) }}
                </text>
                <text
                    v-for="(f, i) in xTicks"
                    :key="`tx${i}`"
                    :x="x(f)"
                    :y="B + 18"
                    text-anchor="middle"
                >
                    {{ fmt(f) }}
                </text>
                <text :x="(L + R) / 2" :y="B + 38" text-anchor="middle">
                    {{ tr('mtf.xLabel') }}
                </text>
                <text
                    :x="14"
                    :y="(T + B) / 2"
                    text-anchor="middle"
                    :transform="`rotate(-90 14 ${(T + B) / 2})`"
                >
                    {{ tr('mtf.yLabel') }}
                </text>
            </g>
            <polyline :points="points" fill="none" stroke="var(--accent)" stroke-width="2.4" />
        </svg>
        <p v-else class="empty" role="status">
            {{ pending ? tr('common.computing') : tr('mtf.noData') }}
        </p>
        <div v-if="mtf && run" class="legend" role="status">
            <span
                >{{ tr('fragments.revision') }} {{ run.revision }} {{ tr('fragments.field') }}
                {{ run.field }} ·
                {{
                    mtf.spectrum.kind === 'monochromatic'
                        ? describeSpectrum(mtf.spectrum)
                        : tr('mtf.spectrumPoly')
                }}
                · {{ describeSampling(mtf.sampling) }} · {{ describeMapping(run.mapping) }}</span
            >
            <span
                >{{ mtf.rays }} {{ tr('fragments.raysBackend') }} {{ mtf.backend }}
                {{ tr('fragments.transmittedWeight') }}
                {{ mtf.transmitted_weight.toPrecision(4) }}</span
            >
            <span
                >{{ tr('fragments.globalImagePlaneAxis') }} {{ mtf.axis }}
                {{ tr('fragments.notFieldLocalCommonPhysicalFrequenciesIn') }} {{ mtf.units }}</span
            >
            <span
                >{{ tr('fragments.imageOriginWeightedCentroid') }} {{ mtf.origin[0].toFixed(5) }},
                {{ mtf.origin[1].toFixed(5) }} mm · {{ mtf.normalization }}</span
            >
        </div>
        <div v-if="mtf && mtf.components.length" class="ledger-wrap">
            <SpectralLedger :rows="rows" :caption="tr('mtf.ledgerCaption')" />
        </div>
        <details v-if="mtf" class="data-table">
            <summary>{{ tr('mtf.dataTable') }}</summary>
            <table>
                <caption class="sr-only">
                    {{
                        tr('mtf.dataCaption')
                    }}
                </caption>
                <thead>
                    <tr>
                        <th scope="col">{{ tr('mtf.colFrequency') }}</th>
                        <th scope="col">{{ tr('mtf.yLabel') }}</th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="(f, i) in mtf.frequencies" :key="i">
                        <td>{{ f.toFixed(1) }}</td>
                        <td>{{ (mtf.mtf[i] ?? 0).toFixed(4) }}</td>
                    </tr>
                </tbody>
            </table>
        </details>
    </section>
</template>
