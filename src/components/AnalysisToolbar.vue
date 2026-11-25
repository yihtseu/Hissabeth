<script setup lang="ts">
import { tr } from '../i18n';
import type { OpticalSystem, PupilSampling } from '../types';
import type { SpectralMode } from '../sampling';
import { fieldLabel } from '../validation';
import RaySamplingControl from './RaySamplingControl.vue';

/**
 * Shared top bar for the three analysis views. Field, spectral mode, wavelength
 * and pupil sampling always render in the same order with the same labels;
 * view-specific controls (axis, max frequency, recompute, …) go in the default
 * slot. A control appears only when its model is provided, so callers keep full
 * control over which requests a view sends.
 */
const props = defineProps<{
    system: OpticalSystem;
    /** Id referenced by the parent panel's `aria-labelledby`. */
    titleId: string;
    title: string;
    /** Small provenance line, e.g. the saved-system revision. */
    subtitle?: string;
    fieldIndex?: number;
    wavelengthIndex?: number;
    spectralMode?: SpectralMode;
    sampling?: PupilSampling;
}>();
const emit = defineEmits<{
    'update:fieldIndex': [value: number];
    'update:wavelengthIndex': [value: number];
    'update:spectralMode': [value: SpectralMode];
    'update:sampling': [value: PupilSampling];
}>();

function onSpectral(e: Event): void {
    const v = (e.target as HTMLSelectElement).value;
    if (v === 'mono' || v === 'poly') emit('update:spectralMode', v);
}
const showWavelength = () =>
    props.wavelengthIndex !== undefined &&
    (props.spectralMode === undefined || props.spectralMode === 'mono');
</script>

<template>
    <div class="panel-head analysis-toolbar">
        <h3 :id="titleId">{{ title }}</h3>
        <span v-if="subtitle">{{ subtitle }}</span>
        <div class="controls">
            <label v-if="fieldIndex !== undefined"
                >{{ tr('analysis.field') }}
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
            <label v-if="spectralMode !== undefined"
                >{{ tr('analysis.spectralMode') }}
                <select :value="spectralMode" @change="onSpectral">
                    <option value="mono">{{ tr('analysis.mono') }}</option>
                    <option value="poly">{{ tr('analysis.poly') }}</option>
                </select>
            </label>
            <label v-if="showWavelength()"
                >{{ tr('analysis.wavelength') }}
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
            <RaySamplingControl
                v-if="sampling !== undefined"
                :model-value="sampling"
                @update:model-value="emit('update:sampling', $event)"
            />
            <slot />
        </div>
    </div>
</template>

<style scoped>
.analysis-toolbar {
    row-gap: 8px;
}
@media (max-width: 600px) {
    .analysis-toolbar {
        align-items: flex-start;
        flex-direction: column;
    }
    .analysis-toolbar .controls {
        width: 100%;
    }
}
</style>
