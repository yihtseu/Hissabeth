<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import {
    AlertTriangle,
    CheckCircle2,
    Compass,
    PackagePlus,
    RefreshCw,
    ShieldAlert,
} from 'lucide-vue-next';
import { callTool, errorMessage } from '../api';
import { tr } from '../i18n';
import type {
    Bench,
    BenchAlignmentGuide,
    BenchCatalogItem,
    BenchLabCheck,
    BenchPathBreak,
    BenchReadoutReading,
    BenchStage,
    BenchValueBasis,
    Vec3,
} from '../bench';

interface StageControl {
    dof: 'position' | 'pzt';
    value: number;
    allowed: [number, number];
}

const props = defineProps<{
    bench: Bench;
    online: boolean;
    saving: boolean;
    labCheck: BenchLabCheck | null;
    labPending: boolean;
    labStale: boolean;
    labError: string;
}>();

const emit = defineEmits<{
    select: [id: string];
    addFromCatalog: [item: BenchCatalogItem];
    addBeamDump: [position: Vec3, direction: Vec3, branch: string];
    refreshLab: [];
    setStage: [id: string, dof: StageControl['dof'], value: number];
}>();

const stageError = ref<{ id: string; dof: StageControl['dof']; message: string }>();
watch(
    () => props.bench,
    () => (stageError.value = undefined),
);

function stageControls(stage: BenchStage): StageControl[] {
    const controls: StageControl[] = [
        { dof: 'position', value: stage.position_mm, allowed: stage.travel },
    ];
    if (stage.pzt) {
        controls.push({ dof: 'pzt', value: stage.pzt.value_um, allowed: stage.pzt.range_um });
    }
    return controls;
}

function editStage(stage: BenchStage, control: StageControl, event: Event): void {
    const input = event.target as HTMLInputElement;
    const value = input.valueAsNumber;
    const [min, max] = control.allowed;
    if (!Number.isFinite(value) || value < min || value > max) {
        stageError.value = {
            id: stage.id,
            dof: control.dof,
            message: tr('benchLab.stageRangeError', {
                stage: stage.id,
                control: tr(`benchLab.stageControls.${control.dof}`),
                value: input.value || '—',
                min,
                max,
            }),
        };
        input.value = String(control.value);
        return;
    }
    stageError.value = undefined;
    emit('setStage', stage.id, control.dof, value);
}

const subtab = ref<'audit' | 'alignment' | 'catalog'>('audit');
const guide = ref<BenchAlignmentGuide | null>(null);
const guidePending = ref(false);
const guideError = ref('');

const catalogItems = ref<BenchCatalogItem[]>([]);
const catalogQuery = ref('');
const catalogKind = ref('');
const catalogError = ref('');

async function loadAlignment(): Promise<void> {
    if (!props.online) return;
    guidePending.value = true;
    guideError.value = '';
    try {
        guide.value = await callTool('bench_alignment_guide', { bench: props.bench });
    } catch (e) {
        guideError.value = errorMessage(e);
    } finally {
        guidePending.value = false;
    }
}

async function loadCatalog(): Promise<void> {
    if (!props.online) return;
    catalogError.value = '';
    try {
        const res = await callTool('bench_catalog', {
            ...(catalogQuery.value.trim() ? { query: catalogQuery.value.trim() } : {}),
            ...(catalogKind.value ? { kind: catalogKind.value } : {}),
        });
        catalogItems.value = res.items;
    } catch (e) {
        catalogError.value = errorMessage(e);
    }
}

watch(subtab, (t) => {
    if (t === 'alignment') void loadAlignment();
    if (t === 'catalog' && catalogItems.value.length === 0) void loadCatalog();
});

watch(
    () => props.bench,
    () => {
        if (subtab.value === 'alignment') void loadAlignment();
    },
    { deep: true },
);

onMounted(() => {
    void loadCatalog();
});

