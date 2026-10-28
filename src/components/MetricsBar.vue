<script setup lang="ts">
import type { Paraxial, SpotDiagram } from '../types';

defineProps<{ paraxial: Paraxial; spot: SpotDiagram | null }>();

const fixed = (v: number | null, digits: number): string => (v === null ? '—' : v.toFixed(digits));
</script>

<template>
  <dl class="metrics" aria-label="First-order and spot metrics">
    <div><dt>Effective focal length</dt><dd>{{ fixed(paraxial.efl, 3) }} <small>mm</small></dd></div>
    <div><dt>Back focal length</dt><dd>{{ fixed(paraxial.bfl, 3) }} <small>mm</small></dd></div>
    <div><dt>F-number</dt><dd>ƒ / {{ fixed(paraxial.f_number, 2) }}</dd></div>
    <div><dt>RMS spot radius</dt><dd>{{ spot ? (spot.rms_radius * 1000).toFixed(2) : '—' }} <small>µm</small></dd></div>
  </dl>
</template>
