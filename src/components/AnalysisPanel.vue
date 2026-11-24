<script setup lang="ts">
import { tr } from '../i18n';
import { usePreferencesStore } from '../stores/preferences';
import { samplingPreset } from '../preferences';
import { computed, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import { checkRevision, savedSystemChanged } from '../provenance';
import { describeSampling, type SpectralMode } from '../sampling';
import { FALLBACK_COLOR } from '../color';
import type {
    AnalysisKind,
    AnalysisOutcome,
    OpticalSystem,
    PupilSampling,
    RGBA,
    Spectrum,
} from '../types';
import { fieldLabel } from '../validation';
import AnalysisResult from './AnalysisResult.vue';
import RaySamplingControl from './RaySamplingControl.vue';

const props = defineProps<{
    system: OpticalSystem;
    revision: number;
    online: boolean;
    dirty: boolean;
}>();

// Sampling values follow the product specification.
const CURVE_SAMPLES = 33;
const WAVEFRONT_GRID = 17;
const DIFFRACTION_GRID = 32;
const ZERNIKE_GRID = 33;
const ZERNIKE_TERMS = 37;

const CURVATURE_MIN = 2;
const CURVATURE_MAX = 257;
const FIELD_CURVATURE_ID: AnalysisKind = 'field_curvature';

interface Params {
    fieldIndex: number;
    wavelength: number;
    lower: number;
    upper: number;
    sampling: PupilSampling;
    spectrum: Spectrum;
    /** Field curvature only: sweep samples and the optional endpoint (omitted means +Y at the largest field). */
    curvatureSamples: number;
    fieldEnd: [number, number] | undefined;
    /** Gaussian tools only: input 1/e² waist radius in mm, its global z, and the fiber mode field diameter. */
    waist: number;
    waistZ: number;
    mfd: number;
}
interface Def {
    id: AnalysisKind;
    label: string;
    help: string;
    field: boolean;
    wavelength: boolean;
    /** Uses the shared Mono/Weighted spectral mode instead of a bare wavelength (wavelength is then only shown in mono). */
    spectral?: boolean;
    /** Geometric ray sampling control (grid or hexapolar); FFT analyses never offer it. */
    rays?: boolean;
    /** lower/upper wavelengths come from the saved wavelength range */
    range?: boolean;
    sampling: string;
    run: (p: Params) => Promise<AnalysisOutcome>;
}

const DEFS: Def[] = [
    {
        id: 'ray_fan',
        get label() {
            return tr('analysis.defs.ray_fan.label');
        },
        get help() {
            return tr('analysis.defs.ray_fan.help');
        },
        field: true,
        wavelength: true,
        get sampling() {
            return tr('analysis.samplesPerFan', { n: CURVE_SAMPLES });
        },
        run: async (p) => ({
            kind: 'ray_fan',
            data: await callTool('ray_fan', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
                samples: CURVE_SAMPLES,
            }),
        }),
    },
    {
        id: 'opd_fan',
        get label() {
            return tr('analysis.defs.opd_fan.label');
        },
        get help() {
            return tr('analysis.defs.opd_fan.help');
        },
        field: true,
        wavelength: true,
        get sampling() {
            return tr('analysis.samplesPerFan', { n: CURVE_SAMPLES });
        },
        run: async (p) => ({
            kind: 'opd_fan',
            data: await callTool('opd_fan', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
                samples: CURVE_SAMPLES,
            }),
        }),
    },
    {
        id: 'chromatic_focus',
        get label() {
            return tr('analysis.defs.chromatic_focus.label');
        },
        get help() {
            return tr('analysis.defs.chromatic_focus.help');
        },
        field: false,
        wavelength: false,
        range: true,
        get sampling() {
            return tr('analysis.samplesWavelengths', { n: CURVE_SAMPLES });
        },
        run: async (p) => ({
            kind: 'chromatic_focus',
            data: await callTool('chromatic_focus', {
                lower: p.lower,
                upper: p.upper,
                samples: CURVE_SAMPLES,
            }),
        }),
    },
    {
        id: 'longitudinal_aberration',
        get label() {
            return tr('analysis.defs.longitudinal_aberration.label');
        },
        get help() {
            return tr('analysis.defs.longitudinal_aberration.help');
        },
        field: false,
        wavelength: true,
        get sampling() {
            return tr('analysis.samplesPupilHeights', { n: CURVE_SAMPLES });
        },
        run: async (p) => ({
            kind: 'longitudinal_aberration',
            data: await callTool('longitudinal_aberration', {
                wavelength: p.wavelength,
                samples: CURVE_SAMPLES,
            }),
        }),
    },
    {
        id: 'spherochromatism',
        get label() {
            return tr('analysis.defs.spherochromatism.label');
        },
        get help() {
            return tr('analysis.defs.spherochromatism.help');
        },
        field: false,
        wavelength: false,
        get sampling() {
            return tr('analysis.samplesPupilHeightsPerWavelength', { n: CURVE_SAMPLES });
        },
        run: async () => ({
            kind: 'spherochromatism',
            data: await callTool('spherochromatism', { samples: CURVE_SAMPLES }),
        }),
    },
    {
        id: 'field_curvature',
        get label() {
            return tr('analysis.defs.field_curvature.label');
        },
        get help() {
            return tr('analysis.defs.field_curvature.help');
        },
        field: false,
        wavelength: false,
        get sampling() {
            return tr('analysis.defs.field_curvature.detail');
        },
        run: async (p) => ({
            kind: 'field_curvature',
            data: await callTool(
                'field_curvature',
                p.fieldEnd
                    ? { samples: p.curvatureSamples, field_end: p.fieldEnd }
                    : { samples: p.curvatureSamples },
            ),
        }),
    },
    {
        id: 'astigmatism',
        get label() {
            return tr('analysis.defs.astigmatism.label');
        },
        get help() {
            return tr('analysis.defs.astigmatism.help');
        },
        field: true,
        wavelength: true,
        get sampling() {
            return tr('analysis.defs.astigmatism.detail');
        },
        run: async (p) => ({
            kind: 'astigmatism',
            data: await callTool('astigmatism', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
            }),
        }),
    },
    {
        id: 'distortion',
        get label() {
            return tr('analysis.defs.distortion.label');
        },
        get help() {
            return tr('analysis.defs.distortion.help');
        },
        field: true,
        wavelength: true,
        get sampling() {
            return tr('analysis.defs.distortion.detail');
        },
        run: async (p) => ({
            kind: 'distortion',
            data: await callTool('distortion', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
            }),
        }),
    },
    {
        id: 'wavefront',
        get label() {
            return tr('analysis.defs.wavefront.label');
        },
        get help() {
            return tr('analysis.defs.wavefront.help');
        },
        field: true,
        wavelength: true,
        get sampling() {
            return tr('analysis.pupilGrid', { n: WAVEFRONT_GRID });
        },
        run: async (p) => ({
            kind: 'wavefront',
            data: await callTool('wavefront', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
                grid: WAVEFRONT_GRID,
            }),
        }),
    },
    {
        id: 'diffraction_psf',
        get label() {
            return tr('analysis.defs.diffraction_psf.label');
        },
        get help() {
            return tr('analysis.defs.diffraction_psf.help');
        },
        field: true,
        wavelength: true,
        get sampling() {
            return tr('analysis.pupilGrid', { n: DIFFRACTION_GRID });
        },
        run: async (p) => ({
            kind: 'diffraction_psf',
            data: await callTool('diffraction_psf', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
                grid: DIFFRACTION_GRID,
            }),
        }),
    },
    {
        id: 'diffraction_mtf',
        get label() {
            return tr('analysis.defs.diffraction_mtf.label');
        },
        get help() {
            return tr('analysis.defs.diffraction_mtf.help');
        },
        field: true,
        wavelength: false,
        spectral: true,
        get sampling() {
            return tr('analysis.pupilGrid', { n: DIFFRACTION_GRID });
        },
        run: async (p) => ({
            kind: 'diffraction_mtf',
            data: await callTool('diffraction_mtf', {
                field_index: p.fieldIndex,
                spectrum: p.spectrum,
                grid: DIFFRACTION_GRID,
            }),
        }),
    },
    {
        id: 'encircled_energy',
        get label() {
            return tr('analysis.defs.encircled_energy.label');
        },
        get help() {
            return tr('analysis.defs.encircled_energy.help');
        },
        field: true,
        wavelength: true,
        rays: true,
        get sampling() {
            return tr('analysis.samplesRadii', { n: CURVE_SAMPLES });
        },
        run: async (p) => ({
            kind: 'encircled_energy',
            data: await callTool('encircled_energy', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
                sampling: p.sampling,
                samples: CURVE_SAMPLES,
            }),
        }),
    },
    {
        id: 'wavefront_zernike',
        get label() {
            return tr('analysis.defs.wavefront_zernike.label');
        },
        get help() {
            return tr('analysis.defs.wavefront_zernike.help');
        },
        field: true,
        wavelength: true,
        get sampling() {
            return tr('analysis.zernikeDesc', { grid: ZERNIKE_GRID, terms: ZERNIKE_TERMS });
        },
        run: async (p) => ({
            kind: 'wavefront_zernike',
            data: await callTool('wavefront_zernike', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
                grid: ZERNIKE_GRID,
                terms: ZERNIKE_TERMS,
            }),
        }),
    },
    {
        id: 'huygens_psf',
        get label() {
            return tr('analysis.defs.huygens_psf.label');
        },
        get help() {
            return tr('analysis.defs.huygens_psf.help');
        },
        field: true,
        wavelength: true,
        get sampling() {
            return tr('analysis.pupilGrid', { n: DIFFRACTION_GRID });
        },
        run: async (p) => ({
            kind: 'huygens_psf',
            data: await callTool('huygens_psf', {
                field_index: p.fieldIndex,
                wavelength: p.wavelength,
                grid: DIFFRACTION_GRID,
            }),
        }),
    },
    {
        id: 'huygens_mtf',
        get label() {
            return tr('analysis.defs.huygens_mtf.label');
        },
        get help() {
            return tr('analysis.defs.huygens_mtf.help');
        },
        field: true,
        wavelength: false,
        spectral: true,
        get sampling() {
            return tr('analysis.pupilGrid', { n: DIFFRACTION_GRID });
        },
        run: async (p) => ({
            kind: 'huygens_mtf',
            data: await callTool('huygens_mtf', {
                field_index: p.fieldIndex,
                spectrum: p.spectrum,
                grid: DIFFRACTION_GRID,
            }),
        }),
    },
    {
        id: 'gaussian_beam',
        get label() {
            return tr('analysis.defs.gaussian_beam.label');
        },
        get help() {
            return tr('analysis.defs.gaussian_beam.help');
        },
        field: false,
        wavelength: true,
        get sampling() {
            return tr('analysis.gaussianDesc');
        },
        run: async (p) => ({
            kind: 'gaussian_beam',
            data: await callTool('gaussian_beam', {
                wavelength: p.wavelength,
                waist: p.waist,
                waist_z: p.waistZ,
            }),
        }),
    },
    {
        id: 'fiber_coupling',
        get label() {
            return tr('analysis.defs.fiber_coupling.label');
        },
        get help() {
            return tr('analysis.defs.fiber_coupling.help');
        },
        field: false,
        wavelength: true,
        get sampling() {
            return tr('analysis.gaussianDesc');
        },
        run: async (p) => ({
            kind: 'fiber_coupling',
            data: await callTool('fiber_coupling', {
                wavelength: p.wavelength,
                waist: p.waist,
                waist_z: p.waistZ,
                mode_field_diameter: p.mfd,
            }),
        }),
    },
];