// Modal camera readouts follow the lab check: one revisioned request per
// detector that declares a readout, refreshed whenever the check reruns. Each
// result keeps the bench snapshot it was computed from and is shown only while
// that snapshot is current and its revision matches a fresh lab check.
const readoutResults = ref<{ snapshot: string; readings: BenchReadoutReading[] }>();
const readoutError = ref('');
let readoutRequest = 0;
let requestSnapshot = '';
const benchSnapshot = computed(() => JSON.stringify(props.bench));
const readouts = computed(() => {
    const result = readoutResults.value;
    if (!result || result.snapshot !== benchSnapshot.value || props.labStale) return [];
    return result.readings.filter((r) => r.revision === props.labCheck?.revision);
});
async function loadReadouts(): Promise<void> {
    const detectors = props.bench.elements
        .filter((e) => e.kind === 'detector' && e.readout)
        .map((e) => e.id);
    const request = ++readoutRequest;
    const snapshot = benchSnapshot.value;
    requestSnapshot = snapshot;
    const bench = JSON.parse(snapshot) as Bench;
    readoutResults.value = undefined;
    readoutError.value = '';
    if (!props.online || detectors.length === 0) return;
    try {
        const readings = await Promise.all(
            detectors.map((detector) => callTool('bench_readout', { bench, detector })),
        );
        if (request !== readoutRequest || snapshot !== benchSnapshot.value) return;
        readoutResults.value = { snapshot, readings };
    } catch (e) {
        if (request !== readoutRequest || snapshot !== benchSnapshot.value) return;
        readoutError.value = errorMessage(e);
    }
}
watch(
    () => props.labCheck,
    () => void loadReadouts(),
    { immediate: true },
);
watch(benchSnapshot, (snapshot) => {
    // An edit invalidates the shown readings at once; the next lab check reloads
    // them. A request already started for this very snapshot is kept.
    if (snapshot === requestSnapshot) return;
    readoutRequest++;
    readoutResults.value = undefined;
    readoutError.value = '';
});
const peak = (r: BenchReadoutReading) =>
    Math.max(1e-300, ...r.ports.flatMap((p) => p.modes.map((m) => m.intensity)));
const cellShade = (r: BenchReadoutReading, value: number) =>
    `color-mix(in srgb, var(--accent, #0d9488) ${Math.round((100 * value) / peak(r))}%, transparent)`;
const modalValue = (n: number) =>
    Math.abs(n) < 1e-9 ? '0' : Math.abs(n) < 1e-3 ? n.toExponential(1) : n.toFixed(3);

const scoreClass = computed(() => {
    const score = props.labCheck?.readiness_score ?? 0;
    if (score >= 85) return 'badge-ok';
    if (score >= 60) return 'badge-warn';
    return 'badge-bad';
});

const fmt = (n: number | null | undefined, digits = 2) =>
    n === null || n === undefined ? '—' : n.toFixed(digits);
const reading = (n: number) => Number(n.toPrecision(10)).toString();
const interruption = ({ element, port, reason }: BenchPathBreak) =>
    `${element} / ${port} (${reason})`;
/** The value's basis, its assumptions and omissions, in one line. */
const basisText = (basis: BenchValueBasis) =>
    [
        tr(`benchLab.readings.basisKind.${basis.kind}`),
        ...(basis.assumptions ?? []).map((a) =>
            tr(`benchLab.readings.assumption.${a.code}`, {
                elements: (a.elements ?? []).join(', '),
            }).trim(),
        ),
        ...(basis.omitted ?? []).map((o) => tr(`benchLab.readings.omission.${o.code}`)),
    ].join(' · ');
const statusBadge = {
    pass: 'badge-ok',
    indeterminate: 'badge-warn',
    fail: 'badge-bad',
    unreachable: 'badge-bad',
} as const;
</script>

