<script setup lang="ts">
import { tr } from '../i18n';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { callTool, errorMessage, inDesktop } from '../api';
import type { CatalogWarning, MaterialSearchResult, OpticalSystem } from '../types';
import { selectedSurfaceIndex } from '../composables/useSurfaceSelection';
import { fmt } from '../chart';

const props = defineProps<{
    modelValue: OpticalSystem;
    online: boolean;
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

// Installing or removing a catalog changes what a search finds.
let stopCatalogs: UnlistenFn | undefined;
let unmounted = false;
onMounted(async () => {
    if (!inDesktop()) return;
    try {
        const stop = await listen('glass-catalogs-changed', () => void search(offset.value));
        if (unmounted) stop();
        else stopCatalogs = stop;
    } catch {
        // The list refreshes on the next search.
    }
});
onBeforeUnmount(() => {
    unmounted = true;
    stopCatalogs?.();
});
const catalogsError = ref('');
async function manageCatalogs(): Promise<void> {
    try {
        await invoke('open_glass_catalogs');
        catalogsError.value = '';
    } catch (e) {
        catalogsError.value = errorMessage(e);
    }
}

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

// ---- apply to the selected surface (draft only; the saved system is never written here) ----
const assigned = ref('');

const surfaces = computed(() => props.modelValue.surfaces);
const targetSurface = computed(() => {
    const i = selectedSurfaceIndex.value;
    if (i === null) return null;
    const s = surfaces.value[i];
    return s ? { index: i, surface: s } : null;
});
/** Only refracting rows take a material; mirrors and dummies keep the incident medium. */
const applyTarget = computed(() =>
    targetSurface.value && targetSurface.value.surface.interaction === 'refract'
        ? targetSurface.value
        : null,
);
/** Medium a reflect or dummy row retains: the material of the last refracting row above it, else the object medium. */
function incidentMedium(i: number): string {
    for (let k = i - 1; k >= 0; k--)
        if (surfaces.value[k]!.interaction === 'refract') return surfaces.value[k]!.material;
    return props.modelValue.object_material;
}
/** Why the chosen surface cannot take a glass right now; empty when a target is ready. */
const targetBlock = computed(() => {
    const t = targetSurface.value;
    if (!t) return tr('mats.selectSurface');
    if (t.surface.interaction === 'reflect')
        return tr('rx.mirrorKeeps', {
            medium: incidentMedium(t.index),
            material: t.surface.material,
        });
    if (t.surface.interaction === 'dummy')
        return tr('rx.dummyKeeps', {
            medium: incidentMedium(t.index),
            material: t.surface.material,
        });
    return '';
});
function assign(key: string): void {
    const t = applyTarget.value;
    if (!t) return;
    const next = surfaces.value.slice();
    next[t.index] = { ...t.surface, material: key };
    emit('update:modelValue', { ...props.modelValue, surfaces: next });
    assigned.value = tr('mats.assigned', { n: t.index + 1, key });
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
            <button v-if="inDesktop()" type="button" @click="manageCatalogs">
                {{ tr('mats.manageCatalogs') }}
            </button>
        </div>
        <div v-if="catalogsError" role="alert" class="banner error inset">{{ catalogsError }}</div>

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
        <p class="note">
            <template v-if="applyTarget"
                >{{ tr('mats.useOnSurface') }}: S{{ applyTarget.index + 1 }}</template
            >
            <template v-else>{{ targetBlock }}</template>
        </p>
        <p v-if="assigned" role="status" class="note">{{ assigned }}</p>

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
                                :disabled="!applyTarget"
                                :aria-label="
                                    applyTarget
                                        ? `${tr('common.apply')} S${applyTarget.index + 1}: ${g.key}`
                                        : tr('mats.useAria', { key: g.key })
                                "
                                @click="assign(g.key)"
                            >
                                {{
                                    applyTarget
                                        ? `${tr('common.apply')} S${applyTarget.index + 1}`
                                        : tr('mats.use')
                                }}
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