const kind = ref<AnalysisKind>('ray_fan');
const fieldIndex = ref(0);
const wavelengthIndex = ref(props.system.reference_wavelength);
const spectralMode = ref<SpectralMode>('mono');
/** Kept across revisions and analysis kinds; changes only on user action. */
const energySampling = ref<PupilSampling>(
    samplingPreset(usePreferencesStore().preferences.analysis),
);
/** Field slice: 'default' omits the endpoint; 'x+', 'y-' etc. are maxima on an axis; 'f<n>' is stored field n. */
const slice = ref('default');
/** Applied sweep sample count (drives requests) and the raw text being edited; only Apply moves text into the count. */
const curvatureSamples = ref(33);
const samplesText = ref(String(curvatureSamples.value));
/** Gaussian beam inputs (mm); kept across kinds, validated before any request. */
const gaussianWaist = ref(1);
const gaussianWaistZ = ref(0);
const fiberMfd = ref(0.02);
const isGaussian = computed(
    () => kind.value === 'gaussian_beam' || kind.value === 'fiber_coupling',
);
const gaussianValid = computed(
    () =>
        Number.isFinite(gaussianWaist.value) &&
        gaussianWaist.value > 0 &&
        gaussianWaist.value <= 1000 &&
        Math.abs(gaussianWaistZ.value) <= 1e9 &&
        (kind.value !== 'fiber_coupling' ||
            (Number.isFinite(fiberMfd.value) && fiberMfd.value > 0 && fiberMfd.value <= 1000)),
);
const parsedSamples = computed(() => {
    const t = samplesText.value.trim();
    const n = t === '' ? NaN : Number(t);
    return Number.isInteger(n) && n >= CURVATURE_MIN && n <= CURVATURE_MAX ? n : null;
});
const samplesValid = computed(() => parsedSamples.value !== null);
const samplesUnapplied = computed(
    () => samplesValid.value && parsedSamples.value !== curvatureSamples.value,
);
/** Commits a valid whole number; the existing key watcher then runs the analysis once. Invalid text sends nothing. */
function applySamples(): void {
    const n = parsedSamples.value;
    if (n === null) return;
    curvatureSamples.value = n;
    samplesText.value = String(n);
}
const outcome = ref<AnalysisOutcome | null>(null);
const error = ref('');
const loading = ref(false);
const ranField = ref('');
const ranRevision = ref(props.revision);
const ranColor = ref<RGBA>(FALLBACK_COLOR);
let token = 0;

