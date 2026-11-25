<script setup lang="ts">
import { tr } from '../i18n';
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import { rgbCss } from '../color';
import { checkRevision, savedSystemChanged } from '../provenance';
import { describeMapping, describeSampling, percent } from '../sampling';
import type {
    OpticalSystem,
    PolychromaticSpot,
    PupilMapping,
    PupilSampling,
    RunInfo,
    SpotDiagram,
} from '../types';
import { fieldLabel } from '../validation';
import AnalysisToolbar from './AnalysisToolbar.vue';
import SpotCanvas from './SpotCanvas.vue';
import SpectralLedger, { type LedgerRow } from './SpectralLedger.vue';

const props = defineProps<{
    system: OpticalSystem;
    spot: SpotDiagram | null;
    /** Provenance of `spot`, captured when its request started. */
    spotRun: RunInfo | null;
    spotPending: boolean;
    sampling: PupilSampling;
    fieldIndex: number;
    wavelengthIndex: number;
    revision: number;
    online: boolean;
}>();
const emit = defineEmits<{
    'update:fieldIndex': [value: number];
    'update:wavelengthIndex': [value: number];
    'update:sampling': [value: PupilSampling];
}>();

const mode = ref<'single' | 'all'>('single');
const poly = ref<PolychromaticSpot | null>(null);
const polyRun = ref<{ field: string; revision: number; mapping: PupilMapping } | null>(null);
const polyPending = ref(false);
const polyError = ref('');
let token = 0;

/** Drops the previous result first, so nothing old stays under changed controls. */
async function loadPoly(): Promise<void> {
    const mine = ++token;
    poly.value = null;
    polyRun.value = null;
    polyError.value = '';
    polyPending.value = false;
    if (mode.value !== 'all' || !props.online) return;
    const field = props.fieldIndex;
    const sampling = { ...props.sampling };
    const run = {
        field: fieldLabel(props.system, field),
        revision: props.revision,
        mapping: props.system.pupil_mapping,
    };
    polyPending.value = true;
    try {
        const r = await callTool('polychromatic_spot', { field_index: field, sampling });
        if (mine !== token) return;
        const check = checkRevision(run.revision, props.revision, r.revision);
        if (check === 'superseded') return;
        if (check === 'newer') {
            polyError.value = savedSystemChanged(tr('remaining.m6'), run.revision, r.revision);
            return;
        }
        poly.value = r;
        polyRun.value = run;
    } catch (e) {
        if (mine !== token) return;
        polyError.value = errorMessage(e);
    } finally {
        if (mine === token) polyPending.value = false;
    }
}
onBeforeUnmount(() => {
    token++;
});
watch(
    [mode, () => props.fieldIndex, () => props.revision, () => props.online, () => props.sampling],
    () => void loadPoly(),
    { immediate: true },
);

const polyRows = computed<LedgerRow[]>(() =>
    (poly.value?.failures ?? []).map((f) => ({
        wavelength: f.wavelength,
        weight: f.weight,
        transmitted: f.transmitted,
        sampled: f.transmitted + f.rejected,
        fraction: f.transmitted_fraction,
        effective: f.effective_weight,
        color: f.color,
    })),
);

const um = (v: number): string => (v * 1000).toFixed(2);
</script>

