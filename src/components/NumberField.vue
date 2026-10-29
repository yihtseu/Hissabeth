<script setup lang="ts">
import { ref, watch } from 'vue';

const props = defineProps<{ modelValue: number }>();
const emit = defineEmits<{ 'update:modelValue': [value: number] }>();

const text = ref(String(props.modelValue));
const invalid = ref(false);

watch(
    () => props.modelValue,
    (v) => {
        if (Number(text.value) !== v) text.value = String(v);
    },
);

function onInput(e: Event): void {
    const raw = (e.target as HTMLInputElement).value;
    text.value = raw;
    const n = raw.trim() === '' ? NaN : Number(raw);
    invalid.value = !Number.isFinite(n);
    if (!invalid.value) emit('update:modelValue', n);
}

function onBlur(): void {
    text.value = String(props.modelValue);
    invalid.value = false;
}
</script>

<template>
    <input
        type="number"
        step="any"
        :value="text"
        :aria-invalid="invalid"
        @input="onInput"
        @blur="onBlur"
    />
</template>
