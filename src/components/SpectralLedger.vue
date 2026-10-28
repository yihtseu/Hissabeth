<script setup lang="ts">
import { computed } from 'vue';
import { fmt, wavelengthColor } from '../chart';
import { percent } from '../sampling';

export interface LedgerRow {
  wavelength: number;
  weight: number;
  transmitted: number;
  sampled: number;
  fraction: number;
  effective: number;
  /** Diffraction components only: PSF pixel pitch and sampling cutoff. */
  pixelSize?: number;
  cutoff?: number;
}

const props = defineProps<{ rows: LedgerRow[]; caption: string }>();
const hasCutoff = computed(() => props.rows.some((r) => r.cutoff !== undefined));
const totalEffective = computed(() => props.rows.reduce((a, r) => a + r.effective, 0));
</script>

<template>
  <table class="wide-table ledger">
    <caption>{{ caption }}</caption>
    <thead>
      <tr>
        <th scope="col">λ (nm)</th><th scope="col">Weight</th><th scope="col">Transmitted / sampled</th>
        <th scope="col">Transmission</th><th scope="col">Effective weight</th>
        <template v-if="hasCutoff"><th scope="col">Pixel (µm)</th><th scope="col">Cutoff (cycles/mm)</th></template>
      </tr>
    </thead>
    <tbody>
      <tr v-for="(r, i) in rows" :key="i">
        <td><svg width="10" height="10" aria-hidden="true"><circle cx="5" cy="5" r="4" :fill="wavelengthColor(r.wavelength)" /></svg> {{ r.wavelength }}</td>
        <td>{{ fmt(r.weight, 4) }}</td>
        <td>{{ r.transmitted }} / {{ r.sampled }}</td>
        <td>{{ percent(r.fraction) }}</td>
        <td>{{ fmt(r.effective, 4) }} <small>({{ totalEffective > 0 ? percent(r.effective / totalEffective) : '—' }})</small></td>
        <template v-if="hasCutoff"><td>{{ r.pixelSize === undefined ? '—' : fmt(r.pixelSize * 1000, 4) }}</td><td>{{ r.cutoff === undefined ? '—' : fmt(r.cutoff, 4) }}</td></template>
      </tr>
    </tbody>
  </table>
</template>
