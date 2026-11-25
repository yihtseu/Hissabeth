<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { GitCompare, RotateCcw, Undo2, Redo2 } from 'lucide-vue-next';
import { callTool, errorMessage } from '../api';
import { tr } from '../i18n';
import type { HistoryCompareResult, HistoryList } from '../types';

const props = defineProps<{
    revision: number;
    online: boolean;
    dirty: boolean;
    busy: boolean;
    stale: boolean;
    refreshPending: boolean;
    mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
}>();

const history = ref<HistoryList | null>(null);
const compare = ref<HistoryCompareResult | null>(null);
const fromIndex = ref<number>(0);
const toIndex = ref<number>(0);
const loading = ref(false);
const error = ref('');

const canMutate = computed(
    () => props.online && !props.dirty && !props.busy && !props.stale && !props.refreshPending,
);

async function refresh(): Promise<void> {
    if (!props.online) return;
    loading.value = true;
    error.value = '';
    try {
        const list = await callTool('history_list', {});
        history.value = list;
        toIndex.value = list.cursor;
        fromIndex.value = Math.max(0, list.cursor - 1);
        if (list.entries.length >= 2) {
            await runCompare();
        } else {
            compare.value = null;
        }
    } catch (e) {
        error.value = errorMessage(e);
    } finally {
        loading.value = false;
    }
}

async function runCompare(): Promise<void> {
    if (!props.online || !history.value) return;
    error.value = '';
    try {
        compare.value = await callTool('history_compare', {
            from_index: fromIndex.value,
            to_index: toIndex.value,
        });
    } catch (e) {
        error.value = errorMessage(e);
    }
}

async function restore(index: number): Promise<void> {
    if (!canMutate.value) return;
    await props.mutate((rev) => callTool('history_restore', { index, revision: rev }));
}

async function doUndo(): Promise<void> {
    if (!canMutate.value || !history.value?.can_undo) return;
    await props.mutate((rev) => callTool('history_undo', { revision: rev }));
}

async function doRedo(): Promise<void> {
    if (!canMutate.value || !history.value?.can_redo) return;
    await props.mutate((rev) => callTool('history_redo', { revision: rev }));
}

watch(
    () => [props.revision, props.online],
    () => {
        void refresh();
    },
    { immediate: true },
);

const fmtTime = (ms: number) => new Date(ms).toLocaleTimeString();
const fmt = (n: number | null, digits = 4) => (n === null ? '—' : n.toFixed(digits));
const fmtSigned = (n: number | null, digits = 4) =>
    n === null ? '—' : `${n >= 0 ? '+' : ''}${n.toFixed(digits)}`;
</script>