<template>
    <section class="panel lab-panel" aria-label="Lab engineering assistant">
        <fieldset
            v-for="stage in bench.stages"
            :key="stage.id"
            class="stage-controls"
            :disabled="!online || saving"
        >
            <legend>{{ tr('benchLab.stageTitle', { stage: stage.id }) }}</legend>
            <small class="note">{{ stage.members.join(', ') }}</small>
            <div v-for="control in stageControls(stage)" :key="control.dof" class="stage-control">
                <label>
                    {{ tr(`benchLab.stageControls.${control.dof}`) }}
                    <input
                        type="number"
                        step="any"
                        :min="control.allowed[0]"
                        :max="control.allowed[1]"
                        :value="control.value"
                        @change="editStage(stage, control, $event)"
                    />
                </label>
                <input
                    type="range"
                    step="any"
                    :aria-label="tr(`benchLab.stageControls.${control.dof}`)"
                    :min="control.allowed[0]"
                    :max="control.allowed[1]"
                    :value="control.value"
                    @input="editStage(stage, control, $event)"
                />
                <small class="note">{{ control.allowed.join(' … ') }}</small>
                <p
                    v-if="stageError?.id === stage.id && stageError.dof === control.dof"
                    class="json-error"
                    role="alert"
                >
                    {{ stageError.message }}
                </p>
            </div>
        </fieldset>
        <div class="lab-tabs" role="tablist">
            <button
                type="button"
                role="tab"
                :aria-selected="subtab === 'audit'"
                @click="subtab = 'audit'"
            >
                <ShieldAlert :size="14" /> {{ tr('benchLab.tabAudit') }}
                <span v-if="labCheck && !labStale" class="badge" :class="scoreClass">
                    {{ labCheck.readiness_score }}/100
                </span>
            </button>
            <button
                type="button"
                role="tab"
                :aria-selected="subtab === 'alignment'"
                @click="subtab = 'alignment'"
            >
                <Compass :size="14" /> {{ tr('benchLab.tabAlignment') }}
            </button>
            <button
                type="button"
                role="tab"
                :aria-selected="subtab === 'catalog'"
                @click="subtab = 'catalog'"
            >
                <PackagePlus :size="14" /> {{ tr('benchLab.tabCatalog') }}
            </button>
            <div class="spacer"></div>
            <button
                v-if="subtab === 'audit'"
                type="button"
                class="small"
                :disabled="!online || labPending"
                @click="emit('refreshLab')"
            >
                <RefreshCw :size="12" /> {{ tr('benchLab.refresh') }}
            </button>
            <button
                v-if="subtab === 'alignment'"
                type="button"
                class="small"
                :disabled="!online || guidePending"
                @click="loadAlignment"
            >
                <RefreshCw :size="12" /> {{ tr('benchLab.refresh') }}
            </button>
        </div>

        <!-- TAB 1: LAB READINESS & CLEARANCE AUDIT -->
        <div v-if="subtab === 'audit'" class="lab-body">
            <p v-if="labError" class="json-error" role="alert">{{ labError }}</p>
            <p v-if="labPending && !labCheck" class="note">{{ tr('benchLab.checking') }}</p>
            <template v-else-if="labCheck">
                <p class="lab-revision" role="status">
                    <span v-if="labStale" class="badge badge-warn" data-lab-stale>{{
                        tr('benchLab.readings.stale')
                    }}</span>
                    {{ tr('benchLab.readings.revision', { revision: labCheck.revision }) }}
                    ·
                    {{ tr(labCheck.saved ? 'benchLab.readings.saved' : 'benchLab.readings.draft') }}
                    <span v-if="labPending && labStale" class="note">{{
                        tr('benchLab.checking')
                    }}</span>
                </p>

                <p v-if="readoutError" class="json-error" role="alert">{{ readoutError }}</p>
                <div
                    v-for="r in readouts"
                    :key="`readout:${r.detector}`"
                    class="lab-section"
                    :data-modal-readout="r.detector"
                >
                    <h4>
                        {{
                            tr('benchLab.readings.modal', {
                                detector: r.detector,
                                normalization: tr(
                                    `benchLab.readings.normalization.${r.normalization}`,
                                ),
                            })
                        }}
                        <span class="note">{{
                            tr('benchLab.readings.revision', { revision: r.revision })
                        }}</span>
                    </h4>
                    <table class="modal-heatmap">
                        <thead>
                            <tr>
                                <th></th>
                                <th v-for="m in r.ports[0]?.modes ?? []" :key="m.index">
                                    {{ m.index }}
                                </th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="port in r.ports" :key="port.id">
                                <th :title="port.arms.join(', ')">{{ port.id }}</th>
                                <td
                                    v-for="m in port.modes"
                                    :key="m.index"
                                    :data-port="port.id"
                                    :data-mode="m.index"
                                    :style="{ backgroundColor: cellShade(r, m.intensity) }"
                                    :title="
                                        tr('benchLab.readings.fraction', {
                                            value: fmt(100 * m.fraction, 1),
                                        })
                                    "
                                >
                                    {{ modalValue(m.intensity) }}
                                </td>
                            </tr>
                        </tbody>
                    </table>
                    <ul v-if="r.signals.length" class="lab-readings">
                        <li
                            v-for="signal in r.signals"
                            :key="signal.pair"
                            :data-signal="signal.pair"
                        >
                            <b>{{ signal.pair }}</b>
                            <span>{{
                                tr('benchLab.readings.quadratures', {
                                    real: modalValue(signal.real),
                                    imag: modalValue(signal.imag),
                                })
                            }}</span>
                        </li>
                    </ul>
                    <p v-for="w in r.warnings" :key="w" class="note">{{ w }}</p>
                </div>

                <div v-if="labCheck.stages.length || labCheck.branches.length" class="lab-section">
                    <h4>{{ tr('benchLab.readings.paths') }}</h4>
                    <ul class="lab-readings">
                        <li
                            v-for="stage in labCheck.stages"
                            :key="`stage:${stage.id}`"
                            :data-stage-reading="stage.id"
                        >
                            <b>{{ tr('benchLab.stageTitle', { stage: stage.id }) }}</b>
                            <span>{{
                                tr('benchLab.readings.position', {
                                    value: reading(stage.position_mm),
                                })
                            }}</span>
                            <span v-if="stage.pzt_um !== null"
                                >PZT {{ reading(stage.pzt_um) }} µm</span
                            >
                        </li>
                        <li
                            v-for="arm in labCheck.branches"
                            :key="`branch:${arm.id}`"
                            :data-branch-reading="arm.id"
                        >
                            <b>
                                <span
                                    class="arm-swatch"
                                    :style="{ backgroundColor: arm.color }"
                                ></span>
                                {{ arm.id }}
                            </b>
                            <span v-for="stage in arm.stage_ids" :key="stage" class="note">
                                {{ tr('benchLab.stageTitle', { stage }) }}
                            </span>
                            <span v-if="arm.break" class="note">
                                {{ tr('benchLab.readings.unavailable') }} ·
                                {{ interruption(arm.break) }}
                            </span>
                            <span v-for="(path, index) in arm.paths" :key="index">
                                <span v-if="arm.paths.length > 1" class="note"
                                    >{{ path.source }} · {{ path.wavelength_nm }} nm ·
                                </span>
                                {{
                                    tr(
                                        path.break
                                            ? 'benchLab.readings.partialOpl'
                                            : 'benchLab.readings.opl',
                                        { value: reading(path.opl_mm) },
                                    )
                                }}
                                <span v-if="path.break" class="note">
                                    · {{ interruption(path.break) }}</span
                                >
                            </span>
                        </li>
                    </ul>
                </div>

                <div v-if="labCheck.assertions.length" class="lab-section">
                    <h4>{{ tr('benchLab.readings.assertions') }}</h4>
                    <ul class="lab-readings">
                        <li
                            v-for="result in labCheck.assertions"
                            :key="result.assertion"
                            :data-assertion="result.assertion"
                            :data-status="result.status"
                            :class="{
                                'failed-assertion':
                                    result.status === 'fail' || result.status === 'unreachable',
                                'undecided-assertion': result.status === 'indeterminate',
                            }"
                        >
                            <b>{{ result.assertion }}</b>
                            <span class="note">{{
                                tr(`benchLab.readings.kind.${result.kind}`)
                            }}</span>
                            <span class="badge" :class="statusBadge[result.status]">
                                {{ tr(`benchLab.readings.status.${result.status}`) }}
                            </span>
                            <span class="note">{{ result.branches.join(' / ') }}</span>
                            <span v-if="typeof result.value === 'number'">
                                {{ result.kind === 'image_rotation' ? 'θ' : 'ΔOPL' }}
                                {{ reading(result.value) }}
                                {{ result.unit === 'um' ? 'µm' : result.unit }}
                            </span>
                            <span v-else-if="result.value && 'difference' in result.value">
                                Δ {{ reading(result.value.difference) }} {{ result.unit }} @
                                {{ reading(result.value.wavelength_nm) }} nm
                            </span>
                            <span v-else-if="result.value && 'parity' in result.value">
                                {{ result.value.reflections.join(' / ') }} ·
                                {{ result.value.parity }}
                            </span>
                            <span v-else-if="result.value">
                                Δr {{ reading(result.value.position_mm) }} mm · Δθ
                                {{ reading(result.value.angle_mrad) }} mrad
                            </span>
                            <small class="assertion-detail">
                                <template v-if="result.residual !== null">
                                    {{
                                        tr('benchLab.readings.residual', {
                                            value: reading(result.residual),
                                            unit: result.unit === 'um' ? 'µm' : result.unit,
                                        })
                                    }}
                                    ·
                                </template>
                                {{
                                    tr('benchLab.readings.limit', {
                                        target: reading(result.target),
                                        tolerance: reading(result.tolerance),
                                        unit: result.unit === 'um' ? 'µm' : result.unit,
                                    })
                                }}
                            </small>
                            <small
                                v-if="result.basis && result.status !== 'unreachable'"
                                class="assertion-detail"
                                data-basis
                                :title="result.basis.omitted?.map((o) => o.detail).join('\n')"
                                >{{
                                    tr('benchLab.readings.basis', {
                                        basis: basisText(result.basis),
                                    })
                                }}</small
                            >
                            <span v-if="result.break" class="assertion-detail">
                                {{
                                    tr('benchLab.readings.interrupted', {
                                        location: interruption(result.break),
                                    })
                                }}
                            </span>
                            <small v-else class="assertion-detail note"
                                >{{ result.at.element }} / {{ result.at.port }}</small
                            >
                        </li>
                    </ul>
                </div>

                <div
                    v-if="labCheck.handedness?.length || labCheck.relays?.length"
                    class="lab-section"
                >
                    <h4>{{ tr('benchLab.readings.optics') }}</h4>
                    <ul class="lab-readings">
                        <li
                            v-for="hand in labCheck.handedness ?? []"
                            :key="`hand:${hand.branch}`"
                            :data-handedness-reading="hand.branch"
                        >
                            {{
                                tr('benchLab.readings.handedness', {
                                    branch: hand.branch,
                                    count: hand.reflections,
                                    hand: tr(
                                        hand.handedness < 0
                                            ? 'benchLab.readings.mirrored'
                                            : 'benchLab.readings.upright',
                                    ),
                                })
                            }}
                        </li>
                        <li
                            v-for="relay in labCheck.relays ?? []"
                            :key="`relay:${relay.id}`"
                            :data-relay-reading="relay.id"
                            :class="{ 'failed-assertion': relay.checks.some((c) => !c.passed) }"
                        >
                            <b>{{
                                tr('benchLab.readings.relay', {
                                    id: relay.id,
                                    branch: relay.branch,
                                })
                            }}</b>
                            <span v-if="relay.abcd"
                                >A {{ reading(relay.abcd[0]) }} · B {{ reading(relay.abcd[1]) }} mm
                                · C {{ reading(relay.abcd[2]) }} /mm · D
                                {{ reading(relay.abcd[3]) }}</span
                            >
                            <span v-if="relay.pupil_image_mm !== null" class="note">{{
                                tr('benchLab.readings.pupilImage', {
                                    stop: relay.pupil_stop ?? '',
                                    target: relay.pupil_target ?? '',
                                    value: reading(relay.pupil_image_mm),
                                })
                            }}</span>
                            <span
                                v-for="check in relay.checks"
                                :key="check.kind"
                                :class="['badge', check.passed ? 'badge-ok' : 'badge-bad']"
                                >{{ check.kind }}
                                <template v-if="check.value !== null">{{
                                    reading(check.value)
                                }}</template></span
                            >
                            <span
                                v-for="check in relay.checks.filter((c) => c.deviations_mm)"
                                :key="`dev:${check.kind}`"
                                class="note"
                                >{{
                                    tr('benchLab.readings.deviations', {
                                        values: (check.deviations_mm ?? [])
                                            .map((v) => reading(v))
                                            .join(' / '),
                                    })
                                }}</span
                            >
                            <span
                                v-if="relay.checks.find((c) => c.basis)?.basis"
                                class="note"
                                data-basis
                                >{{
                                    tr('benchLab.readings.basis', {
                                        basis: basisText(relay.checks.find((c) => c.basis)!.basis!),
                                    })
                                }}</span
                            >
                            <span v-if="relay.break" class="note">{{
                                interruption(relay.break)
                            }}</span>
                        </li>
                    </ul>
                </div>

                <div
                    v-if="
                        labCheck.periscopes?.length ||
                        labCheck.folds?.length ||
                        labCheck.zones?.length
                    "
                    class="lab-section"
                >
                    <h4>{{ tr('benchLab.readings.layout') }}</h4>
                    <ul class="lab-readings">
                        <li
                            v-for="periscope in labCheck.periscopes ?? []"
                            :key="`periscope:${periscope.id}`"
                            :data-periscope-reading="periscope.id"
                            :class="{ 'failed-assertion': periscope.conflicts.length > 0 }"
                        >
                            <b>{{ tr('benchLab.readings.periscope', { id: periscope.id }) }}</b>
                            <span v-if="periscope.exit_height_mm !== null">{{
                                tr('benchLab.readings.exitHeight', {
                                    value: reading(periscope.exit_height_mm),
                                })
                            }}</span>
                            <span v-if="periscope.min_clearance_mm !== null">{{
                                tr('benchLab.readings.clearance', {
                                    value: reading(periscope.min_clearance_mm),
                                    element: periscope.nearest_element ?? '',
                                })
                            }}</span>
                            <span v-if="periscope.conflicts.length" class="badge badge-bad">{{
                                tr('benchLab.readings.conflicts', {
                                    elements: periscope.conflicts.join(', '),
                                })
                            }}</span>
                            <span v-if="periscope.break" class="note">{{
                                interruption(periscope.break)
                            }}</span>
                        </li>
                        <li
                            v-for="fold in labCheck.folds ?? []"
                            :key="`fold:${fold.id}`"
                            :data-fold-reading="fold.id"
                        >
                            <b>{{ tr('benchLab.readings.fold', { id: fold.id }) }}</b>
                            <span v-if="fold.break" class="note">
                                {{ tr('benchLab.readings.unavailable') }} ·
                                {{ interruption(fold.break) }}
                            </span>
                            <span v-for="(path, index) in fold.paths" :key="index">
                                <span v-if="fold.paths.length > 1" class="note"
                                    >{{ path.source }} · {{ path.wavelength_nm }} nm ·
                                </span>
                                {{
                                    tr('benchLab.readings.length', {
                                        value: reading(path.length_mm),
                                    })
                                }}
                                <span v-if="path.break" class="note">
                                    · {{ interruption(path.break) }}</span
                                >
                            </span>
                        </li>
                        <li
                            v-for="zone in labCheck.zones ?? []"
                            :key="`zone:${zone.id}`"
                            :data-zone-reading="zone.id"
                            :class="{ 'failed-assertion': zone.out_of_bounds.length > 0 }"
                        >
                            <b>{{ zone.label || zone.id }}</b>
                            <span>{{
                                tr('benchLab.readings.zoneArea', {
                                    value: reading(zone.area_mm2 / 100),
                                    count: zone.members.length,
                                })
                            }}</span>
                            <span v-if="zone.out_of_bounds.length" class="badge badge-bad">{{
                                tr('benchLab.readings.outOfZone', {
                                    elements: zone.out_of_bounds.join(', '),
                                })
                            }}</span>
                        </li>
                    </ul>
                </div>

                <dl class="metrics">
                    <div>
                        <dt>{{ tr('benchLab.readiness') }}</dt>
                        <dd>
                            {{ labCheck.readiness_score }}
                            <small>/ 100</small>
                        </dd>
                    </div>
                    <div>
                        <dt>{{ tr('benchLab.errors') }}</dt>
                        <dd>{{ labCheck.counts.errors }}</dd>
                    </div>
                    <div>
                        <dt>{{ tr('benchLab.warnings') }}</dt>
                        <dd>{{ labCheck.counts.warnings }}</dd>
                    </div>
                    <div>
                        <dt>{{ tr('benchLab.beamDumps') }}</dt>
                        <dd>{{ labCheck.recommended_beam_dumps.length }}</dd>
                    </div>
                </dl>

                <div v-if="labCheck.issues.length" class="lab-section">
                    <h4>{{ tr('benchLab.issues') }} ({{ labCheck.issues.length }})</h4>
                    <ul class="issue-list">
                        <li
                            v-for="(issue, idx) in labCheck.issues"
                            :key="idx"
                            class="issue-row"
                            :class="`sev-${issue.severity}`"
                        >
                            <span
                                class="badge"
                                :class="
                                    issue.severity === 'error'
                                        ? 'badge-bad'
                                        : issue.severity === 'warning'
                                          ? 'badge-warn'
                                          : 'badge-info'
                                "
                            >
                                {{ issue.code }}
                            </span>
                            <button
                                v-if="issue.element_id"
                                type="button"
                                class="small"
                                @click="emit('select', issue.element_id)"
                            >
                                {{ issue.element_id }}
                            </button>
                            <div class="issue-text">
                                <span>{{ issue.message }}</span>
                                <small v-if="issue.branch" class="note">
                                    {{ issue.branch }}
                                </small>
                            </div>
                        </li>
                    </ul>
                </div>
                <p v-else class="note ok-inline">
                    <CheckCircle2 :size="14" /> {{ tr('benchLab.allClear') }}
                </p>

                <div v-if="labCheck.recommended_beam_dumps.length" class="lab-section">
                    <h4><AlertTriangle :size="13" /> {{ tr('benchLab.beamDumpSection') }}</h4>
                    <div class="table-scroll">
                        <table>
                            <thead>
                                <tr>
                                    <th>{{ tr('bench.branch') }}</th>
                                    <th>{{ tr('benchLab.recommendedPos') }}</th>
                                    <th>{{ tr('bench.sourcePower') }}</th>
                                    <th></th>
                                </tr>
                            </thead>
                            <tbody>
                                <tr v-for="(bd, i) in labCheck.recommended_beam_dumps" :key="i">
                                    <td>{{ bd.branch }}</td>
                                    <td>
                                        {{ bd.position.map((v) => v.toFixed(1)).join(', ') }} mm
                                    </td>
                                    <td>{{ bd.power.toFixed(3) }}</td>
                                    <td>
                                        <button
                                            type="button"
                                            class="small primary"
                                            :disabled="labStale"
                                            @click="
                                                emit(
                                                    'addBeamDump',
                                                    bd.position,
                                                    bd.direction,
                                                    bd.branch,
                                                )
                                            "
                                        >
                                            + {{ tr('benchLab.placeDump') }}
                                        </button>
                                    </td>
                                </tr>
                            </tbody>
                        </table>
                    </div>
                </div>

                <div class="eng-two-col">
                    <div v-if="labCheck.gaussian.hits.length" class="lab-section">
                        <h4>{{ tr('benchLab.gaussianClearance') }}</h4>
                        <div class="table-scroll">
                            <table>
                                <thead>
                                    <tr>
                                        <th>ID</th>
                                        <th>ω (mm)</th>
                                        <th>2ω (mm)</th>
                                        <th>R_ap / ω</th>
                                        <th>{{ tr('opt.status') }}</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr
                                        v-for="(h, i) in labCheck.gaussian.hits"
                                        :key="i"
                                        @click="emit('select', h.element_id)"
                                    >
                                        <td>
                                            <b>{{ h.element_id }}</b>
                                        </td>
                                        <td>{{ fmt(h.beam_radius_mm, 3) }}</td>
                                        <td>{{ fmt(h.beam_diameter_mm, 3) }}</td>
                                        <td>{{ fmt(h.clearance_ratio, 2) }}×</td>
                                        <td>
                                            <span
                                                class="badge"
                                                :class="
                                                    h.status === 'ok'
                                                        ? 'badge-ok'
                                                        : h.status === 'tight'
                                                          ? 'badge-warn'
                                                          : 'badge-bad'
                                                "
                                            >
                                                {{ h.status }}
                                            </span>
                                        </td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </div>

                    <div v-if="labCheck.clamping.length" class="lab-section">
                        <h4>{{ tr('benchLab.clampingTable') }}</h4>
                        <div class="table-scroll">
                            <table>
                                <thead>
                                    <tr>
                                        <th>ID</th>
                                        <th>{{ tr('benchLab.hole') }}</th>
                                        <th>Δ (mm)</th>
                                        <th>{{ tr('benchLab.clampingStatus') }}</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <tr
                                        v-for="c in labCheck.clamping"
                                        :key="c.element_id"
                                        @click="emit('select', c.element_id)"
                                    >
                                        <td>
                                            <b>{{ c.element_id }}</b>
                                        </td>
                                        <td>C{{ c.hole[0] }}, R{{ c.hole[1] }}</td>
                                        <td>{{ fmt(c.offset_mm, 1) }}</td>
                                        <td>
                                            <span
                                                class="badge"
                                                :class="
                                                    c.style === 'direct_hole'
                                                        ? 'badge-ok'
                                                        : c.style === 'clamping_fork'
                                                          ? 'badge-info'
                                                          : 'badge-bad'
                                                "
                                            >
                                                {{ c.style }}
                                            </span>
                                        </td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </div>
                </div>
            </template>
        </div>

        <!-- TAB 2: ALIGNMENT & BEAM-WALKING GUIDE -->
        <div v-else-if="subtab === 'alignment'" class="lab-body">
            <p v-if="guideError" role="alert" class="json-error">{{ guideError }}</p>
            <p v-else-if="guidePending && !guide" class="note">{{ tr('benchLab.loading') }}</p>
            <template v-else-if="guide">
                <div class="lab-section">
                    <h4>{{ tr('benchLab.buildSteps') }}</h4>
                    <div class="table-scroll">
                        <table>
                            <thead>
                                <tr>
                                    <th>#</th>
                                    <th>ID</th>
                                    <th>{{ tr('benchLab.hole') }}</th>
                                    <th>Z (mm)</th>
                                    <th>{{ tr('optical.mount') }}</th>
                                    <th>ω (mm)</th>
                                    <th>{{ tr('benchLab.instruction') }}</th>
                                </tr>
                            </thead>
                            <tbody>
                                <tr
                                    v-for="s in guide.steps"
                                    :key="s.step"
                                    @click="emit('select', s.id)"
                                >
                                    <td>{{ s.step }}</td>
                                    <td>
                                        <b>{{ s.id }}</b>
                                    </td>
                                    <td>
                                        C{{ s.hole[0] }}, R{{ s.hole[1] }}
                                        <small class="note">
                                            (Δ{{ s.hole_offset.toFixed(1) }} mm)
                                        </small>
                                    </td>
                                    <td>{{ s.beam_height_z.toFixed(1) }}</td>
                                    <td>{{ s.mount_part ?? '—' }}</td>
                                    <td>{{ fmt(s.expected_beam_radius_mm, 2) }}</td>
                                    <td>{{ s.instruction }}</td>
                                </tr>
                            </tbody>
                        </table>
                    </div>
                </div>

                <div v-if="guide.sensitivities.length" class="lab-section">
                    <h4>{{ tr('benchLab.screwSensitivity') }}</h4>
                    <div class="table-scroll">
                        <table>
                            <thead>
                                <tr>
                                    <th>ID</th>
                                    <th>TPI</th>
                                    <th>mrad / ¼ turn</th>
                                    <th>{{ tr('benchLab.downstream') }}</th>
                                    <th>Δspot / ¼ turn</th>
                                </tr>
                            </thead>
                            <tbody>
                                <tr
                                    v-for="sen in guide.sensitivities"
                                    :key="sen.element_id"
                                    @click="emit('select', sen.element_id)"
                                >
                                    <td>
                                        <b>{{ sen.element_id }}</b>
                                    </td>
                                    <td>{{ sen.screw_tpi }}</td>
                                    <td>{{ fmt(sen.mrad_per_quarter_turn, 3) }}</td>
                                    <td>
                                        <div v-for="t in sen.targets" :key="t.target_id">
                                            {{ t.target_id }}
                                            <span class="note"
                                                >({{ t.distance_mm.toFixed(0) }} mm)</span
                                            >
                                        </div>
                                        <span v-if="!sen.targets.length">—</span>
                                    </td>
                                    <td>
                                        <div v-for="t in sen.targets" :key="t.target_id">
                                            <b>{{ fmt(t.spot_shift_mm_per_quarter_turn, 3) }} mm</b>
                                        </div>
                                        <span v-if="!sen.targets.length">—</span>
                                    </td>
                                </tr>
                            </tbody>
                        </table>
                    </div>
                </div>
            </template>
        </div>

        <!-- TAB 3: STANDARD LAB HARDWARE CATALOG -->
        <div v-else class="lab-body">
            <div class="controls" style="margin-bottom: 10px">
                <input
                    v-model="catalogQuery"
                    type="search"
                    :placeholder="tr('benchLab.searchCatalog')"
                    @input="loadCatalog"
                />
                <select v-model="catalogKind" @change="loadCatalog">
                    <option value="">{{ tr('benchLab.allKinds') }}</option>
                    <option value="mirror">{{ tr('bench.kinds.mirror') }}</option>
                    <option value="curved_mirror">{{ tr('bench.kinds.curved_mirror') }}</option>
                    <option value="lens">{{ tr('bench.kinds.lens') }}</option>
                    <option value="splitter">{{ tr('bench.kinds.splitter') }}</option>
                    <option value="pbs">{{ tr('bench.kinds.pbs') }}</option>
                    <option value="waveplate">{{ tr('bench.kinds.waveplate') }}</option>
                    <option value="polarizer">{{ tr('bench.kinds.polarizer') }}</option>
                    <option value="detector">{{ tr('bench.kinds.detector') }}</option>
                    <option value="aperture">{{ tr('bench.kinds.aperture') }}</option>
                    <option value="blocker">{{ tr('bench.kinds.blocker') }}</option>
                </select>
            </div>
            <p v-if="catalogError" role="alert" class="json-error">{{ catalogError }}</p>
            <div class="table-scroll">
                <table>
                    <thead>
                        <tr>
                            <th>{{ tr('benchLab.part') }}</th>
                            <th>{{ tr('bench.kind') }}</th>
                            <th>{{ tr('bench.aperture') }}</th>
                            <th>{{ tr('optical.mount') }}</th>
                            <th>{{ tr('benchLab.price') }}</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="item in catalogItems" :key="item.part">
                            <td>
                                <b>{{ item.part }}</b>
                                <div class="note">{{ item.name }}</div>
                            </td>
                            <td>{{ tr('bench.kinds.' + item.kind) }}</td>
                            <td>R {{ item.aperture }} mm</td>
                            <td>
                                {{ item.mount.part }}
                                <div class="note">
                                    {{ item.mount.base_size.join('×') }} mm ·
                                    {{ item.clamping_style }}
                                </div>
                            </td>
                            <td>${{ item.typical_price_usd }}</td>
                            <td>
                                <button
                                    type="button"
                                    class="small primary"
                                    @click="emit('addFromCatalog', item)"
                                >
                                    + {{ tr('benchLab.addToBench') }}
                                </button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>
    </section>
