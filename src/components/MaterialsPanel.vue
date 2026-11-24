<script setup lang="ts">
import { tr } from '../i18n';
import { computed, onMounted, ref } from 'vue';
import { callTool, errorMessage } from '../api';
import type { CatalogWarning, MaterialSearchResult, OpticalSystem } from '../types';
import { fmt } from '../chart';

const props = defineProps<{
    modelValue: OpticalSystem;
    online: boolean;
}>();

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
    </section>
</template>