const def = computed(() => DEFS.find((d) => d.id === kind.value)!);
const showWavelength = computed(
    () => def.value.wavelength || (def.value.spectral && spectralMode.value === 'mono'),
);
const samplingText = computed(() =>
    def.value.rays
        ? `${describeSampling(energySampling.value)}, ${def.value.sampling}`
        : kind.value === FIELD_CURVATURE_ID
          ? tr('analysis.curvatureDesc', {
                samples: curvatureSamples.value,
                slice: sliceText(props.system),
                detail: def.value.sampling,
            })
          : def.value.sampling,
);

// Keep the chosen wavelength by value when a revision reorders or edits the wavelength list.
let selectedValue = props.system.wavelengths[wavelengthIndex.value]?.value;
watch(wavelengthIndex, (i) => {
    selectedValue = props.system.wavelengths[i]?.value;
});
watch(
    () => props.system,
    (sys) => {
        // Keep the current entry when it still holds the selected value, so duplicate values keep their own index and color.
        if (sys.wavelengths[wavelengthIndex.value]?.value === selectedValue) return;
        const kept = sys.wavelengths.findIndex((w) => w.value === selectedValue);
        wavelengthIndex.value = kept >= 0 ? kept : sys.reference_wavelength;
    },
);
const isCurvature = computed(() => kind.value === FIELD_CURVATURE_ID);
const maxField = (sys: OpticalSystem): number =>
    Math.max(...sys.fields.map((f) => Math.hypot(f.x, f.y)));