</template>

<style scoped>
.stage-controls {
    margin: 6px 10px 10px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    min-width: 0;
}
.stage-control {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
    margin: 4px 0;
}
.stage-control input[type='number'] {
    width: 7em;
}
.stage-control .json-error {
    flex-basis: 100%;
    margin: 0;
}
.lab-panel {
    margin-top: 10px;
}
.lab-tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    flex-wrap: wrap;
}
.lab-tabs button[role='tab'] {
    border: 1px solid transparent;
    background: transparent;
    padding: 5px 10px;
    font-size: 12px;
}
.lab-tabs button[role='tab'][aria-selected='true'] {
    background: var(--panel);
    border-color: var(--border);
    color: var(--accent);
    font-weight: 600;
}
.lab-body {
    padding: 10px 12px;
}
.lab-section {
    margin-top: 12px;
}
.lab-section h4 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
}
.lab-revision {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    color: var(--muted);
    font-size: 12px;
}
.modal-heatmap {
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
    font-size: 12px;
}
.modal-heatmap th,
.modal-heatmap td {
    border: 1px solid var(--border, #d0d7de);
    padding: 3px 8px;
    text-align: right;
}
.lab-readings {
    display: grid;
    gap: 6px;
    list-style: none;
    margin: 0;
    padding: 0;
}
.lab-readings li {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    align-items: baseline;
    padding: 6px 8px;
    border: 1px solid var(--border);
    background: var(--surface);
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
}
.lab-readings .failed-assertion {
    border-color: var(--danger);
}
.lab-readings .undecided-assertion {
    border-color: var(--warning);
}
.assertion-detail {
    flex-basis: 100%;
}
.arm-swatch {
    display: inline-block;
    width: 1em;
    height: 0.32em;
    vertical-align: middle;
}
.issue-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
}
.issue-row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 2px;
    background: var(--surface);
    font-size: 12px;
}
.issue-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
}
.ok-inline {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--positive);
}
</style>
