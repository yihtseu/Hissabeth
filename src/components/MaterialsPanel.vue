<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { callTool, errorMessage } from '../api';
import type { CatalogImportResult, CatalogWarning, MaterialSearchResult, OpticalSystem } from '../types';
import { fmt } from '../chart';

const props = defineProps<{
  modelValue: OpticalSystem;
  online: boolean;
  dirty: boolean;
  busy: boolean;
  /** Runs a backend operation that rewrites the saved system; resolves false when refused or failed. */
  mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: OpticalSystem] }>();

const LIMIT = 20;
const query = ref('');
const catalog = ref('');
const offset = ref(0);
const found = ref<MaterialSearchResult | null>(null);
const searchError = ref('');
const searching = ref(false);
let searchToken = 0;

async function search(at = 0): Promise<void> {
  if (!props.online) return;
  const mine = ++searchToken;
  searching.value = true;
  try {
    const args: { query: string; catalog?: string; offset: number; limit: number } = {
      query: query.value.trim(), offset: at, limit: LIMIT,
    };
    if (catalog.value.trim()) args.catalog = catalog.value.trim();
    const r = await callTool('material_search', args);
    if (mine !== searchToken) return;
    found.value = r;
    offset.value = at;
    searchError.value = '';
  } catch (e) {
    if (mine === searchToken) searchError.value = errorMessage(e);
  } finally {
    if (mine === searchToken) searching.value = false;
  }
}
onMounted(() => void search());

const lastPage = computed(() => (found.value ? offset.value + LIMIT >= found.value.total : true));
const pageText = computed(() => {
  const f = found.value;
  if (!f) return '';
  if (f.total === 0) return 'No matches';
  return `${offset.value + 1}–${Math.min(offset.value + LIMIT, f.total)} of ${f.total}`;
});

function warningText(w: CatalogWarning): string {
  return `${w.catalog}${w.glass ? ` / ${w.glass}` : ''}, line ${w.line}: ${w.message}`;
}

// ---- assign to a surface ---------------------------------------------------
const target = ref(0);
const assigned = ref('');
function assign(key: string): void {
  const i = Math.min(target.value, props.modelValue.surfaces.length - 1);
  emit('update:modelValue', {
    ...props.modelValue,
    surfaces: props.modelValue.surfaces.map((s, k) => (k === i ? { ...s, material: key } : s)),
  });
  assigned.value = `Surface ${i + 1} set to ${key} in the draft. Apply changes to save.`;
}

// ---- index evaluation ---------------------------------------------------------
const material = ref('');
const evalWavelength = ref(props.modelValue.wavelengths[props.modelValue.reference_wavelength]?.value ?? 587.5618);
const indexResult = ref('');
const indexError = ref('');
async function evaluate(): Promise<void> {
  indexResult.value = '';
  indexError.value = '';
  if (!material.value.trim()) { indexError.value = 'Enter a material name.'; return; }
  if (!(evalWavelength.value >= 100 && evalWavelength.value <= 20000)) { indexError.value = 'Wavelength must be 100–20000 nm.'; return; }
  try {
    const r = await callTool('material_index', { material: material.value.trim(), wavelength: evalWavelength.value });
    indexResult.value = `n(${evalWavelength.value} nm) = ${r.index.toFixed(6)}`;
  } catch (e) {
    indexError.value = errorMessage(e);
  }
}

// ---- catalog import --------------------------------------------------------------
const importName = ref('');
const importText = ref('');
const importStatus = ref('');
const importError = ref('');
const importWarnings = ref<CatalogWarning[]>([]);
const importBlock = computed(() =>
  !props.online ? 'Engine offline.'
  : props.dirty ? 'Apply or revert draft changes before importing.'
  : !importName.value.trim() ? 'Enter a catalog name.'
  : !importText.value.trim() ? 'Paste catalog text or choose a file.'
  : importName.value.trim().length > 128 ? 'Catalog name is longer than 128 characters.'
  : '',
);

async function onFile(e: Event): Promise<void> {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;
  importText.value = await file.text();
  if (!importName.value.trim()) importName.value = file.name.replace(/\.[^.]*$/, '');
  input.value = '';
}
async function doImport(): Promise<void> {
  importStatus.value = '';
  importError.value = '';
  importWarnings.value = [];
  const name = importName.value.trim();
  const text = importText.value;
  let imported: CatalogImportResult | null = null;
  const ok = await props.mutate(async (revision) => {
    imported = await callTool('catalog_import', { catalog: name, text, revision });
    return imported;
  });
  if (ok) {
    const done = imported as CatalogImportResult | null;
    const count = done?.glasses_imported;
    importStatus.value = count === undefined
      ? `Catalog ${name} imported.`
      : `Catalog ${name} imported: ${count} ${count === 1 ? 'glass' : 'glasses'}.`;
    importWarnings.value = done?.warnings ?? [];
    importText.value = '';
    void search(0);
  } else {
    importError.value = 'Import failed; see the error banner above.';
  }
}
</script>

