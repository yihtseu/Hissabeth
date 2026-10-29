<script setup lang="ts">
import NumberField from './NumberField.vue';
import NumberListField from './NumberListField.vue';
import type { EndpointSurface } from '../types';

const props = defineProps<{
  /** "Object" or "Image"; prefixes every accessible label. */
  name: string;
  modelValue: EndpointSurface;
  /** Concise statement of where this profile sits and what it requires. */
  note: string;
  /** Problem with the current draft values, shown beside the controls. */
  problem?: string;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: EndpointSurface] }>();

const patch = (p: Partial<EndpointSurface>): void => emit('update:modelValue', { ...props.modelValue, ...p });
const label = (what: string): string => `${props.name} surface ${what}`;
</script>

<template>
  <details class="subpanel endpoint">
    <summary><h4>{{ name }} surface</h4></summary>
    <div class="form-grid">
      <div class="field">
        <label :for="`ep-${name}-radius`">Radius (mm, 0 = plane)</label>
        <NumberField :id="`ep-${name}-radius`" :aria-label="label('radius')" :model-value="modelValue.radius" @update:model-value="(v: number) => patch({ radius: v })" />
      </div>
      <div class="field">
        <label :for="`ep-${name}-conic`">Conic</label>
        <NumberField :id="`ep-${name}-conic`" :aria-label="label('conic constant')" :model-value="modelValue.conic" @update:model-value="(v: number) => patch({ conic: v })" />
      </div>
      <div class="field">
        <label :for="`ep-${name}-aperture`">Clear radius (mm)</label>
        <NumberField :id="`ep-${name}-aperture`" min="0" :aria-label="label('clear radius')" :model-value="modelValue.aperture" @update:model-value="(v: number) => patch({ aperture: v })" />
        <small>Must be positive.</small>
      </div>
      <div class="field">
        <label class="inline"><input type="checkbox" :aria-label="label('clip rays outside the clear radius')" :checked="modelValue.clear_aperture" @change="patch({ clear_aperture: ($event.target as HTMLInputElement).checked })" /> Clip rays</label>
        <small>Off: the clear radius never clips.</small>
      </div>
      <div class="field">
        <label :for="`ep-${name}-draw`">Draw semi-diam. (mm)</label>
        <NumberField :id="`ep-${name}-draw`" readonly :aria-label="label('draw semi-diameter (derived, read-only)')" :model-value="modelValue.semi_diameter" />
        <small>Derived on Apply; never clips.</small>
      </div>
      <div class="field">
        <label :for="`ep-${name}-even`">Even A4, A6…</label>
        <NumberListField :id="`ep-${name}-even`" allow-empty :aria-label="label('even asphere coefficients')" :model-value="modelValue.even" @update:model-value="(v: number[]) => patch({ even: v })" />
      </div>
      <div class="field">
        <label :for="`ep-${name}-odd`">Odd A3, A4…</label>
        <NumberListField :id="`ep-${name}-odd`" allow-empty :aria-label="label('odd asphere coefficients')" :model-value="modelValue.odd" @update:model-value="(v: number[]) => patch({ odd: v })" />
      </div>
    </div>
    <small class="hint">{{ note }}</small>
    <small v-if="problem" class="warn-text" role="alert">{{ problem }}</small>
  </details>
</template>
