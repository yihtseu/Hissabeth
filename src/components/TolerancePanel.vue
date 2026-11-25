<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { Activity, Dice5, Plus, Sparkles, Trash2 } from 'lucide-vue-next';
import { callTool, errorMessage } from '../api';
import { tr } from '../i18n';
import type {
    OpticalSystem,
    ToleranceCompensator,
    ToleranceCriterion,
    ToleranceMonteCarloResult,
    ToleranceOperand,
    ToleranceSensitivityResult,
} from '../types';

const props = defineProps<{
    system: OpticalSystem;
    revision: number;
    online: boolean;
}>();

interface TolRow {
    id: number;
    kind: ToleranceOperand['kind'];
    surface: number;
    lastSurface: number;
    delta: number;
}

let nextId = 1;
const rows = ref<TolRow[]>([]);
const compKind = ref<'none' | 'autofocus'>('autofocus');
const compMaxDelta = ref('2.0');
const critKind = ref<ToleranceCriterion['kind']>('rms_spot');
const critField = ref(-1);
const critFreq = ref(50);
const trials = ref(100);
const distribution = ref<'normal' | 'uniform'>('normal');
const seed = ref(42);
const passThreshold = ref('');

const runningSens = ref(false);
const runningMc = ref(false);
const error = ref('');
const sensResult = ref<ToleranceSensitivityResult | null>(null);
const mcResult = ref<ToleranceMonteCarloResult | null>(null);

function generateDefaults(): void {
    const out: TolRow[] = [];
    for (let i = 0; i < props.system.surfaces.length; i++) {
        const s = props.system.surfaces[i]!;
        if (s.interaction === 'dummy') continue;
        if (s.radius !== 0) {
            out.push({ id: nextId++, kind: 'radius', surface: i, lastSurface: i, delta: 0.5 });
        }
        out.push({ id: nextId++, kind: 'thickness', surface: i, lastSurface: i, delta: 0.05 });
        out.push({ id: nextId++, kind: 'decenter', surface: i, lastSurface: i, delta: 0.05 });
        out.push({ id: nextId++, kind: 'tilt', surface: i, lastSurface: i, delta: 0.05 });
        if (s.material !== 'AIR' && s.interaction === 'refract') {
            out.push({ id: nextId++, kind: 'index', surface: i, lastSurface: i, delta: 0.001 });
        }
    }
    rows.value = out;
    error.value = '';
}

function addRow(): void {
    rows.value.push({ id: nextId++, kind: 'thickness', surface: 0, lastSurface: 0, delta: 0.05 });
}

function buildTolerances(): ToleranceOperand[] {
    return rows.value.map((r) => {
        switch (r.kind) {
            case 'radius':
                return { kind: 'radius', surface: r.surface, delta_percent: r.delta };
            case 'thickness':
                return { kind: 'thickness', surface: r.surface, delta: r.delta };
            case 'decenter':
                return { kind: 'decenter', surface: r.surface, delta: r.delta };
            case 'tilt':
                return { kind: 'tilt', surface: r.surface, delta_deg: r.delta };
            case 'index':
                return { kind: 'index', surface: r.surface, delta: r.delta };
            case 'abbe':
                return { kind: 'abbe', surface: r.surface, delta_percent: r.delta };
            case 'element_decenter':
                return {
                    kind: 'element_decenter',
                    first: Math.min(r.surface, r.lastSurface),
                    last: Math.max(r.surface, r.lastSurface),
                    delta: r.delta,
                };
            case 'element_tilt':
                return {
                    kind: 'element_tilt',
                    first: Math.min(r.surface, r.lastSurface),
                    last: Math.max(r.surface, r.lastSurface),
                    delta_deg: r.delta,
                };
        }
    });
}

function buildCompensator(): ToleranceCompensator {
    if (compKind.value === 'none') return { kind: 'none' };
    const parsed = Number(compMaxDelta.value.trim());
    return {
        kind: 'autofocus',
        max_delta: compMaxDelta.value.trim() && Number.isFinite(parsed) ? parsed : null,
    };
}

function buildCriterion(): ToleranceCriterion {
    switch (critKind.value) {
        case 'rms_spot':
            return { kind: 'rms_spot', field_index: critField.value < 0 ? null : critField.value };
        case 'rms_wavefront':
            return { kind: 'rms_wavefront', field_index: Math.max(0, critField.value) };
        case 'efl':
            return { kind: 'efl' };
        case 'geometric_mtf':
            return {
                kind: 'geometric_mtf',
                field_index: Math.max(0, critField.value),
                frequency: critFreq.value,
                axis: 'y',
            };
    }
}

