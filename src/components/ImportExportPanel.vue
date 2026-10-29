<script setup lang="ts">
import { computed, ref } from 'vue';
import { callTool, errorMessage } from '../api';
import type { ImportFormat, OpticalSystem } from '../types';

const props = defineProps<{
  system: OpticalSystem;
  online: boolean;
  dirty: boolean;
  busy: boolean;
  mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
}>();

const FORMATS: { id: ImportFormat; label: string }[] = [
  { id: 'hissabeth', label: 'Hissabeth project' },
  { id: 'geopter', label: 'Geopter' },
];

const format = ref<ImportFormat>('hissabeth');
const text = ref('');
const status = ref('');
const error = ref('');
const working = ref(false);

/** Stored conditions the Geopter format cannot hold; the engine still reports anything else it cannot represent. */
const geopterBlock = computed(() => {
  const sys = props.system;
  if (sys.pupil_mapping === 'real_stop')
    return 'Geopter always uses the reference entrance pupil mapping. Apply “Reference entrance pupil” as the pupil ray mapping, or export the Hissabeth project format.';
  if (sys.object_distance === null) return 'Geopter needs a finite object distance. Use the Hissabeth project format for an object at infinity.';
  if (sys.custom_glasses.length > 0) return 'Geopter cannot store custom glass catalogs. Use the Hissabeth project format.';
  const builtIn = (where: string): string => `${where} is FUSED_SILICA, a built-in material that cannot be stored in a Geopter project. Use the Hissabeth project format.`;
  if (sys.object_material === 'FUSED_SILICA') return builtIn('Object material');
  const silica = sys.surfaces.findIndex((x) => x.material === 'FUSED_SILICA');
  if (silica >= 0) return builtIn(`Surface ${silica + 1} material`);
  if (sys.temperature !== 25 || sys.pressure !== 101325) return 'Geopter needs the standard environment: set temperature to 25 °C and pressure to 101325 Pa, or use the Hissabeth project format.';
  const nonRefract = sys.surfaces.findIndex((x) => x.interaction !== 'refract');
  if (nonRefract >= 0) return `Geopter supports refracting surfaces only: set surface ${nonRefract + 1} to refract, or use the Hissabeth project format.`;
  const mixed = sys.surfaces.findIndex((x) => x.even.length > 0 && x.odd.length > 0);
  if (mixed >= 0) return `Geopter cannot store both even and odd asphere coefficients on one surface: clear one list on surface ${mixed + 1}, or use the Hissabeth project format.`;
  for (const [name, e] of [['object', sys.object_surface], ['image', sys.image_surface]] as const)
    if (e.even.length > 0 && e.odd.length > 0)
      return `Geopter cannot store both even and odd asphere coefficients on one surface: clear one list on the ${name} surface, or use the Hissabeth project format.`;
  // Geopter stores exactly ten even or odd coefficients per profile; shorter lists are padded with zeros on export.
  const profiles = [
    { where: 'Object', e: sys.object_surface },
    ...sys.surfaces.map((x, i) => ({ where: `Surface ${i + 1}`, e: x })),
    { where: 'Image', e: sys.image_surface },
  ];
  for (const { where, e } of profiles)
    if (e.even.length > 10 || e.odd.length > 10)
      return `${where} has more than 10 ${e.even.length > 10 ? 'even' : 'odd'} asphere coefficients; Geopter stores at most 10. Remove the higher terms, or use the Hissabeth project format.`;
  if (sys.surfaces.some((x) => x.gap_solve.kind !== 'fixed')) return 'Geopter does not store gap solves. Use the Hissabeth project format.';
  return '';
});
const exportBlock = computed(() =>
  !props.online ? 'Engine offline.'
  : format.value === 'geopter' ? geopterBlock.value
  : '',
);
const importBlock = computed(() =>
  !props.online ? 'Engine offline.'
  : props.dirty ? 'Apply or revert draft changes before importing.'
  : !text.value.trim() ? 'Paste project text or choose a file.'
  : '',
);

function fileName(): string {
  const base = props.system.name.trim().replace(/[^\w.-]+/g, '_').replace(/^_+|_+$/g, '') || 'system';
  return `${base}.${format.value}.json`;
}

async function doExport(): Promise<void> {
  status.value = '';
  error.value = '';
  working.value = true;
  try {
    const r = await callTool('project_export', { format: format.value });
    text.value = r.text;
    status.value = `Exported saved system as ${format.value} (${r.text.length.toLocaleString()} characters).`;
  } catch (e) {
    error.value = errorMessage(e);
  } finally {
    working.value = false;
  }
}

function download(): void {
  const url = URL.createObjectURL(new Blob([text.value], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = fileName();
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
  status.value = `Saved ${fileName()}.`;
  error.value = '';
}

async function onFile(e: Event): Promise<void> {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;
  try {
    text.value = await file.text();
    status.value = `Loaded ${file.name}. Review the text, then import.`;
    error.value = '';
  } catch (err) {
    error.value = errorMessage(err);
  }
  input.value = '';
}

async function doImport(): Promise<void> {
  status.value = '';
  error.value = '';
  const body = text.value;
  const f = format.value;
  const ok = await props.mutate((revision) => callTool('project_import', { format: f, text: body, revision }));
  if (ok) status.value = `Imported ${f} project; the saved system was replaced.`;
  else error.value = 'Import failed; see the error banner above.';
}
</script>

<template>
  <section class="panel" aria-labelledby="ie-title">
    <div class="panel-head"><h3 id="ie-title">Import / export</h3><span>Exports use the saved system</span></div>
    <div class="analysis-controls">
      <div class="controls">
        <label>Format
          <select v-model="format">
            <option v-for="f in FORMATS" :key="f.id" :value="f.id">{{ f.label }}</option>
          </select>
        </label>
        <label>Open file <input type="file" accept=".json,.txt,application/json,text/plain" @change="onFile" /></label>
      </div>
    </div>
    <div class="subpanel">
      <label for="ie-text" class="block-label">Project text</label>
      <textarea id="ie-text" v-model="text" class="paste tall" spellcheck="false" aria-describedby="ie-status ie-error" />
      <div class="controls">
        <button type="button" :disabled="!!exportBlock || working" @click="doExport">Export saved system</button>
        <button type="button" :disabled="!text" @click="download">Download file</button>
        <button type="button" class="primary" :disabled="!!importBlock || busy" @click="doImport">Import (replaces saved system)</button>
      </div>
      <p v-if="format === 'geopter'" class="note">Geopter files keep the object medium, endpoint labels, endpoint profiles and apertures, and field and wavelength colors, but need a finite object distance, the Reference entrance pupil ray mapping and the standard environment, and cannot store gap solves or custom catalogs. Use the Hissabeth project format to keep those.</p>
      <p v-if="format === 'geopter' && online && exportBlock" class="note">{{ exportBlock }}</p>
      <p v-if="importBlock" class="note">{{ importBlock }}</p>
      <p id="ie-status" role="status" class="note">{{ status }}</p>
      <p id="ie-error" role="alert" class="json-error">{{ error }}</p>
    </div>
  </section>
</template>