<template>
    <section class="panel" aria-labelledby="hist-title">
        <div class="panel-head">
            <h3 id="hist-title">{{ tr('hist.title') }}</h3>
            <span>{{ tr('hist.subtitle') }}</span>
        </div>

        <div class="analysis-controls">
            <div class="controls">
                <button type="button" :disabled="!canMutate || !history?.can_undo" @click="doUndo">
                    <Undo2 :size="14" /> {{ tr('hist.undo') }}
                </button>
                <button type="button" :disabled="!canMutate || !history?.can_redo" @click="doRedo">
                    <Redo2 :size="14" /> {{ tr('hist.redo') }}
                </button>
                <div class="spacer"></div>
                <template v-if="history && history.entries.length >= 2">
                    <label>
                        {{ tr('hist.from') }}
                        <select v-model.number="fromIndex">
                            <option v-for="e in history.entries" :key="e.index" :value="e.index">
                                #{{ e.index + 1 }} · {{ e.action }} (r{{ e.revision }})
                            </option>
                        </select>
                    </label>
                    <label>
                        {{ tr('hist.to') }}
                        <select v-model.number="toIndex">
                            <option v-for="e in history.entries" :key="e.index" :value="e.index">
                                #{{ e.index + 1 }} · {{ e.action }} (r{{ e.revision }})
                            </option>
                        </select>
                    </label>
                    <button type="button" class="primary" @click="runCompare">
                        <GitCompare :size="14" /> {{ tr('hist.compare') }}
                    </button>
                </template>
            </div>
            <p v-if="dirty" class="note warn-text">{{ tr('opt.dirtyHint') }}</p>
            <p v-if="error" role="alert" class="json-error">{{ error }}</p>
        </div>

        <div v-if="compare" class="subpanel">
            <h4>
                {{ tr('hist.diffHeading') }}: #{{ compare.from.index + 1 }} ({{
                    compare.from.action
                }}) → #{{ compare.to.index + 1 }} ({{ compare.to.action }})
            </h4>
            <dl class="metrics">
                <div>
                    <dt>EFL</dt>
                    <dd>
                        {{ fmt(compare.to.metrics.efl, 3) }} mm
                        <small class="delta-pill">{{ fmtSigned(compare.deltas.efl, 3) }}</small>
                    </dd>
                </div>
                <div>
                    <dt>BFL</dt>
                    <dd>
                        {{ fmt(compare.to.metrics.bfl, 3) }} mm
                        <small class="delta-pill">{{ fmtSigned(compare.deltas.bfl, 3) }}</small>
                    </dd>
                </div>
                <div>
                    <dt>F/#</dt>
                    <dd>
                        {{ fmt(compare.to.metrics.f_number, 2) }}
                        <small class="delta-pill">{{
                            fmtSigned(compare.deltas.f_number, 2)
                        }}</small>
                    </dd>
                </div>
                <div v-for="(spot, fi) in compare.to.metrics.rms_spot_mm" :key="fi">
                    <dt>RMS Spot F{{ fi + 1 }}</dt>
                    <dd>
                        {{ spot !== null ? (spot * 1000).toFixed(2) : '—' }} µm
                        <small
                            v-if="
                                compare.deltas.rms_spot_mm[fi] !== null &&
                                compare.deltas.rms_spot_mm[fi] !== undefined
                            "
                            class="delta-pill"
                            :class="
                                (compare.deltas.rms_spot_mm[fi] ?? 0) <= 0
                                    ? 'delta-good'
                                    : 'delta-bad'
                            "
                        >
                            {{ fmtSigned((compare.deltas.rms_spot_mm[fi] ?? 0) * 1000, 2) }}
                            µm
                        </small>
                    </dd>
                </div>
            </dl>

            <div v-if="compare.surface_changes.length" class="table-scroll">
                <h4>{{ tr('hist.surfaceChanges') }}</h4>
                <table>
                    <thead>
                        <tr>
                            <th>{{ tr('opt.surface') }}</th>
                            <th>{{ tr('opt.status') }}</th>
                            <th>{{ tr('hist.details') }}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="sc in compare.surface_changes" :key="sc.surface">
                            <td>S{{ sc.surface + 1 }}</td>
                            <td>
                                <span
                                    class="badge"
                                    :class="
                                        sc.status === 'added'
                                            ? 'badge-ok'
                                            : sc.status === 'removed'
                                              ? 'badge-bad'
                                              : 'badge-info'
                                    "
                                    >{{ sc.status }}</span
                                >
                            </td>
                            <td>
                                <template v-if="sc.changes">
                                    <span v-for="(c, ci) in sc.changes" :key="ci" class="diff-chip">
                                        <strong>{{ c.field }}:</strong>
                                        {{ JSON.stringify(c.before) }} →
                                        {{ JSON.stringify(c.after) }}
                                    </span>
                                </template>
                                <span v-else>{{ sc.label || '—' }}</span>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
            <p v-else class="note">{{ tr('hist.noSurfaceChanges') }}</p>
        </div>

        <div class="subpanel">
            <h4>{{ tr('hist.timeline') }}</h4>
            <p v-if="loading && !history" class="note">{{ tr('app.loadingSystem') }}</p>
            <div v-else-if="history" class="table-scroll">
                <table>
                    <thead>
                        <tr>
                            <th>#</th>
                            <th>{{ tr('hist.action') }}</th>
                            <th>{{ tr('hist.revision') }}</th>
                            <th>{{ tr('hist.system') }}</th>
                            <th>{{ tr('hist.time') }}</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr
                            v-for="e in [...history.entries].reverse()"
                            :key="e.index"
                            :class="{ 'row-active': e.current }"
                        >
                            <td>#{{ e.index + 1 }}</td>
                            <td>
                                <span class="badge" :class="e.current ? 'badge-ok' : 'badge-info'">
                                    {{ e.action }}
                                </span>
                            </td>
                            <td>r{{ e.revision }}</td>
                            <td>{{ e.name }} ({{ e.surfaces }} S)</td>
                            <td>{{ fmtTime(e.timestamp_ms) }}</td>
                            <td class="row-actions">
                                <button
                                    v-if="!e.current"
                                    type="button"
                                    class="small"
                                    :disabled="!canMutate"
                                    @click="restore(e.index)"
                                >
                                    <RotateCcw :size="12" /> {{ tr('hist.restore') }}
                                </button>
                                <span v-else class="note">{{ tr('hist.current') }}</span>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>
    </section>
</template>
