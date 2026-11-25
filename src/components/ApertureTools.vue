<script setup lang="ts">
import { tr } from '../i18n';
import { computed, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import type { ApertureReport, OpticalSystem, VignettingUpdateResult } from '../types';
import { clone } from '../validation';

const props = defineProps<{
    /** The saved system: these actions and the coverage report act on it, never on an unapplied draft. */
    system: OpticalSystem;
    revision: number;
    online: boolean;
    /** Kept for template compatibility; pending drafts are chained in automatically. */
    dirty?: boolean;
    /** The saved view is outdated (changed elsewhere, or a committed change has not refreshed yet). */
    stale: boolean;
    busy: boolean;
    /** Chains a valid pending draft, runs the operation with the current revision, then reloads. */
    mutate: (op: (revision: number, system: OpticalSystem) => Promise<unknown>) => Promise<boolean>;
}>();

const DIRECTIONS = {
    get upper_y() {
        return tr('aperture.dirUpperY');
    },
    get lower_y() {
        return tr('aperture.dirLowerY');
    },
    get upper_x() {
        return tr('aperture.dirUpperX');
    },
    get lower_x() {
        return tr('aperture.dirLowerX');
    },
} as const;

const open = ref(false);
const report = ref<ApertureReport | null>(null);
const reportError = ref('');
const loading = ref(false);
const status = ref('');
/** Boundary summary of the last Auto vignetting run; hidden once another revision supersedes it. */
const vigResult = ref<VignettingUpdateResult | null>(null);
let reportToken = 0;

const block = computed(() => {
    if (!props.online) return tr('common.offline');
    if (props.stale) return tr('aperture.blockStale');
    return '';
});

/** Shown while its revision is the displayed one, or newer than a refresh that is still pending. */
const summary = computed(() =>
    vigResult.value && vigResult.value.revision >= props.revision ? vigResult.value : null,
);
const summaryPending = computed(() => !!summary.value && summary.value.revision > props.revision);

watch(
    () => props.revision,
    (r) => {
        if (vigResult.value && vigResult.value.revision < r) vigResult.value = null;
        status.value = '';
    },
);

/** Only a report for the displayed revision is ever shown, and never while offline or while the saved view is stale. */
const shown = computed(() =>
    props.online && !props.stale && report.value && report.value.revision === props.revision
        ? report.value
        : null,
);
const reportPending = computed(
    () => open.value && props.online && !props.stale && !shown.value && !reportError.value,
);

const surfaceLabel = (i: number | null): string => (i === null ? tr('common.dash') : String(i + 1));
const endpointName = (e: 'object' | 'image'): string =>
    e === 'object' ? tr('aperture.rowObject') : tr('aperture.rowImage');
/** Endpoints are named; a null surface is never shown as an internal surface number. */
const whereLabel = (endpoint: 'object' | 'image' | null, surface: number | null): string =>
    endpoint ? `${endpointName(endpoint)} surface` : surfaceLabel(surface);

async function loadReport(): Promise<void> {
    const token = ++reportToken;
    report.value = null;
    reportError.value = '';
    if (!props.online || props.stale) {
        loading.value = false;
        return;
    }
    loading.value = true;
    try {
        const r = await callTool('aperture_report', {});
        if (token !== reportToken) return;
        if (r.revision !== props.revision) {
            report.value = null;
            reportError.value = tr('aperture.revisionMismatch', {
                computed: r.revision,
                displayed: props.revision,
            });
        } else {
            report.value = r;
            reportError.value = '';
        }
    } catch (e) {
        if (token === reportToken) {
            report.value = null;
            reportError.value = errorMessage(e);
        }
    } finally {
        if (token === reportToken) loading.value = false;
    }
}

watch(
    [open, () => props.revision, () => props.online, () => props.stale],
    () => {
        if (open.value) void loadReport();
        else {
            reportToken++;
            report.value = null;
            reportError.value = '';
            loading.value = false;
        }
    },
    { immediate: true },
);

async function auto(): Promise<void> {
    status.value = '';
    let out: VignettingUpdateResult | null = null;
    const ok = await props.mutate(async (revision) => {
        out = await callTool('vignetting_update', { revision });
    });
    const result = out as VignettingUpdateResult | null;
    if (!ok || !result) return;
    vigResult.value = result;
    const limited = result.fields.reduce(
        (n, f) => n + f.boundaries.filter((b) => b.status === 'aperture').length,
        0,
    );
    status.value = `Revision ${result.revision}: vignetting updated in ${result.iterations} ${result.iterations === 1 ? 'iteration' : 'iterations'} (${limited} aperture-limited ${limited === 1 ? 'boundary' : 'boundaries'}).`;
}

async function clear(): Promise<void> {
    status.value = '';
    vigResult.value = null;
    const ok = await props.mutate(async (revision, system) => {
        const next = clone(system);
        for (const f of next.fields) f.vignetting = [0, 0, 0, 0];
        await callTool('system_set', { system: next, revision });
    });
    if (ok) status.value = tr('aperture.clearStatus');
}
</script>

<template>
    <section class="panel" aria-labelledby="aperture-title">
        <div class="panel-head">
            <h3 id="aperture-title">{{ tr('aperture.title') }}</h3>
            <span>{{ tr('common.savedSystem') }}</span>
        </div>
        <div class="subpanel">
            <h4>{{ tr('aperture.vignetting') }}</h4>
            <div class="controls">
                <button type="button" :disabled="!!block || busy" @click="auto">
                    {{ tr('aperture.auto') }}
                </button>
                <button type="button" :disabled="!!block || busy" @click="clear">
                    {{ tr('aperture.clear') }}
                </button>
            </div>
            <p class="note">
                {{ tr('aperture.note') }}
            </p>
            <p v-if="block" class="note">{{ block }}</p>
            <p v-if="status" role="status" class="note">{{ status }}</p>
            <div
                v-if="summary"
                class="table-scroll"
                role="region"
                aria-labelledby="vig-summary-title"
                tabindex="0"
            >
                <h4 id="vig-summary-title">
                    {{ tr('fragments.computedBoundariesRevision') }} {{ summary.revision }}, λ
                    {{ summary.wavelength }} {{ tr('fragments.nmTolerance') }}
                    {{ summary.tolerance }})
                </h4>
                <p v-if="summaryPending" class="note">
                    {{ tr('fragments.thePrescriptionShownIsStillRevision') }} {{ revision
                    }}{{ tr('fragments.itWillUpdateOnceTheViewRefreshes') }}
                </p>
                <table>
                    <caption class="sr-only">
                        {{
                            tr('aperture.boundariesCaption')
                        }}
                    </caption>
                    <thead>
                        <tr>
                            <th scope="col">{{ tr('aperture.colField') }}</th>
                            <th v-for="(label, d) in DIRECTIONS" :key="d" scope="col">
                                {{ label }} {{ tr('fragments.scaleLimit') }}
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="f in summary.fields" :key="f.field_index">
                            <th scope="row">
                                <span class="surface-index">{{ f.field_index + 1 }}</span>
                            </th>
                            <td v-for="(_label, d) in DIRECTIONS" :key="d">
                                <template
                                    v-for="b in f.boundaries.filter((x) => x.direction === d)"
                                    :key="b.direction"
                                >
                                    {{ b.pupil_scale.toFixed(4) }} ·
                                    {{
                                        b.status !== 'aperture'
                                            ? tr('aperture.unlimited')
                                            : b.limiting_endpoint
                                              ? whereLabel(b.limiting_endpoint, null).toLowerCase()
                                              : tr('aperture.limitSurface', {
                                                    n: surfaceLabel(b.limiting_surface),
                                                })
                                    }}
                                </template>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>
        <details
            class="data-table wide"
            :open="open"
            @toggle="open = ($event.target as HTMLDetailsElement).open"
        >
            <summary>{{ tr('aperture.coverage') }}</summary>
            <p class="note">
                {{ tr('aperture.coverageNote') }}
            </p>
            <p v-if="!online" class="note">{{ tr('aperture.coverageOffline') }}</p>
            <p v-else-if="stale" role="status" class="note">
                {{ tr('aperture.coverageStale') }}
            </p>
            <p v-else-if="reportPending" role="status" class="note">
                {{ tr('fragments.loadingCoverageForRevision') }} {{ revision }}…
            </p>
            <p v-if="reportError" role="alert" class="note warn-text">{{ reportError }}</p>
            <template v-if="shown">
                <p class="note">
                    {{ tr('fragments.revision') }} {{ shown.revision }} · λ
                    {{ shown.wavelength }} nm · {{ shown.sampling }} ·
                    {{ shown.sampled_rays - shown.failed_rays }} of {{ shown.sampled_rays }}
                    {{ tr('fragments.raysTracedCompletely')
                    }}<span v-if="shown.failed_rays"
                        >; {{ shown.failed_rays }}
                        {{
                            tr('fragments.failedPartialRaysCountOnlyForTheSurfacesTheyReached')
                        }}</span
                    >
                </p>
                <table>
                    <caption class="sr-only">
                        {{
                            tr('aperture.coverageCaption')
                        }}
                    </caption>
                    <thead>
                        <tr>
                            <th scope="col">{{ tr('aperture.colSurface') }}</th>
                            <th scope="col">{{ tr('aperture.colDraw') }}</th>
                            <th scope="col">{{ tr('aperture.colClear') }}</th>
                            <th scope="col">{{ tr('aperture.colClip') }}</th>
                            <th scope="col">
                                {{ tr('fragments.reachedRaysOf') }} {{ shown.sampled_rays }})
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr>
                            <th scope="row">{{ tr('aperture.rowObject') }}</th>
                            <td>{{ shown.object.semi_diameter }}</td>
                            <td>{{ shown.object.aperture }}</td>
                            <td>
                                {{
                                    shown.object.clear_aperture ? tr('common.yes') : tr('common.no')
                                }}
                            </td>
                            <td>{{ shown.object.reached_rays }} / {{ shown.sampled_rays }}</td>
                        </tr>
                        <tr v-for="s in shown.surfaces" :key="s.surface">
                            <th scope="row">
                                <span class="surface-index">{{ s.surface + 1 }}</span>
                            </th>
                            <td>{{ s.semi_diameter }}</td>
                            <td>{{ s.aperture }}</td>
                            <td>{{ s.clear_aperture ? tr('common.yes') : tr('common.no') }}</td>
                            <td>{{ s.reached_rays }} / {{ shown.sampled_rays }}</td>
                        </tr>
                        <tr>
                            <th scope="row">{{ tr('aperture.rowImage') }}</th>
                            <td>{{ shown.image.semi_diameter }}</td>
                            <td>{{ shown.image.aperture }}</td>
                            <td>
                                {{
                                    shown.image.clear_aperture ? tr('common.yes') : tr('common.no')
                                }}
                            </td>
                            <td>{{ shown.image.reached_rays }} / {{ shown.sampled_rays }}</td>
                        </tr>
                    </tbody>
                </table>
                <p v-if="!shown.failures.length" class="note">{{ tr('aperture.noFailures') }}</p>
                <template v-else>
                    <h4 id="ap-fail-title">
                        {{ tr('fragments.geometricFailures') }}{{ shown.failures.length }})
                    </h4>
                    <table aria-labelledby="ap-fail-title">
                        <caption class="sr-only">
                            {{
                                tr('aperture.failuresCaption')
                            }}
                        </caption>
                        <thead>
                            <tr>
                                <th scope="col">{{ tr('aperture.colField') }}</th>
                                <th scope="col">{{ tr('aperture.colPupil') }}</th>
                                <th scope="col">{{ tr('aperture.colFailedSurface') }}</th>
                                <th scope="col">{{ tr('aperture.colPrefix') }}</th>
                                <th scope="col">{{ tr('aperture.colReason') }}</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="(f, i) in shown.failures" :key="i">
                                <td>{{ f.field_index + 1 }}</td>
                                <td>({{ f.pupil[0] }}, {{ f.pupil[1] }})</td>
                                <td>{{ whereLabel(f.endpoint, f.surface) }}</td>
                                <td>
                                    {{
                                        f.reached_surfaces === 0
                                            ? tr('aperture.prefixNone')
                                            : tr('aperture.prefixSurfaces', {
                                                  n: f.reached_surfaces,
                                              })
                                    }}
                                </td>
                                <td>{{ f.kind }}: {{ f.error }}</td>
                            </tr>
                        </tbody>
                    </table>
                </template>
            </template>
        </details>
    </section>
</template>
