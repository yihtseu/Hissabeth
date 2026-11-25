<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { Play, Check, Plus, Trash2, Sparkles } from 'lucide-vue-next';
import { callTool, errorMessage } from '../api';
import { tr } from '../i18n';
import type {
    ConstraintMode,
    OpticalSystem,
    OptimizeOperand,
    OptimizeResult,
    OptimizeVariable,
    Paraxial,
} from '../types';

const props = defineProps<{
    system: OpticalSystem;
    paraxial: Paraxial | null;
    revision: number;
    online: boolean;
    dirty: boolean;
    busy: boolean;
    stale: boolean;
    refreshPending: boolean;
    mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
}>();

interface VarRow {
    id: number;
    kind: OptimizeVariable['kind'];
    surface: number;
    index: number;
    axis: 'x' | 'y' | 'z';
    min: string;
    max: string;
}

interface OpRow {
    id: number;
    kind: OptimizeOperand['kind'];
    surface: number;
    field_index: number; // -1 means all fields for rms_spot
    target: number;
    weight: number;
    mode: ConstraintMode;
}

let nextId = 1;
const vars = ref<VarRow[]>([]);
const ops = ref<OpRow[]>([]);
const maxIterations = ref(30);
const initialDamping = ref(0.001);
const running = ref(false);
const applying = ref(false);
const error = ref('');
const status = ref('');
const result = ref<OptimizeResult | null>(null);

const canApply = computed(
    () =>
        props.online &&
        !props.dirty &&
        !props.busy &&
        !props.stale &&
        !props.refreshPending &&
        !running.value &&
        !applying.value &&
        vars.value.length > 0 &&
        ops.value.length > 0,
);
const canPreview = computed(
    () =>
        props.online &&
        !running.value &&
        !applying.value &&
        vars.value.length > 0 &&
        ops.value.length > 0,
);

function parseBound(raw: string): number | null {
    const t = raw.trim();
    if (!t) return null;
    const n = Number(t);
    return Number.isFinite(n) ? n : null;
}

function buildVariables(): OptimizeVariable[] {
    return vars.value.map((r): OptimizeVariable => {
        const min = parseBound(r.min);
        const max = parseBound(r.max);
        switch (r.kind) {
            case 'even_asphere':
                return { kind: 'even_asphere', surface: r.surface, index: r.index, min, max };
            case 'odd_asphere':
                return { kind: 'odd_asphere', surface: r.surface, index: r.index, min, max };
            case 'decenter':
                return {
                    kind: 'decenter',
                    surface: r.surface,
                    axis: r.axis === 'x' ? 'x' : 'y',
                    min,
                    max,
                };
            case 'tilt':
                return { kind: 'tilt', surface: r.surface, axis: r.axis, min, max };
            case 'config_thickness':
                return {
                    kind: 'config_thickness',
                    configuration: props.system.active_configuration ?? 0,
                    surface: r.surface,
                    min,
                    max,
                };
            case 'config_radius':
                return {
                    kind: 'config_radius',
                    configuration: props.system.active_configuration ?? 0,
                    surface: r.surface,
                    min,
                    max,
                };
            case 'radius':
                return { kind: 'radius', surface: r.surface, min, max };
            case 'thickness':
                return { kind: 'thickness', surface: r.surface, min, max };
            case 'conic':
                return { kind: 'conic', surface: r.surface, min, max };
            case 'model_glass_nd':
                return { kind: 'model_glass_nd', surface: r.surface, min, max };
            case 'model_glass_vd':
                return { kind: 'model_glass_vd', surface: r.surface, min, max };
        }
    });
}

