<script setup lang="ts">
import { tr } from '../i18n';
import { GRID_RANGE, RING_RANGE } from '../sampling';
import type { PupilSampling } from '../types';

const props = defineProps<{ modelValue: PupilSampling }>();
const emit = defineEmits<{ 'update:modelValue': [value: PupilSampling] }>();

/** Last count of each pattern, so toggling back and forth does not lose it. */
const remembered = { grid: 25, hexapolar: 8 };
if (props.modelValue.kind === 'grid') remembered.grid = props.modelValue.count;
else remembered.hexapolar = props.modelValue.rings;

const current = (): number =>
    props.modelValue.kind === 'grid' ? props.modelValue.count : props.modelValue.rings;

function onKind(e: Event): void {
    const v = (e.target as HTMLSelectElement).value;
    if (v === 'grid') emit('update:modelValue', { kind: 'grid', count: remembered.grid });
    else if (v === 'hexapolar')
        emit('update:modelValue', { kind: 'hexapolar', rings: remembered.hexapolar });
}
function onCount(e: Event): void {
    const input = e.target as HTMLInputElement;
    const n = Number(input.value);
    const range = props.modelValue.kind === 'grid' ? GRID_RANGE : RING_RANGE;
    if (input.value.trim() === '' || !Number.isInteger(n) || n < range.min || n > range.max) {
        input.value = String(current()); // invalid entries never reach a request
        return;
    }
    if (props.modelValue.kind === 'grid') {
        remembered.grid = n;
        emit('update:modelValue', { kind: 'grid', count: n });
    } else {
        remembered.hexapolar = n;
        emit('update:modelValue', { kind: 'hexapolar', rings: n });
    }
}
</script>

<template>
    <span class="sampling-control" role="group" :aria-label="tr('sampling.label')">
        <label
            >{{ tr('sampling.pattern') }}
            <select :value="modelValue.kind" @change="onKind">
                <option value="grid">{{ tr('sampling.grid') }}</option>
                <option value="hexapolar">{{ tr('sampling.hexapolar') }}</option>
            </select>
        </label>
        <label v-if="modelValue.kind === 'grid'" :title="tr('sampling.diameterTip')"
            >{{ tr('fragments.diameterPoints') }}{{ GRID_RANGE.min }}–{{ GRID_RANGE.max }})
            <input
                type="number"
                :min="GRID_RANGE.min"
                :max="GRID_RANGE.max"
                step="1"
                :value="modelValue.count"
                @change="onCount"
            />
        </label>
        <label v-else :title="tr('sampling.ringsTip')"
            >{{ tr('fragments.rings') }}{{ RING_RANGE.min }}–{{ RING_RANGE.max }})
            <input
                type="number"
                :min="RING_RANGE.min"
                :max="RING_RANGE.max"
                step="1"
                :value="modelValue.rings"
                @change="onCount"
            />
        </label>
    </span>
</template>
