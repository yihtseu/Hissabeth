<script setup lang="ts">
import { tr } from '../i18n';
import { computed, onMounted, ref } from 'vue';
import { callTool, errorMessage } from '../api';
import type {
    CatalogImportResult,
    CatalogWarning,
    MaterialSearchResult,
    OpticalSystem,
} from '../types';
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
            query: query.value.trim(),
            offset: at,
            limit: LIMIT,
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
    if (f.total === 0) return tr('mats.noMatches');
    return tr('mats.pageRange', {
        from: offset.value + 1,
        to: Math.min(offset.value + LIMIT, f.total),
        total: f.total,
    });
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
    assigned.value = tr('mats.assigned', { n: i + 1, key: key });
}

// ---- index evaluation ---------------------------------------------------------
const material = ref('');
const evalWavelength = ref(
    props.modelValue.wavelengths[props.modelValue.reference_wavelength]?.value ?? 587.5618,
);
const indexResult = ref('');
const indexError = ref('');
async function evaluate(): Promise<void> {
    indexResult.value = '';
    indexError.value = '';
    if (!material.value.trim()) {
        indexError.value = tr('mats.enterMaterial');
        return;
    }
    if (!(evalWavelength.value >= 100 && evalWavelength.value <= 20000)) {
        indexError.value = tr('mats.wavelengthRange');
        return;
    }
    try {
        const r = await callTool('material_index', {
            material: material.value.trim(),
            wavelength: evalWavelength.value,
        });
        indexResult.value = tr('mats.indexResult', {
            wavelength: evalWavelength.value,
            index: r.index.toFixed(6),
        });
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
    !props.online
        ? tr('common.offline')
        : props.dirty
          ? tr('mats.blockDirty')
          : !importName.value.trim()
            ? tr('mats.blockName')
            : !importText.value.trim()
              ? tr('mats.blockText')
              : importName.value.trim().length > 128
                ? tr('mats.blockNameLong')
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
        importStatus.value =
            count === undefined
                ? tr('mats.imported', { name: name })
                : `Catalog ${name} imported: ${count} ${count === 1 ? 'glass' : 'glasses'}.`;
        importWarnings.value = done?.warnings ?? [];
        importText.value = '';
        void search(0);
    } else {
        importError.value = tr('mats.importFailed');
    }
}
</script>

