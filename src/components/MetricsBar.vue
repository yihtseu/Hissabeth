<script setup lang="ts">
import type { Paraxial, SpotDiagram } from '../types';

defineProps<{ paraxial: Paraxial; spot: SpotDiagram | null }>();

const fixed = (v: number | null, digits: number): string => (v === null ? '—' : v.toFixed(digits));
</script>

<template>
  <dl class="metrics" aria-label="First-order and spot metrics">
    <div><dt>Effective focal length</dt><dd>{{ fixed(paraxial.efl, 3) }} <small>mm</small></dd></div>
    <div><dt>Back focal length</dt><dd>{{ fixed(paraxial.bfl, 3) }} <small>mm</small></dd></div>
    <div><dt title="Paraxial 1/(2 |n_image × slope|)">Working f-number</dt><dd>ƒ / {{ fixed(paraxial.f_number, 2) }}</dd></div>
    <div><dt>Paraxial image NA</dt><dd>{{ fixed(paraxial.image_na, 3) }}</dd></div>
    <div><dt>Paraxial object NA <small>(n = {{ fixed(paraxial.object_index, 4) }})</small></dt><dd>{{ fixed(paraxial.object_na, 3) }}</dd></div>
    <div><dt>Front focal distance</dt><dd>{{ fixed(paraxial.front_focal_distance, 3) }} <small>mm</small></dd></div>
    <div><dt>RMS spot radius</dt><dd>{{ spot ? (spot.rms_radius * 1000).toFixed(2) : '—' }} <small>µm</small></dd></div>
  </dl>
</template>
