<script setup lang="ts">
import { tr } from '../i18n';
import { computed, ref, watch } from 'vue';
import { callTool } from '../api';
import NumberField from './NumberField.vue';
import { canonicalJson } from '../validation';
import type {
    GapSolve,
    GapSolveEntry,
    OpticalSystem,
    SolveKind,
    SolveRecalculateResult,
    SolveResult,
} from '../types';

const props = defineProps<{
    /** The saved system: solves act on it, never on an unapplied draft. */
    system: OpticalSystem;
    online: boolean;
    dirty: boolean;
    /** The saved view is outdated (changed elsewhere, or a committed change has not refreshed yet). */
    stale: boolean;
    busy: boolean;
    /** Rejects unsaved drafts, runs the operation with the current revision, then reloads. */
    mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
}>();

type ActiveKind = Exclude<SolveKind, 'fixed'>;

const KINDS: { id: ActiveKind; label: string; help: string }[] = [
    {
        id: 'pickup',
        get label() {
            return tr('solve.kinds.pickup');
        },
        get help() {
            return tr('solve.help.pickup');
        },
    },
    {
        id: 'edge_thickness',
        get label() {
            return tr('solve.kinds.edge_thickness');
        },
        get help() {
            return tr('solve.help.edge_thickness');
        },
    },
    {
        id: 'overall_length',
        get label() {
            return tr('solve.kinds.overall_length');
        },
        get help() {
            return tr('solve.help.overall_length');
        },
    },
    {
        id: 'marginal_height',
        get label() {
            return tr('solve.kinds.marginal_height');
        },
        get help() {
            return tr('solve.help.marginal_height');
        },
    },
];

// Gap numbers are shown 1-based ("gap after surface n"); converted to zero-based API indexes when sending.
const kind = ref<ActiveKind>('pickup');
const gap = ref(1);
const source = ref(1);
const scale = ref(1);
const offset = ref(0);
const height = ref(5);
const first = ref(1);
const last = ref(1);
const value = ref(0);
const zone = ref(0);
const status = ref('');

const count = computed(() => props.system.surfaces.length);
const surfaceNumbers = computed(() => Array.from({ length: count.value }, (_, i) => i + 1));
const help = computed(() => KINDS.find((k) => k.id === kind.value)!.help);
const solves = computed<GapSolveEntry[]>(() =>
    props.system.surfaces.flatMap((s, gapIndex) =>
        s.gap_solve.kind === 'fixed'
            ? []
            : [{ gap: gapIndex, thickness: s.thickness, definition: s.gap_solve }],
    ),
);
const editing = computed(() => props.system.surfaces[gap.value - 1]?.gap_solve.kind !== 'fixed');

/** Load an active definition into the form fields. */
function load(d: GapSolve): void {
    switch (d.kind) {
        case 'fixed':
            return;
        case 'pickup':
            source.value = d.source + 1;
            scale.value = d.scale;
            offset.value = d.offset;
            break;
        case 'edge_thickness':
            height.value = d.height;
            value.value = d.value;
            break;
        case 'overall_length':
            first.value = d.first + 1;
            last.value = d.last;
            value.value = d.value;
            break;
        case 'marginal_height':
            value.value = d.value;
            zone.value = d.zone;
            break;
    }
    kind.value = d.kind;
}

function select(n: number): void {
    gap.value = n;
    status.value = '';
    // Edit always reloads, even when the gap number is unchanged.
    const d = props.system.surfaces[n - 1]?.gap_solve;
    if (d) load(d);
}

// Load the selected gap's saved relation on mount, on selection change, and when that gap's saved
// definition changes (reload / external edit). The key covers only the definition, so unchanged
// polls and thickness-only edits never reset in-progress typing.
watch(
    () => canonicalJson([gap.value, props.system.surfaces[gap.value - 1]?.gap_solve ?? null]),
    () => {
        const d = props.system.surfaces[gap.value - 1]?.gap_solve;
        if (d) load(d);
    },
    { immediate: true },
);