<template>
    <section class="panel" aria-labelledby="spot-title">
        <AnalysisToolbar
            title-id="spot-title"
            :title="tr('spot.title')"
            :system="system"
            :field-index="fieldIndex"
            :wavelength-index="mode === 'single' ? wavelengthIndex : undefined"
            :sampling="sampling"
            @update:field-index="emit('update:fieldIndex', $event)"
            @update:wavelength-index="emit('update:wavelengthIndex', $event)"
            @update:sampling="emit('update:sampling', $event)"
        >
            <label
                >{{ tr('spot.view') }}
                <select v-model="mode">
                    <option value="single">{{ tr('spot.viewSingle') }}</option>
                    <option value="all">{{ tr('spot.viewAll') }}</option>
                </select>
            </label>
        </AnalysisToolbar>
        <div v-if="mode === 'all' && polyError" role="alert" class="banner error inset">
            {{ polyError }}
        </div>
        <SpotCanvas
            v-if="mode === 'single' && spot && spotRun"
            :points="spot.points"
            :centroid="spot.centroid"
            :color="rgbCss(spotRun.color)"
            :opacity="0.65"
            :label="
                tr('spot.canvasSingle', { points: spot.points.length, rms: um(spot.rms_radius) })
            "
        />
        <SpotCanvas
            v-else-if="mode === 'all' && poly && polyRun"
            :points="poly.points"
            :centroid="poly.centroid"
            :colors="poly.colors"
            :opacity="0.6"
            :label="tr('spot.canvasPoly', { points: poly.points.length, rms: um(poly.rms_radius) })"
        />
        <p v-else class="empty" role="status">
            {{
                (mode === 'all' ? polyPending : spotPending)
                    ? tr('common.computing')
                    : tr('spot.noData')
            }}
        </p>

        <div v-if="mode === 'single' && spot && spotRun" class="legend" role="status">
            <span
                >{{ tr('fragments.revision') }} {{ spotRun.revision }} {{ tr('fragments.field') }}
                {{ spotRun.field }} · {{ spotRun.wavelength }} nm ·
                {{ describeSampling(spot.sampling) }} · {{ describeMapping(spotRun.mapping) }}</span
            >
            <span
                >{{ spot.points.length }} {{ tr('fragments.transmittedOf') }}
                {{ spot.sampled_rays }} {{ tr('fragments.sampledRays') }}
                {{ percent(spot.transmitted_weight) }}
                {{ tr('fragments.ofNominalPupilAreaTransmitted') }}</span
            >
            <span>{{ tr('spot.legendCoords') }}</span>
            <span
                >{{ tr('fragments.invalid') }} {{ spot.blocked }} {{ tr('fragments.blocked') }}
                {{ spot.missed }} {{ tr('fragments.missedFailed') }}
                {{ spot.total_internal_reflection }} {{ tr('ray.failTir') }}</span
            >
            <span
                >{{ tr('fragments.areaWeightedRms') }} {{ um(spot.rms_radius) }}
                {{ tr('fragments.mGeometricRadius') }} {{ um(spot.geometric_radius) }}
                {{ tr('fragments.mCenteredOnTheWeightedCentroid') }}</span
            >
        </div>
        <template v-if="mode === 'all' && poly && polyRun">
            <div class="legend" role="status">
                <span
                    >{{ tr('fragments.revision') }} {{ polyRun.revision }}
                    {{ tr('fragments.field') }} {{ polyRun.field }} ·
                    {{ describeSampling(poly.sampling) }} · {{ describeMapping(polyRun.mapping) }}
                    {{ tr('fragments.allPositiveWeightWavelengths') }}</span
                >
                <span
                    >{{ poly.points.length }} {{ tr('fragments.transmittedOf') }}
                    {{ poly.sampled_rays }} {{ tr('fragments.sampledRaysValue') }}</span
                >
                <span
                    >{{ tr('fragments.weightedRms') }} {{ um(poly.rms_radius) }}
                    {{
                        tr('fragments.mAboutTheCommonWeightedCentroidNoPerWavelengthRecentering')
                    }}</span
                >
                <span
                    >{{ tr('fragments.transmittedSpectralAreaWeight') }}
                    {{ poly.transmitted_weight.toPrecision(4)
                    }}{{ tr('fragments.zeroWeightWavelengthsAreSkipped') }}</span
                >
            </div>
            <div class="ledger-wrap">
                <SpectralLedger :rows="polyRows" :caption="tr('spot.ledgerCaption')" />
            </div>
        </template>
    </section>
</template>
