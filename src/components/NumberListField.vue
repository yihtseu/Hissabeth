<script setup lang="ts">
import { ref, watch } from 'vue';
import { parseNumberList } from '../validation';

const props = withDefaults(defineProps<{ modelValue: number[]; allowEmpty?: boolean }>(), {
    allowEmpty: false,
});
const emit = defineEmits<{ 'update:modelValue': [value: number[]] }>();

const text = ref(props.modelValue.join(', '));
const invalid = ref(false);

watch(
    () => props.modelValue,
    (v) => {
        const current = parseNumberList(text.value);
        if (!current || JSON.stringify(current) !== JSON.stringify(v)) text.value = v.join(', ');
    },
);

function onInput(e: Event): void {
    const raw = (e.target as HTMLInputElement).value;
    text.value = raw;
    const list = parseNumberList(raw);
    invalid.value = list === null || (!props.allowEmpty && list.length === 0);
    if (!invalid.value && list) emit('update:modelValue', list);
}

function onBlur(): void {
    text.value = props.modelValue.join(', ');
    invalid.value = false;
}
</script>

<template>
    <input
        type="text"
        inputmode="decimal"
        autocomplete="off"
        :value="text"
        :aria-invalid="invalid"
        @input="onInput"
        @blur="onBlur"
    />
</template>