/** Endpoint for the request; undefined omits it so the backend uses +Y at the largest field. */
function sliceEnd(sys: OpticalSystem): [number, number] | undefined {
    const m = maxField(sys);
    switch (slice.value) {
        case 'x+':
            return [m, 0];
        case 'x-':
            return [-m, 0];
        case 'y-':
            return [0, -m];
        case 'default':
            return undefined;
        default: {
            const f = sys.fields[Number(slice.value.slice(1))];
            return f ? [f.x, f.y] : undefined;
        }
    }
}
const sliceText = (sys: OpticalSystem): string => {
    if (slice.value.startsWith('f'))
        return tr('analysis.sliceTextStored', {
            label: fieldLabel(sys, Number(slice.value.slice(1))),
        });
    return (
        (
            {
                get default() {
                    return tr('analysis.sliceYPlus');
                },
                get 'x+'() {
                    return tr('analysis.sliceXPlus');
                },
                get 'x-'() {
                    return tr('analysis.sliceXMinus');
                },
                get 'y-'() {
                    return tr('analysis.sliceYMinus');
                },
            } as Record<string, string>
        )[slice.value] ?? '+Y maximum'
    );
};
// A stored-field slice must keep addressing an existing field after the prescription changes.
watch(
    () => props.system,
    (sys) => {
        if (slice.value.startsWith('f') && Number(slice.value.slice(1)) >= sys.fields.length)
            slice.value = 'default';
    },
);
const curvatureKey = computed(() =>
    isCurvature.value ? `${slice.value}|${curvatureSamples.value}` : '',
);
const range = computed(() => {
    const v = props.system.wavelengths.map((w) => w.value);
    return { lower: Math.min(...v), upper: Math.max(...v) };
});