<template>
  <section class="panel" aria-labelledby="mat-title">
    <div class="panel-head"><h3 id="mat-title">Material catalogs</h3><span>Glasses are addressed as NAME_CATALOG</span></div>

    <form class="analysis-controls" role="search" aria-label="Search glasses" @submit.prevent="search(0)">
      <div class="controls">
        <label>Search <input v-model="query" type="search" placeholder="name, e.g. BK7" /></label>
        <label>Catalog <input v-model="catalog" type="text" placeholder="all catalogs" /></label>
        <button type="submit" class="primary" :disabled="!online || searching">Search</button>
      </div>
    </form>
    <div v-if="searchError" role="alert" class="banner error inset">{{ searchError }}</div>

    <details v-if="found && found.warnings.length" class="catalog-warnings">
      <summary>{{ found.warnings.length }} catalog {{ found.warnings.length === 1 ? 'warning' : 'warnings' }}</summary>
      <ul>
        <li v-for="(w, i) in found.warnings" :key="i">{{ warningText(w) }}</li>
      </ul>
    </details>

    <div v-if="found" class="table-scroll" role="region" aria-label="Search results" tabindex="0">
      <table>
        <caption class="sr-only">Glass search results, {{ pageText }}</caption>
        <thead><tr>
          <th scope="col">Material</th><th scope="col">Catalog</th><th scope="col">nd</th><th scope="col">Vd</th>
          <th scope="col">Formula</th><th scope="col">Range (µm)</th><th scope="col"><span class="sr-only">Actions</span></th>
        </tr></thead>
        <tbody>
          <tr v-for="g in found.glasses" :key="g.key">
            <th scope="row">{{ g.key }}</th>
            <td>{{ g.catalog }}</td>
            <td>{{ g.nd.toFixed(5) }}</td>
            <td>{{ g.vd.toFixed(2) }}</td>
            <td>{{ g.formula }}</td>
            <td>{{ g.wavelength_range ? `${fmt(g.wavelength_range[0])}–${fmt(g.wavelength_range[1])}` : '—' }}</td>
            <td class="row-actions">
              <button type="button" class="small" :aria-label="`Use ${g.key} on the chosen surface`" @click="assign(g.key)">Use</button>
              <button type="button" class="small" :aria-label="`Evaluate index of ${g.key}`" @click="material = g.key">Index…</button>
            </td>
          </tr>
          <tr v-if="!found.glasses.length"><td colspan="7">No glasses match.</td></tr>
        </tbody>
      </table>
    </div>
    <div v-if="found" class="legend pager">
      <span role="status">{{ pageText }}</span>
      <button type="button" class="small" :disabled="offset === 0 || searching" @click="search(Math.max(0, offset - LIMIT))">Previous</button>
      <button type="button" class="small" :disabled="lastPage || searching" @click="search(offset + LIMIT)">Next</button>
      <label class="inline">Use on surface
        <select v-model.number="target">
          <option v-for="(_, i) in modelValue.surfaces" :key="i" :value="i">{{ i + 1 }}</option>
        </select>
      </label>
      <span v-if="assigned" role="status">{{ assigned }}</span>
    </div>

    <div class="subpanel">
      <h4>Refractive index</h4>
      <div class="controls">
        <label>Material <input v-model="material" type="text" placeholder="N-BK7_SCHOTT" /></label>
        <label>Wavelength (nm) <input v-model.number="evalWavelength" type="number" min="100" max="20000" step="any" /></label>
        <button type="button" :disabled="!online" @click="evaluate">Evaluate</button>
        <output v-if="indexResult">{{ indexResult }}</output>
      </div>
      <p v-if="indexError" role="alert" class="json-error">{{ indexError }}</p>
    </div>

    <div class="subpanel">
      <h4>Import catalog</h4>
      <p class="note">Paste Zemax AGF text or choose a file. Imported glasses are stored with the system as custom glasses.</p>
      <div class="controls">
        <label>Catalog name <input v-model="importName" type="text" maxlength="128" /></label>
        <label>File <input type="file" accept=".agf,.txt,text/plain" @change="onFile" /></label>
      </div>
      <textarea v-model="importText" class="paste" rows="6" spellcheck="false" aria-label="Catalog text" />
      <div class="controls">
        <button type="button" class="primary" :disabled="!!importBlock || busy" @click="doImport">Import catalog</button>
        <span v-if="importBlock" class="note">{{ importBlock }}</span>
        <span v-if="importStatus" role="status">{{ importStatus }}</span>
        <span v-if="importError" role="alert" class="json-error">{{ importError }}</span>
      </div>
      <details v-if="importWarnings.length" class="catalog-warnings" open>
        <summary>{{ importWarnings.length }} import {{ importWarnings.length === 1 ? 'warning' : 'warnings' }}</summary>
        <ul>
          <li v-for="(w, i) in importWarnings" :key="i">{{ warningText(w) }}</li>
        </ul>
      </details>
    </div>
  </section>
</template>