function buildOperands(): OptimizeOperand[] {
    const waves = props.system.wavelengths.map((w) => w.value);
    const wMin = waves.length ? Math.min(...waves) : 486.1327;
    const wMax = waves.length ? Math.max(...waves) : 656.2725;
    return ops.value.map((r) => {
        switch (r.kind) {
            case 'rms_spot':
                return {
                    kind: 'rms_spot',
                    field_index: r.field_index < 0 ? null : r.field_index,
                    target: r.target,
                    weight: r.weight,
                    mode: r.mode,
                };
            case 'rms_wavefront':
                return {
                    kind: 'rms_wavefront',
                    field_index: Math.max(0, r.field_index),
                    target: r.target,
                    weight: r.weight,
                    mode: r.mode,
                };
            case 'efl':
            case 'bfl':
            case 'working_f_number':
            case 'total_track':
                return { kind: r.kind, target: r.target, weight: r.weight, mode: r.mode };
            case 'center_thickness':
            case 'edge_thickness':
                return {
                    kind: r.kind,
                    surface: r.surface,
                    target: r.target,
                    weight: r.weight,
                    mode: r.mode,
                };
            case 'distortion':
            case 'chief_ray_angle':
                return {
                    kind: r.kind,
                    field_index: Math.max(0, r.field_index),
                    target: r.target,
                    weight: r.weight,
                    mode: r.mode,
                };
            case 'axial_color':
                return {
                    kind: 'axial_color',
                    wave1: wMin,
                    wave2: wMax,
                    target: r.target,
                    weight: r.weight,
                    mode: r.mode,
                };
            case 'lateral_color':
                return {
                    kind: 'lateral_color',
                    field_index: Math.max(0, r.field_index),
                    wave1: wMin,
                    wave2: wMax,
                    target: r.target,
                    weight: r.weight,
                    mode: r.mode,
                };
            case 'ray_intercept':
                return {
                    kind: 'ray_intercept',
                    field_index: Math.max(0, r.field_index),
                    px: 0,
                    py: 1,
                    axis: 'y',
                    target: r.target,
                    weight: r.weight,
                    mode: r.mode,
                };
        }
    });
}

function loadPreset(): void {
    const surfs = props.system.surfaces;
    const nextVars: VarRow[] = [];
    for (let i = 0; i < surfs.length; i++) {
        const s = surfs[i]!;
        if (s.interaction !== 'dummy') {
            nextVars.push({
                id: nextId++,
                kind: 'radius',
                surface: i,
                index: 0,
                axis: 'y',
                min: '',
                max: '',
            });
        }
    }
    const lastIdx = surfs.length - 1;
    if (lastIdx >= 0 && surfs[lastIdx]!.gap_solve.kind === 'fixed') {
        nextVars.push({
            id: nextId++,
            kind: 'thickness',
            surface: lastIdx,
            index: 0,
            axis: 'y',
            min: '1',
            max: '',
        });
    }
    vars.value = nextVars;

    const nextOps: OpRow[] = [
        {
            id: nextId++,
            kind: 'rms_spot',
            surface: 0,
            field_index: -1,
            target: 0,
            weight: 50,
            mode: 'eq',
        },
    ];
    if (props.paraxial?.efl !== null && props.paraxial?.efl !== undefined) {
        nextOps.push({
            id: nextId++,
            kind: 'efl',
            surface: 0,
            field_index: 0,
            target: Number(props.paraxial.efl.toFixed(3)),
            weight: 10,
            mode: 'eq',
        });
    }
    for (let i = 0; i < surfs.length; i++) {
        const s = surfs[i]!;
        if (s.material !== 'AIR' && s.interaction === 'refract') {
            nextOps.push({
                id: nextId++,
                kind: 'edge_thickness',
                surface: i,
                field_index: 0,
                target: 1.5,
                weight: 10,
                mode: 'gt',
            });
        }
    }
    ops.value = nextOps;
    error.value = '';
    status.value = tr('opt.presetLoaded');
}

function addVar(): void {
    vars.value.push({
        id: nextId++,
        kind: 'radius',
        surface: 0,
        index: 0,
        axis: 'y',
        min: '',
        max: '',
    });
}

function addOp(): void {
    ops.value.push({
        id: nextId++,
        kind: 'rms_spot',
        surface: 0,
        field_index: -1,
        target: 0,
        weight: 10,
        mode: 'eq',
    });
}

async function runOptimization(apply: boolean): Promise<void> {
    error.value = '';
    status.value = '';
    const variables = buildVariables();
    const operands = buildOperands();
    if (apply) {
        applying.value = true;
        try {
            let captured: OptimizeResult | null = null;
            const ok = await props.mutate(async (rev) => {
                captured = await callTool('optimize', {
                    variables,
                    operands,
                    max_iterations: maxIterations.value,
                    initial_damping: initialDamping.value,
                    apply: true,
                    revision: rev,
                });
                return captured;
            });
            if (ok && captured) {
                result.value = captured;
                status.value = tr('opt.applied');
            }
        } catch (e) {
            error.value = errorMessage(e);
        } finally {
            applying.value = false;
        }
        return;
    }
    running.value = true;
    try {
        result.value = await callTool('optimize', {
            variables,
            operands,
            max_iterations: maxIterations.value,
            initial_damping: initialDamping.value,
            apply: false,
        });
        status.value = tr('opt.previewDone');
    } catch (e) {
        error.value = errorMessage(e);
    } finally {
        running.value = false;
    }
}

