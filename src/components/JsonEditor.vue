<script setup lang="ts">
import { ref, watch } from 'vue';
import type { OpticalSystem } from '../types';
import { parseSystem } from '../validation';

const props = defineProps<{ modelValue: OpticalSystem }>();
const emit = defineEmits<{ 'update:modelValue': [value: OpticalSystem] }>();

const pretty = (s: OpticalSystem): string => JSON.stringify(s, null, 2);
const text = ref(pretty(props.modelValue));
const parseError = ref('');
let lastEmitted = JSON.stringify(props.modelValue);

watch(
  () => props.modelValue,
  (v) => {
    if (JSON.stringify(v) !== lastEmitted) {
      text.value = pretty(v);
      parseError.value = '';
      lastEmitted = JSON.stringify(v);
    }
  },
);

function onInput(e: Event): void {
  text.value = (e.target as HTMLTextAreaElement).value;
  try {
    const parsed = parseSystem(JSON.parse(text.value) as unknown);
    if (typeof parsed === 'string') {
      parseError.value = parsed;
      return;
    }
    parseError.value = '';
    lastEmitted = JSON.stringify(parsed);
    emit('update:modelValue', parsed);
  } catch (err) {
    parseError.value = err instanceof Error ? err.message : String(err);
  }
}
</script>

<template>
  <section class="panel json-panel" aria-labelledby="json-title">
    <h3 id="json-title">Full prescription</h3>
    <p id="json-hint">Advanced view of the same draft as the Prescription tab. Valid edits update the draft; apply them with “Apply changes”.</p>
    <textarea :value="text" aria-labelledby="json-title" aria-describedby="json-hint json-error" :aria-invalid="parseError !== ''" spellcheck="false" @input="onInput" />
    <p id="json-error" class="json-error" role="status">{{ parseError }}</p>
  </section>
</template>