<template>
    <section class="panel" aria-labelledby="mat-title">
        <div class="panel-head">
            <h3 id="mat-title">{{ tr('mats.title') }}</h3>
            <span>{{ tr('mats.subtitle') }}</span>
        </div>

        <form
            class="analysis-controls"
            role="search"
            :aria-label="tr('mats.searchLabel')"
            @submit.prevent="search(0)"
        >
            <div class="controls">
                <label
                    >{{ tr('mats.search') }}
                    <input
                        v-model="query"
                        type="search"
                        :placeholder="tr('mats.searchPlaceholder')"
                /></label>
                <label
                    >{{ tr('mats.catalog') }}
                    <input
                        v-model="catalog"
                        type="text"
                        :placeholder="tr('mats.catalogPlaceholder')"
                /></label>
                <button type="submit" class="primary" :disabled="!online || searching">
                    {{ tr('mats.search') }}
                </button>
            </div>
        </form>
        <div v-if="searchError" role="alert" class="banner error inset">{{ searchError }}</div>

        <details v-if="found && found.warnings.length" class="catalog-warnings">
            <summary>
                {{ found.warnings.length }} {{ tr('fragments.catalog') }}
                {{ found.warnings.length === 1 ? 'warning' : 'warnings' }}
            </summary>
            <ul>
                <li v-for="(w, i) in found.warnings" :key="i">{{ warningText(w) }}</li>
            </ul>
        </details>

        <div
            v-if="found"
            class="table-scroll"
            role="region"
            :aria-label="tr('remaining.m9')"
            tabindex="0"
        >
            <table>
                <caption class="sr-only">
                    {{
                        tr('fragments.glassSearchResults')
                    }}
                    {{
                        pageText
                    }}
                </caption>
                <thead>
                    <tr>
                        <th scope="col">{{ tr('mats.colMaterial') }}</th>
                        <th scope="col">{{ tr('mats.catalog') }}</th>
                        <th scope="col">nd</th>
                        <th scope="col">Vd</th>
                        <th scope="col">{{ tr('sys.colFormula') }}</th>
                        <th scope="col">{{ tr('mats.colRange') }}</th>
                        <th scope="col">
                            <span class="sr-only">{{ tr('common.actions') }}</span>
                        </th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="g in found.glasses" :key="g.key">
                        <th scope="row">{{ g.key }}</th>
                        <td>{{ g.catalog }}</td>
                        <td>{{ g.nd.toFixed(5) }}</td>
                        <td>{{ g.vd.toFixed(2) }}</td>
                        <td>{{ g.formula }}</td>
                        <td>
                            {{
                                g.wavelength_range
                                    ? `${fmt(g.wavelength_range[0])}–${fmt(g.wavelength_range[1])}`
                                    : tr('common.dash')
                            }}
                        </td>
                        <td class="row-actions">
                            <button
                                type="button"
                                class="small"
                                :aria-label="tr('mats.useAria', { key: g.key })"
                                @click="assign(g.key)"
                            >
                                {{ tr('mats.use') }}
                            </button>
                            <button
                                type="button"
                                class="small"
                                :aria-label="tr('mats.indexAria', { key: g.key })"
                                @click="material = g.key"
                            >
                                {{ tr('mats.indexButton') }}
                            </button>
                        </td>
                    </tr>
                    <tr v-if="!found.glasses.length">
                        <td colspan="7">{{ tr('mats.noGlasses') }}</td>
                    </tr>
                </tbody>
            </table>
        </div>
        <div v-if="found" class="legend pager">
            <span role="status">{{ pageText }}</span>
            <button
                type="button"
                class="small"
                :disabled="offset === 0 || searching"
                @click="search(Math.max(0, offset - LIMIT))"
            >
                {{ tr('mats.previous') }}
            </button>
            <button
                type="button"
                class="small"
                :disabled="lastPage || searching"
                @click="search(offset + LIMIT)"
            >
                {{ tr('mats.next') }}
            </button>
            <label class="inline"
                >{{ tr('mats.useOnSurface') }}
                <select v-model.number="target">
                    <option v-for="(_, i) in modelValue.surfaces" :key="i" :value="i">
                        {{ i + 1 }}
                    </option>
                </select>
            </label>
            <span v-if="assigned" role="status">{{ assigned }}</span>
        </div>

        <div class="subpanel">
            <h4>{{ tr('mats.refractiveIndex') }}</h4>
            <div class="controls">
                <label
                    >{{ tr('mats.colMaterial') }}
                    <input
                        v-model="material"
                        type="text"
                        :placeholder="tr('mats.materialPlaceholder')"
                /></label>
                <label
                    >{{ tr('mats.wavelengthNm') }}
                    <input
                        v-model.number="evalWavelength"
                        type="number"
                        min="100"
                        max="20000"
                        step="any"
                /></label>
                <button type="button" :disabled="!online" @click="evaluate">
                    {{ tr('mats.evaluate') }}
                </button>
                <output v-if="indexResult">{{ indexResult }}</output>
            </div>
            <p v-if="indexError" role="alert" class="json-error">{{ indexError }}</p>
        </div>

        <div class="subpanel">
            <h4>{{ tr('mats.importCatalog') }}</h4>
            <p class="note">
                {{ tr('mats.importNote') }}
            </p>
            <div class="controls">
                <label
                    >{{ tr('mats.catalogName') }}
                    <input v-model="importName" type="text" maxlength="128"
                /></label>
                <label
                    >{{ tr('mats.file') }}
                    <input type="file" accept=".agf,.txt,text/plain" @change="onFile"
                /></label>
            </div>
            <textarea
                v-model="importText"
                class="paste"
                rows="6"
                spellcheck="false"
                :aria-label="tr('mats.catalogText')"
            />
            <div class="controls">
                <button
                    type="button"
                    class="primary"
                    :disabled="!!importBlock || busy"
                    @click="doImport"
                >
                    {{ tr('mats.importCatalog') }}
                </button>
                <span v-if="importBlock" class="note">{{ importBlock }}</span>
                <span v-if="importStatus" role="status">{{ importStatus }}</span>
                <span v-if="importError" role="alert" class="json-error">{{ importError }}</span>
            </div>
            <details v-if="importWarnings.length" class="catalog-warnings" open>
                <summary>
                    {{ importWarnings.length }} {{ tr('fragments.import') }}
                    {{ importWarnings.length === 1 ? 'warning' : 'warnings' }}
                </summary>
                <ul>
                    <li v-for="(w, i) in importWarnings" :key="i">{{ warningText(w) }}</li>
                </ul>
            </details>
        </div>
    </section>
</template>