// Seed preset automatically on first open if empty.
watch(
    () => props.system.surfaces.length,
    () => {
        const maxS = Math.max(0, props.system.surfaces.length - 1);
        for (const v of vars.value) if (v.surface > maxS) v.surface = maxS;
        for (const o of ops.value) if (o.surface > maxS) o.surface = maxS;
        if (vars.value.length === 0 && ops.value.length === 0) loadPreset();
    },
    { immediate: true },
);

const improvementPct = computed(() => {
    const r = result.value;
    if (!r || r.initial_merit <= 1e-15) return 0;
    return ((r.initial_merit - r.final_merit) / r.initial_merit) * 100;
});

const sparklinePath = computed(() => {
    const hist = result.value?.history.filter((h) => h.accepted) ?? [];
    if (hist.length < 2) return '';
    const w = 420;
    const h = 92;
    const pad = 10;
    const ys = hist.map((p) => Math.log10(Math.max(p.merit, 1e-12)));
    const minY = Math.min(...ys);
    const maxY = Math.max(...ys);
    const span = Math.max(maxY - minY, 1e-6);
    return hist
        .map((_, idx) => {
            const x = pad + (idx / (hist.length - 1)) * (w - 2 * pad);
            const y = h - pad - ((ys[idx]! - minY) / span) * (h - 2 * pad);
            return `${idx === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`;
        })
        .join(' ');
});

const fmt = (n: number) => (Math.abs(n) >= 1e-3 || n === 0 ? n.toFixed(5) : n.toExponential(3));
</script>