// Keep choices inside the saved prescription when surfaces are removed or reloaded.
watch(count, (n) => {
    for (const r of [gap, source, first, last]) if (r.value > n) r.value = n;
});

const commonBlock = computed(() => {
    if (!props.online) return tr('common.offline');
    if (props.stale) return tr('solve.blockStale');
    if (props.dirty) return tr('solve.blockDirty');
    return '';
});

const block = computed(() => {
    if (commonBlock.value) return commonBlock.value;
    const g = gap.value;
    if (kind.value === 'pickup' && source.value === g) return tr('solve.blockPickup');
    if (kind.value === 'edge_thickness') {
        if (!(height.value >= 0)) return tr('solve.blockHeight');
    }
    if (kind.value === 'overall_length' && !(first.value <= g && g <= last.value))
        return tr('solve.blockRange');
    return '';
});

function definition(): GapSolve {
    switch (kind.value) {
        case 'pickup':
            return {
                kind: 'pickup',
                source: source.value - 1,
                scale: scale.value,
                offset: offset.value,
            };
        case 'edge_thickness':
            return { kind: 'edge_thickness', height: height.value, value: value.value };
        // The API interval is the half-open gap range [first, last): first..last inclusive when counted from 1.
        case 'overall_length':
            return {
                kind: 'overall_length',
                first: first.value - 1,
                last: last.value,
                value: value.value,
            };
        case 'marginal_height':
            return { kind: 'marginal_height', value: value.value, zone: zone.value };
    }
}

function describe(d: GapSolve): string {
    switch (d.kind) {
        case 'fixed':
            return tr('solve.descFixed');
        case 'pickup':
            return tr('solve.descPickup', {
                source: d.source + 1,
                scale: d.scale,
                offset: d.offset,
            });
        case 'edge_thickness':
            return tr('solve.descEdge', { value: d.value, height: d.height });
        case 'overall_length':
            return tr('solve.descOverall', { value: d.value, first: d.first + 1, last: d.last });
        case 'marginal_height':
            return tr('solve.descMarginal', { value: d.value, zone: d.zone });
    }
}

async function install(g: number, def: GapSolve): Promise<void> {
    status.value = '';
    let solved: SolveResult | null = null;
    const ok = await props.mutate(async (revision) => {
        solved = await callTool('solve', { gap: g, definition: def, revision });
    });
    const result = solved as SolveResult | null;
    if (!ok || !result) return;
    const n = result.solve.gap + 1;
    status.value =
        result.solve.definition.kind === 'fixed'
            ? tr('solve.statusRemoved', { n: n, thickness: result.solve.thickness })
            : tr('solve.statusSolved', { n: n, thickness: result.solve.thickness });
}

const apply = (): Promise<void> => install(gap.value - 1, definition());
const remove = (g: number): Promise<void> => install(g, { kind: 'fixed' });

async function recalculate(): Promise<void> {
    status.value = '';
    let out: SolveRecalculateResult | null = null;
    const ok = await props.mutate(async (revision) => {
        out = await callTool('solve_recalculate', { revision });
    });
    const result = out as SolveRecalculateResult | null;
    if (ok && result)
        status.value = `Recalculated ${result.solves.length} active ${result.solves.length === 1 ? 'solve' : 'solves'}.`;
}
</script>