async function run(): Promise<void> {
    if (!props.online) return;
    const mine = ++token;
    const sys = props.system;
    if (fieldIndex.value >= sys.fields.length) fieldIndex.value = 0;
    if (wavelengthIndex.value >= sys.wavelengths.length)
        wavelengthIndex.value = sys.reference_wavelength;
    const d = def.value;
    if (d.range && !(range.value.upper > range.value.lower)) {
        outcome.value = null;
        error.value = tr('analysis.needsTwoWavelengths');
        return;
    }
    if (isGaussian.value && !gaussianValid.value) {
        outcome.value = null;
        error.value = '';
        return;
    }
    // Everything the response is labeled with is captured here, before dispatch.
    const revisionAtRequest = props.revision;
    const field = fieldIndex.value;
    const fieldText = fieldLabel(sys, field);
    const wavelength = sys.wavelengths[wavelengthIndex.value]!.value;
    const color = sys.wavelengths[wavelengthIndex.value]!.color;
    loading.value = true;
    try {
        const result = await d.run({
            fieldIndex: field,
            wavelength,
            lower: range.value.lower,
            upper: range.value.upper,
            sampling: { ...energySampling.value },
            curvatureSamples: curvatureSamples.value,
            fieldEnd: sliceEnd(sys),
            waist: gaussianWaist.value,
            waistZ: gaussianWaistZ.value,
            mfd: fiberMfd.value,
            spectrum:
                spectralMode.value === 'poly'
                    ? { kind: 'polychromatic' }
                    : { kind: 'monochromatic', wavelength },
        });
        if (mine !== token) return;
        const check = checkRevision(revisionAtRequest, props.revision, result.data.revision);
        if (check === 'superseded') return;
        if (check === 'newer') {
            outcome.value = null;
            error.value = savedSystemChanged(d.label, revisionAtRequest, result.data.revision);
            return;
        }
        outcome.value = result;
        ranRevision.value = revisionAtRequest;
        ranField.value = fieldText;
        ranColor.value = color;
        error.value = '';
    } catch (e) {
        if (mine !== token) return;
        outcome.value = null;
        error.value = errorMessage(e);
    } finally {
        if (mine === token) loading.value = false;
    }
}

// Any input change drops the previous result and in-flight request immediately so a mismatched view is never shown.
// The wavelength only matters when it is part of the request.
watch(
    [
        kind,
        fieldIndex,
        () => (showWavelength.value ? wavelengthIndex.value : -1),
        spectralMode,
        energySampling,
        curvatureKey,
        gaussianWaist,
        gaussianWaistZ,
        fiberMfd,
        () => props.revision,
        () => props.online,
    ],
    () => {
        token++;
        loading.value = false;
        outcome.value = null;
        error.value = '';
        void run();
    },
    { immediate: true },
);
</script>