<template>
    <section class="panel" aria-labelledby="opt-title">
        <div class="panel-head">
            <h3 id="opt-title">{{ tr('opt.title') }}</h3>
            <span>{{ tr('opt.subtitle') }}</span>
        </div>

        <div class="analysis-controls">
            <div class="controls">
                <button type="button" @click="loadPreset">
                    <Sparkles :size="14" /> {{ tr('opt.preset') }}
                </button>
                <label>
                    {{ tr('opt.maxIter') }}
                    <input
                        v-model.number="maxIterations"
                        type="number"
                        min="1"
                        max="200"
                        step="1"
                    />
                </label>
                <label>
                    {{ tr('opt.damping') }}
                    <input
                        v-model.number="initialDamping"
                        type="number"
                        min="0.000001"
                        max="1000"
                        step="0.0005"
                    />
                </label>
                <div class="spacer"></div>
                <button type="button" :disabled="!canPreview" @click="runOptimization(false)">
                    <Play :size="14" />
                    {{ running ? tr('opt.running') : tr('opt.preview') }}
                </button>
                <button
                    type="button"
                    class="primary"
                    :disabled="!canApply"
                    @click="runOptimization(true)"
                >
                    <Check :size="14" />
                    {{ applying ? tr('opt.applying') : tr('opt.apply') }}
                </button>
            </div>
            <p v-if="dirty" class="note warn-text">{{ tr('opt.dirtyHint') }}</p>
            <p v-if="status" role="status" class="note">{{ status }}</p>
            <p v-if="error" role="alert" class="json-error">{{ error }}</p>
        </div>

        <div class="eng-two-col">
            <div class="subpanel">
                <div class="panel-head flush">
                    <h4>{{ tr('opt.variables') }} ({{ vars.length }})</h4>
                    <button type="button" class="small" @click="addVar">
                        <Plus :size="13" /> {{ tr('opt.addVar') }}
                    </button>
                </div>
                <div class="table-scroll">
                    <table>
                        <thead>
                            <tr>
                                <th>{{ tr('opt.kind') }}</th>
                                <th>{{ tr('opt.surface') }}</th>
                                <th>{{ tr('opt.min') }}</th>
                                <th>{{ tr('opt.max') }}</th>
                                <th></th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="(v, idx) in vars" :key="v.id">
                                <td>
                                    <select v-model="v.kind">
                                        <option value="radius">
                                            {{ tr('opt.varKind.radius') }}
                                        </option>
                                        <option value="thickness">
                                            {{ tr('opt.varKind.thickness') }}
                                        </option>
                                        <option value="conic">{{ tr('opt.varKind.conic') }}</option>
                                        <option value="even_asphere">
                                            {{ tr('opt.varKind.even_asphere') }}
                                        </option>
                                        <option value="odd_asphere">
                                            {{ tr('opt.varKind.odd_asphere') }}
                                        </option>
                                        <option value="model_glass_nd">
                                            {{ tr('opt.varKind.model_glass_nd') }}
                                        </option>
                                        <option value="model_glass_vd">
                                            {{ tr('opt.varKind.model_glass_vd') }}
                                        </option>
                                        <option value="decenter">
                                            {{ tr('opt.varKind.decenter') }}
                                        </option>
                                        <option value="tilt">{{ tr('opt.varKind.tilt') }}</option>
                                    </select>
                                </td>
                                <td>
                                    <select v-model.number="v.surface">
                                        <option
                                            v-for="(s, si) in system.surfaces"
                                            :key="si"
                                            :value="si"
                                        >
                                            S{{ si + 1 }}{{ s.label ? ` · ${s.label}` : '' }}
                                        </option>
                                    </select>
                                </td>
                                <td><input v-model="v.min" type="text" placeholder="−∞" /></td>
                                <td><input v-model="v.max" type="text" placeholder="+∞" /></td>
                                <td>
                                    <button
                                        type="button"
                                        class="small"
                                        :aria-label="tr('opt.remove')"
                                        @click="vars.splice(idx, 1)"
                                    >
                                        <Trash2 :size="12" />
                                    </button>
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </div>
            </div>

            <div class="subpanel">
                <div class="panel-head flush">
                    <h4>{{ tr('opt.operands') }} ({{ ops.length }})</h4>
                    <button type="button" class="small" @click="addOp">
                        <Plus :size="13" /> {{ tr('opt.addOp') }}
                    </button>
                </div>
                <div class="table-scroll">
                    <table>
                        <thead>
                            <tr>
                                <th>{{ tr('opt.kind') }}</th>
                                <th>{{ tr('opt.scope') }}</th>
                                <th>{{ tr('opt.mode') }}</th>
                                <th>{{ tr('opt.target') }}</th>
                                <th>{{ tr('opt.weight') }}</th>
                                <th></th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="(o, idx) in ops" :key="o.id">
                                <td>
                                    <select v-model="o.kind">
                                        <option value="rms_spot">
                                            {{ tr('opt.opKind.rms_spot') }}
                                        </option>
                                        <option value="rms_wavefront">
                                            {{ tr('opt.opKind.rms_wavefront') }}
                                        </option>
                                        <option value="efl">{{ tr('opt.opKind.efl') }}</option>
                                        <option value="bfl">{{ tr('opt.opKind.bfl') }}</option>
                                        <option value="working_f_number">
                                            {{ tr('opt.opKind.working_f_number') }}
                                        </option>
                                        <option value="total_track">
                                            {{ tr('opt.opKind.total_track') }}
                                        </option>
                                        <option value="center_thickness">
                                            {{ tr('opt.opKind.center_thickness') }}
                                        </option>
                                        <option value="edge_thickness">
                                            {{ tr('opt.opKind.edge_thickness') }}
                                        </option>
                                        <option value="distortion">
                                            {{ tr('opt.opKind.distortion') }}
                                        </option>
                                        <option value="axial_color">
                                            {{ tr('opt.opKind.axial_color') }}
                                        </option>
                                        <option value="lateral_color">
                                            {{ tr('opt.opKind.lateral_color') }}
                                        </option>
                                        <option value="chief_ray_angle">
                                            {{ tr('opt.opKind.chief_ray_angle') }}
                                        </option>
                                    </select>
                                </td>
                                <td>
                                    <select
                                        v-if="
                                            o.kind === 'center_thickness' ||
                                            o.kind === 'edge_thickness'
                                        "
                                        v-model.number="o.surface"
                                    >
                                        <option
                                            v-for="(_, si) in system.surfaces"
                                            :key="si"
                                            :value="si"
                                        >
                                            S{{ si + 1 }}
                                        </option>
                                    </select>
                                    <select
                                        v-else-if="
                                            [
                                                'rms_spot',
                                                'rms_wavefront',
                                                'distortion',
                                                'lateral_color',
                                                'chief_ray_angle',
                                            ].includes(o.kind)
                                        "
                                        v-model.number="o.field_index"
                                    >
                                        <option v-if="o.kind === 'rms_spot'" :value="-1">
                                            {{ tr('opt.allFields') }}
                                        </option>
                                        <option
                                            v-for="(_, fi) in system.fields"
                                            :key="fi"
                                            :value="fi"
                                        >
                                            F{{ fi + 1 }}
                                        </option>
                                    </select>
                                    <span v-else class="note">—</span>
                                </td>
                                <td>
                                    <select v-model="o.mode">
                                        <option value="eq">=</option>
                                        <option value="lt">≤</option>
                                        <option value="gt">≥</option>
                                    </select>
                                </td>
                                <td>
                                    <input v-model.number="o.target" type="number" step="any" />
                                </td>
                                <td>
                                    <input
                                        v-model.number="o.weight"
                                        type="number"
                                        min="0"
                                        step="any"
                                    />
                                </td>
                                <td>
                                    <button
                                        type="button"
                                        class="small"
                                        :aria-label="tr('opt.remove')"
                                        @click="ops.splice(idx, 1)"
                                    >
                                        <Trash2 :size="12" />
                                    </button>
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </div>
            </div>
        </div>

        <div v-if="result" class="subpanel">
            <div class="panel-head flush">
                <h4>{{ tr('opt.results') }}</h4>
                <span class="badge" :class="result.applied ? 'badge-ok' : 'badge-info'">
                    {{ result.applied ? tr('opt.badgeApplied') : tr('opt.badgePreview') }}
                </span>
            </div>

            <dl class="metrics">
                <div>
                    <dt>{{ tr('opt.initialMerit') }}</dt>
                    <dd>{{ fmt(result.initial_merit) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('opt.finalMerit') }}</dt>
                    <dd>
                        {{ fmt(result.final_merit) }}
                        <small
                            class="delta-pill"
                            :class="improvementPct >= 0 ? 'delta-good' : 'delta-bad'"
                        >
                            {{ improvementPct >= 0 ? '−' : '+'
                            }}{{ Math.abs(improvementPct).toFixed(1) }}%
                        </small>
                    </dd>
                </div>
                <div>
                    <dt>{{ tr('opt.iterations') }}</dt>
                    <dd>{{ result.iterations }}</dd>
                </div>
                <div>
                    <dt>{{ tr('opt.status') }}</dt>
                    <dd class="small-metric">{{ result.reason }}</dd>
                </div>
            </dl>

            <div v-if="sparklinePath" class="eng-chart-box">
                <div class="note">{{ tr('opt.convergenceCurve') }}</div>
                <svg viewBox="0 0 420 92" class="sparkline-svg" aria-hidden="true">
                    <path :d="sparklinePath" fill="none" stroke="var(--accent)" stroke-width="2" />
                </svg>
            </div>

            <div class="eng-two-col">
                <div class="table-scroll">
                    <h4>{{ tr('opt.varChanges') }}</h4>
                    <table>
                        <thead>
                            <tr>
                                <th>{{ tr('opt.variable') }}</th>
                                <th>{{ tr('opt.before') }}</th>
                                <th>{{ tr('opt.after') }}</th>
                                <th>{{ tr('opt.delta') }}</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="v in result.variables" :key="v.index">
                                <td>{{ v.label }}</td>
                                <td>{{ fmt(v.before) }}</td>
                                <td>{{ fmt(v.after) }}</td>
                                <td>{{ fmt(v.after - v.before) }}</td>
                            </tr>
                        </tbody>
                    </table>
                </div>
                <div class="table-scroll">
                    <h4>{{ tr('opt.opSummary') }}</h4>
                    <table>
                        <thead>
                            <tr>
                                <th>{{ tr('opt.operand') }}</th>
                                <th>{{ tr('opt.target') }}</th>
                                <th>{{ tr('opt.before') }}</th>
                                <th>{{ tr('opt.after') }}</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="o in result.operands" :key="o.index">
                                <td>{{ o.label }}</td>
                                <td>
                                    {{ o.mode === 'lt' ? '≤' : o.mode === 'gt' ? '≥' : '=' }}
                                    {{ fmt(o.target) }}
                                </td>
                                <td>{{ fmt(o.before) }}</td>
                                <td>{{ fmt(o.after) }}</td>
                            </tr>
                        </tbody>
                    </table>
                </div>
            </div>
        </div>
    </section>
</template>