async function runSensitivity(): Promise<void> {
    if (!props.online || rows.value.length === 0) return;
    error.value = '';
    runningSens.value = true;
    try {
        sensResult.value = await callTool('tolerance_sensitivity', {
            tolerances: buildTolerances(),
            compensator: buildCompensator(),
            criterion: buildCriterion(),
        });
    } catch (e) {
        error.value = errorMessage(e);
    } finally {
        runningSens.value = false;
    }
}

async function runMonteCarlo(): Promise<void> {
    if (!props.online || rows.value.length === 0) return;
    error.value = '';
    runningMc.value = true;
    try {
        const thr = passThreshold.value.trim() ? Number(passThreshold.value.trim()) : null;
        mcResult.value = await callTool('tolerance_monte_carlo', {
            tolerances: buildTolerances(),
            compensator: buildCompensator(),
            criterion: buildCriterion(),
            trials: trials.value,
            distribution: distribution.value,
            seed: seed.value,
            pass_threshold: thr !== null && Number.isFinite(thr) ? thr : null,
        });
    } catch (e) {
        error.value = errorMessage(e);
    } finally {
        runningMc.value = false;
    }
}

watch(
    () => props.system.surfaces.length,
    () => {
        const maxS = Math.max(0, props.system.surfaces.length - 1);
        for (const r of rows.value) {
            if (r.surface > maxS) r.surface = maxS;
            if (r.lastSurface > maxS) r.lastSurface = maxS;
        }
        if (rows.value.length === 0) generateDefaults();
    },
    { immediate: true },
);

watch(
    () => props.revision,
    () => {
        sensResult.value = null;
        mcResult.value = null;
    },
);

const maxWorst = computed(() =>
    Math.max(...(sensResult.value?.entries.map((e) => e.worst_change) ?? [0]), 1e-12),
);
const maxBin = computed(() =>
    Math.max(...(mcResult.value?.histogram.map((b) => b.count) ?? [1]), 1),
);
const fmt = (n: number | null) =>
    n === null ? '—' : Math.abs(n) >= 1e-3 || n === 0 ? n.toFixed(5) : n.toExponential(3);
const unitForKind = (k: ToleranceOperand['kind']) =>
    k === 'radius' || k === 'abbe'
        ? '%'
        : k === 'tilt' || k === 'element_tilt'
          ? '°'
          : k === 'index'
            ? 'Δn'
            : 'mm';
</script>