<template>
    <section class="panel" aria-labelledby="an-title">
        <div class="panel-head">
            <h3 id="an-title">{{ tr('analysis.title') }}</h3>
            <span
                >{{ tr('fragments.computedOnTheSavedSystemRevision') }}
                {{ outcome ? ranRevision : revision }}</span
            >
        </div>
        <div v-if="dirty" role="status" class="banner warn inset">
            {{ tr('analysis.dirtyNote') }}
        </div>
        <div class="analysis-controls">
            <div class="controls">
                <label
                    >{{ tr('analysis.title') }}
                    <select v-model="kind" class="wide-select" aria-describedby="an-help">
                        <option v-for="d in DEFS" :key="d.id" :value="d.id">{{ d.label }}</option>
                    </select>
                </label>
                <label v-if="def.field"
                    >{{ tr('analysis.field') }}
                    <select v-model.number="fieldIndex">
                        <option v-for="(_, i) in system.fields" :key="i" :value="i">
                            {{ fieldLabel(system, i) }}
                        </option>
                    </select>
                </label>
                <label v-if="def.spectral"
                    >{{ tr('analysis.spectralMode') }}
                    <select v-model="spectralMode">
                        <option value="mono">{{ tr('analysis.mono') }}</option>
                        <option value="poly">{{ tr('analysis.poly') }}</option>
                    </select>
                </label>
                <label v-if="showWavelength"
                    >{{ tr('analysis.wavelength') }}
                    <select v-model.number="wavelengthIndex">
                        <option v-for="(w, i) in system.wavelengths" :key="i" :value="i">
                            {{ i + 1 }}: {{ w.value }} nm
                        </option>
                    </select>
                </label>
                <label v-if="isCurvature"
                    >{{ tr('analysis.fieldSlice') }}
                    <select v-model="slice" class="wide-select">
                        <option value="default">{{ tr('analysis.sliceYPlus') }}</option>
                        <option value="x+">{{ tr('analysis.sliceXPlus') }}</option>
                        <option value="y-">{{ tr('analysis.sliceYMinus') }}</option>
                        <option value="x-">{{ tr('analysis.sliceXMinus') }}</option>
                        <option v-for="(_, i) in system.fields" :key="i" :value="`f${i}`">
                            {{ tr('fragments.storedField') }} {{ fieldLabel(system, i) }}
                        </option>
                    </select>
                </label>
                <form v-if="isCurvature" class="controls" @submit.prevent="applySamples">
                    <label
                        >{{ tr('analysis.fieldSamples') }}
                        <input
                            type="number"
                            min="2"
                            max="257"
                            step="1"
                            required
                            :value="samplesText"
                            @input="samplesText = ($event.target as HTMLInputElement).value"
                        />
                    </label>
                    <button type="submit" :disabled="!samplesValid || !samplesUnapplied">
                        {{ tr('analysis.applySamples') }}
                    </button>
                </form>
                <template v-if="isGaussian">
                    <label
                        >{{ tr('analysis.waistMm') }}
                        <input
                            v-model.number="gaussianWaist"
                            type="number"
                            min="0.000001"
                            max="1000"
                            step="any"
                            required
                        />
                    </label>
                    <label
                        >{{ tr('analysis.waistZMm') }}
                        <input v-model.number="gaussianWaistZ" type="number" step="any" required />
                    </label>
                    <label v-if="kind === 'fiber_coupling'"
                        >{{ tr('analysis.mfdMm') }}
                        <input
                            v-model.number="fiberMfd"
                            type="number"
                            min="0.000001"
                            max="1000"
                            step="any"
                            required
                        />
                    </label>
                </template>
                <RaySamplingControl v-if="def.rays" v-model="energySampling" />
                <span v-if="def.range"
                    >{{ tr('sys.wavelengths') }} {{ range.lower }}–{{ range.upper }}
                    {{ tr('fragments.nmSavedRange') }}</span
                >
                <button
                    type="button"
                    :disabled="
                        !online ||
                        loading ||
                        (isCurvature && (!samplesValid || samplesUnapplied)) ||
                        (isGaussian && !gaussianValid)
                    "
                    @click="run()"
                >
                    {{ tr('analysis.recompute') }}
                </button>
            </div>
            <p v-if="isCurvature && !samplesValid" class="note warn-text" role="status">
                {{ tr('fragments.fieldSamplesMustBeAWholeNumberFrom') }} {{ CURVATURE_MIN }} to
                {{ CURVATURE_MAX }}{{ tr('fragments.nothingIsSentUntilItIsValidAndApplied') }}
            </p>
            <p v-else-if="isCurvature && samplesUnapplied" class="note" role="status">
                {{ tr('fragments.showing') }} {{ curvatureSamples }}
                {{ tr('fragments.samplesPressApplySamplesOrEnterToUse') }} {{ parsedSamples }}.
            </p>
            <p id="an-help" class="note">
                {{ def.help }} {{ tr('fragments.sampling') }} {{ samplingText }}.
            </p>
        </div>
        <div v-if="error" role="alert" class="banner error inset">{{ error }}</div>
        <p v-if="loading && !outcome" class="empty" role="status">{{ tr('common.computing') }}</p>
        <p v-else-if="!outcome && !error" class="empty">
            {{ online ? tr('analysis.noResult') : tr('common.offline') }}
        </p>
        <div v-if="outcome" class="analysis-body" :aria-busy="loading">
            <AnalysisResult
                :outcome="outcome"
                :field-text="def.field ? ranField : undefined"
                :color="ranColor"
            />
        </div>
    </section>
</template>