<template>
    <section class="panel" aria-labelledby="solve-title">
        <div class="panel-head">
            <h3 id="solve-title">{{ tr('solve.title') }}</h3>
            <span>{{ tr('common.savedSystem') }}</span>
        </div>
        <div class="subpanel">
            <h4 id="solve-list-title">{{ tr('solve.savedSolves') }}</h4>
            <p v-if="!solves.length" class="note">
                {{ tr('solve.noSolves') }}
            </p>
            <ul v-else class="solve-list" aria-labelledby="solve-list-title">
                <li v-for="s in solves" :key="s.gap">
                    <span
                        >{{ tr('solve.gapAfter') }} {{ s.gap + 1 }}: {{ describe(s.definition) }}
                        {{ tr('fragments.resolvedThickness') }}
                        <strong>{{ s.thickness }} mm</strong></span
                    >
                    <button
                        type="button"
                        class="small"
                        :disabled="busy"
                        :aria-label="tr('solve.editSolve', { n: s.gap + 1 })"
                        @click="select(s.gap + 1)"
                    >
                        {{ tr('solve.edit') }}
                    </button>
                    <button
                        type="button"
                        class="small"
                        :disabled="!!commonBlock || busy"
                        :aria-label="tr('solve.removeSolve', { n: s.gap + 1 })"
                        @click="remove(s.gap)"
                    >
                        {{ tr('solve.remove') }}
                    </button>
                </li>
            </ul>
            <div class="controls">
                <button
                    type="button"
                    :disabled="!!commonBlock || busy || !solves.length"
                    @click="recalculate"
                >
                    {{ tr('solve.recalculate') }}
                </button>
            </div>
        </div>
        <div class="subpanel">
            <h4>{{ editing ? tr('solve.editTitle') : tr('solve.newTitle') }}</h4>
            <div class="controls">
                <label
                    >{{ tr('solve.gapAfter') }}
                    <select v-model.number="gap">
                        <option v-for="n in surfaceNumbers" :key="n" :value="n">{{ n }}</option>
                    </select>
                </label>
                <label
                    >{{ tr('solve.solveKind') }}
                    <select v-model="kind">
                        <option v-for="k in KINDS" :key="k.id" :value="k.id">{{ k.label }}</option>
                    </select>
                </label>
                <label v-if="kind === 'pickup'"
                    >{{ tr('solve.sourceGap') }}
                    <select v-model.number="source">
                        <option v-for="n in surfaceNumbers" :key="n" :value="n">{{ n }}</option>
                    </select>
                </label>
                <template v-if="kind === 'overall_length'">
                    <label
                        >{{ tr('solve.firstGap') }}
                        <select v-model.number="first">
                            <option v-for="n in surfaceNumbers" :key="n" :value="n">{{ n }}</option>
                        </select>
                    </label>
                    <label
                        >{{ tr('solve.lastGap') }}
                        <select v-model.number="last">
                            <option v-for="n in surfaceNumbers" :key="n" :value="n">{{ n }}</option>
                        </select>
                    </label>
                </template>
                <label v-if="kind === 'pickup'"
                    >{{ tr('solve.scale') }}
                    <NumberField v-model="scale" :aria-label="tr('remaining.m12')"
                /></label>
                <label v-if="kind === 'pickup'"
                    >{{ tr('solve.offset') }}
                    <NumberField v-model="offset" :aria-label="tr('remaining.m13')"
                /></label>
                <label v-if="kind === 'edge_thickness'"
                    >{{ tr('solve.height') }}
                    <NumberField v-model="height" :aria-label="tr('remaining.m14')"
                /></label>
                <label v-if="kind === 'marginal_height'"
                    >{{ tr('solve.zone') }}
                    <NumberField v-model="zone" :aria-label="tr('remaining.m15')"
                /></label>
                <label v-if="kind !== 'pickup'"
                    >{{
                        kind === 'edge_thickness'
                            ? tr('solve.targetEdge')
                            : kind === 'overall_length'
                              ? tr('solve.targetLength')
                              : tr('solve.targetHeight')
                    }}
                    <NumberField v-model="value" :aria-label="tr('remaining.m16')"
                /></label>
                <button type="button" class="primary" :disabled="!!block || busy" @click="apply">
                    {{ editing ? tr('solve.update') : tr('solve.install') }}
                </button>
            </div>
            <p class="note">{{ help }} {{ tr('solve.noteEngine') }}</p>
            <p class="note">
                {{ tr('solve.noteRemove') }}
            </p>
            <p v-if="block" class="note">{{ block }}</p>
            <p v-if="status" role="status" class="note">{{ status }}</p>
        </div>
    </section>
</template>