<template>
    <section class="panel" aria-labelledby="tol-title">
        <div class="panel-head">
            <h3 id="tol-title">{{ tr('tol.title') }}</h3>
            <span>{{ tr('tol.subtitle') }}</span>
        </div>

        <div class="analysis-controls">
            <div class="controls">
                <button type="button" @click="generateDefaults">
                    <Sparkles :size="14" /> {{ tr('tol.defaults') }}
                </button>
                <label>
                    {{ tr('tol.criterion') }}
                    <select v-model="critKind">
                        <option value="rms_spot">{{ tr('opt.opKind.rms_spot') }}</option>
                        <option value="rms_wavefront">{{ tr('opt.opKind.rms_wavefront') }}</option>
                        <option value="efl">{{ tr('opt.opKind.efl') }}</option>
                        <option value="geometric_mtf">MTF</option>
                    </select>
                </label>
                <label v-if="critKind !== 'efl'">
                    {{ tr('tol.field') }}
                    <select v-model.number="critField">
                        <option v-if="critKind === 'rms_spot'" :value="-1">
                            {{ tr('opt.allFields') }}
                        </option>
                        <option v-for="(_, fi) in system.fields" :key="fi" :value="fi">
                            F{{ fi + 1 }}
                        </option>
                    </select>
                </label>
                <label v-if="critKind === 'geometric_mtf'">
                    {{ tr('tol.freq') }}
                    <input v-model.number="critFreq" type="number" min="1" max="1000" />
                </label>
                <label>
                    {{ tr('tol.compensator') }}
                    <select v-model="compKind">
                        <option value="autofocus">{{ tr('tol.compAutofocus') }}</option>
                        <option value="none">{{ tr('tol.compNone') }}</option>
                    </select>
                </label>
                <label v-if="compKind === 'autofocus'">
                    {{ tr('tol.maxStroke') }}
                    <input v-model="compMaxDelta" type="text" placeholder="±mm" />
                </label>
            </div>
            <div class="controls" style="margin-top: 8px">
                <label>
                    {{ tr('tol.trials') }}
                    <input v-model.number="trials" type="number" min="10" max="2000" step="10" />
                </label>
                <label>
                    {{ tr('tol.distribution') }}
                    <select v-model="distribution">
                        <option value="normal">{{ tr('tol.distNormal') }}</option>
                        <option value="uniform">{{ tr('tol.distUniform') }}</option>
                    </select>
                </label>
                <label>
                    {{ tr('tol.seed') }}
                    <input v-model.number="seed" type="number" min="0" />
                </label>
                <label>
                    {{ tr('tol.threshold') }}
                    <input v-model="passThreshold" type="text" :placeholder="tr('tol.optional')" />
                </label>
                <div class="spacer"></div>
                <button
                    type="button"
                    :disabled="!online || runningSens || rows.length === 0"
                    @click="runSensitivity"
                >
                    <Activity :size="14" />
                    {{ runningSens ? tr('tol.running') : tr('tol.runSens') }}
                </button>
                <button
                    type="button"
                    class="primary"
                    :disabled="!online || runningMc || rows.length === 0"
                    @click="runMonteCarlo"
                >
                    <Dice5 :size="14" />
                    {{ runningMc ? tr('tol.running') : tr('tol.runMc') }}
                </button>
            </div>
            <p v-if="error" role="alert" class="json-error">{{ error }}</p>
        </div>

        <div class="subpanel">
            <div class="panel-head flush">
                <h4>{{ tr('tol.operands') }} ({{ rows.length }})</h4>
                <button type="button" class="small" @click="addRow">
                    <Plus :size="13" /> {{ tr('tol.add') }}
                </button>
            </div>
            <div class="table-scroll">
                <table>
                    <thead>
                        <tr>
                            <th>{{ tr('opt.kind') }}</th>
                            <th>{{ tr('opt.surface') }}</th>
                            <th>{{ tr('tol.delta') }}</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="(r, idx) in rows" :key="r.id">
                            <td>
                                <select v-model="r.kind">
                                    <option value="radius">{{ tr('tol.kind.radius') }}</option>
                                    <option value="thickness">
                                        {{ tr('tol.kind.thickness') }}
                                    </option>
                                    <option value="decenter">{{ tr('tol.kind.decenter') }}</option>
                                    <option value="tilt">{{ tr('tol.kind.tilt') }}</option>
                                    <option value="index">{{ tr('tol.kind.index') }}</option>
                                    <option value="abbe">{{ tr('tol.kind.abbe') }}</option>
                                    <option value="element_decenter">
                                        {{ tr('tol.kind.element_decenter') }}
                                    </option>
                                    <option value="element_tilt">
                                        {{ tr('tol.kind.element_tilt') }}
                                    </option>
                                </select>
                            </td>
                            <td>
                                <div class="controls">
                                    <select v-model.number="r.surface">
                                        <option
                                            v-for="(_, si) in system.surfaces"
                                            :key="si"
                                            :value="si"
                                        >
                                            S{{ si + 1 }}
                                        </option>
                                    </select>
                                    <select
                                        v-if="
                                            r.kind === 'element_decenter' ||
                                            r.kind === 'element_tilt'
                                        "
                                        v-model.number="r.lastSurface"
                                    >
                                        <option
                                            v-for="(_, si) in system.surfaces"
                                            :key="si"
                                            :value="si"
                                        >
                                            S{{ si + 1 }}
                                        </option>
                                    </select>
                                </div>
                            </td>
                            <td>
                                <div class="controls">
                                    <input
                                        v-model.number="r.delta"
                                        type="number"
                                        min="0.00001"
                                        step="any"
                                    />
                                    <span class="note">± {{ unitForKind(r.kind) }}</span>
                                </div>
                            </td>
                            <td>
                                <button
                                    type="button"
                                    class="small"
                                    :aria-label="tr('opt.remove')"
                                    @click="rows.splice(idx, 1)"
                                >
                                    <Trash2 :size="12" />
                                </button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>

        <div v-if="sensResult" class="subpanel">
            <h4>{{ tr('tol.sensResults') }}</h4>
            <dl class="metrics">
                <div>
                    <dt>{{ tr('tol.nominal') }}</dt>
                    <dd>
                        {{ fmt(sensResult.nominal) }} <small>{{ sensResult.criterion_unit }}</small>
                    </dd>
                </div>
                <div>
                    <dt>{{ tr('tol.rssChange') }}</dt>
                    <dd>
                        ±{{ fmt(sensResult.rss_change) }}
                        <small>{{ sensResult.criterion_unit }}</small>
                    </dd>
                </div>
                <div>
                    <dt>{{ tr('tol.estimated') }}</dt>
                    <dd>
                        {{ fmt(sensResult.estimated_performance) }}
                        <small>{{ sensResult.criterion_unit }}</small>
                    </dd>
                </div>
            </dl>
            <div class="table-scroll">
                <table>
                    <thead>
                        <tr>
                            <th>{{ tr('tol.item') }}</th>
                            <th>{{ tr('tol.axis') }}</th>
                            <th>+Δ</th>
                            <th>−Δ</th>
                            <th>{{ tr('tol.worst') }}</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="(e, i) in sensResult.entries" :key="i">
                            <td>{{ e.label }}</td>
                            <td>{{ e.axis }}</td>
                            <td>{{ e.plus_error ? 'ERR' : fmt(e.plus_change) }}</td>
                            <td>{{ e.minus_error ? 'ERR' : fmt(e.minus_change) }}</td>
                            <td>
                                <div class="bar-cell">
                                    <span>{{ fmt(e.worst_change) }}</span>
                                    <div class="bar-track">
                                        <div
                                            class="bar-fill"
                                            :style="{
                                                width: `${Math.min(100, (e.worst_change / maxWorst) * 100)}%`,
                                            }"
                                        ></div>
                                    </div>
                                </div>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>

        <div v-if="mcResult" class="subpanel">
            <div class="panel-head flush">
                <h4>{{ tr('tol.mcResults') }}</h4>
                <span
                    v-if="mcResult.pass_rate !== null"
                    class="badge"
                    :class="
                        mcResult.pass_rate >= 0.9
                            ? 'badge-ok'
                            : mcResult.pass_rate >= 0.7
                              ? 'badge-warn'
                              : 'badge-bad'
                    "
                >
                    {{ tr('tol.yield') }}: {{ (mcResult.pass_rate * 100).toFixed(1) }}%
                </span>
            </div>
            <dl class="metrics">
                <div>
                    <dt>{{ tr('tol.nominal') }}</dt>
                    <dd>
                        {{ fmt(mcResult.nominal) }} <small>{{ mcResult.criterion_unit }}</small>
                    </dd>
                </div>
                <div>
                    <dt>{{ tr('tol.mean') }}</dt>
                    <dd>
                        {{ fmt(mcResult.mean) }} <small>± {{ fmt(mcResult.std_dev) }}</small>
                    </dd>
                </div>
                <div>
                    <dt>P50 / P90 / P98</dt>
                    <dd class="small-metric">
                        {{ fmt(mcResult.p50) }} / {{ fmt(mcResult.p90) }} / {{ fmt(mcResult.p98) }}
                    </dd>
                </div>
                <div>
                    <dt>{{ tr('tol.succeeded') }}</dt>
                    <dd>{{ mcResult.succeeded }} / {{ mcResult.trials }}</dd>
                </div>
                <div v-if="mcResult.compensator_range">
                    <dt>{{ tr('tol.compRange') }}</dt>
                    <dd class="small-metric">
                        [{{ mcResult.compensator_range[0].toFixed(3) }},
                        {{ mcResult.compensator_range[1].toFixed(3) }}] mm
                    </dd>
                </div>
            </dl>
            <div v-if="mcResult.histogram.length" class="eng-chart-box">
                <div class="note">{{ tr('tol.histogram') }} ({{ mcResult.criterion_unit }})</div>
                <svg viewBox="0 0 480 120" class="hist-svg" aria-hidden="true">
                    <rect
                        v-for="(b, bi) in mcResult.histogram"
                        :key="bi"
                        :x="12 + bi * 38"
                        :y="100 - (b.count / maxBin) * 86"
                        width="32"
                        :height="(b.count / maxBin) * 86"
                        fill="var(--accent)"
                        opacity="0.82"
                        rx="2"
                    />
                    <text x="12" y="115" font-size="10" fill="var(--muted)">
                        {{ fmt(mcResult.min) }}
                    </text>
                    <text x="468" y="115" font-size="10" text-anchor="end" fill="var(--muted)">
                        {{ fmt(mcResult.max) }}
                    </text>
                </svg>
            </div>
        </div>
    </section>
</template>
