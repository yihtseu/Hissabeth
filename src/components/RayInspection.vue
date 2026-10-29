<script setup lang="ts">
import { tr } from '../i18n';
import { computed, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import { fmt } from '../chart';
import { checkRevision } from '../provenance';
import type {
    InspectLaunch,
    OpticalSystem,
    ParaxialRow,
    ParaxialTrace,
    Point3,
    TraceFailureInfo,
    TraceInspect,
} from '../types';
import { fieldLabel } from '../validation';
import RayGrid, { type GridGroup, type GridRow } from './RayGrid.vue';

const props = defineProps<{
    system: OpticalSystem;
    revision: number;
    online: boolean;
    dirty: boolean;
}>();

type Mode = 'paraxial' | 'pupil' | 'object';

/** Snapshot taken when the request starts; result labels use it, never the live controls. */
interface Prov {
    revision: number;
    mode: Mode;
    field: string;
    wavelength: number;
}

const mode = ref<Mode>('paraxial');
const fieldIndex = ref(0);
const wavelengthIndex = ref(props.system.reference_wavelength);
// Raw field text: a blank field stays blank until the user supplies a value, and is parsed only when a request is built.
const pupilX = ref('0');
const pupilY = ref('0');
const sourceX = ref('0');
const sourceY = ref('0');
const tanX = ref('0');
const tanY = ref('0');
const parseField = (text: string): number => (text.trim() === '' ? NaN : Number(text));

const paraxial = ref<{ data: ParaxialTrace; prov: Prov } | null>(null);
const inspected = ref<{ data: TraceInspect; prov: Prov } | null>(null);
const error = ref('');
const loading = ref(false);
const showRaw = ref(false);
let token = 0;

const infinite = computed(() => props.system.object_distance === null);
const blocked = computed(() => mode.value === 'object' && infinite.value);

// Keep the chosen wavelength entry across revisions: by index while it still holds the value, else by value.
let selectedValue = props.system.wavelengths[wavelengthIndex.value]?.value;
watch(wavelengthIndex, (i) => {
    selectedValue = props.system.wavelengths[i]?.value;
});
watch(
    () => props.system,
    (sys) => {
        if (fieldIndex.value >= sys.fields.length) fieldIndex.value = 0;
        if (sys.wavelengths[wavelengthIndex.value]?.value === selectedValue) return;
        const kept = sys.wavelengths.findIndex((w) => w.value === selectedValue);
        wavelengthIndex.value = kept >= 0 ? kept : sys.reference_wavelength;
    },
);

/** Inputs that matter to the current mode; any change drops the displayed result. */
const inputKey = computed(() =>
    JSON.stringify([
        mode.value,
        wavelengthIndex.value,
        mode.value === 'object'
            ? [sourceX.value, sourceY.value, tanX.value, tanY.value]
            : mode.value === 'pupil'
              ? [fieldIndex.value, pupilX.value, pupilY.value]
              : fieldIndex.value,
    ]),
);
/** Drops the displayed result and cancels any in-flight request; never starts a new one. */
function invalidate(): void {
    token++;
    loading.value = false;
    paraxial.value = null;
    inspected.value = null;
    error.value = '';
    showRaw.value = false;
}
watch([inputKey, () => props.revision, () => props.online], invalidate);

async function run(): Promise<void> {
    if (!props.online || blocked.value || loading.value) return;
    const typed =
        mode.value === 'pupil'
            ? [pupilX.value, pupilY.value]
            : mode.value === 'object'
              ? [sourceX.value, sourceY.value, tanX.value, tanY.value]
              : [];
    const parsed = typed.map(parseField);
    if (!parsed.every(Number.isFinite)) {
        error.value = tr('ray.invalidInput');
        return;
    }
    const [a, b, c, d] = parsed as [number, number, number, number];
    const mine = ++token;
    const sys = props.system;
    const field = Math.min(fieldIndex.value, sys.fields.length - 1);
    const wavelength = sys.wavelengths[wavelengthIndex.value]!.value;
    const prov: Prov = {
        revision: props.revision,
        mode: mode.value,
        field: fieldLabel(sys, field),
        wavelength,
    };
    const launch: InspectLaunch | null =
        prov.mode === 'pupil'
            ? { kind: 'pupil', pupil: [a, b], field_index: field }
            : prov.mode === 'object'
              ? { kind: 'object', point: [a, b], tangent: [c, d] }
              : null;
    paraxial.value = null;
    inspected.value = null;
    error.value = '';
    loading.value = true;
    /** Superseded replies are dropped quietly; a reply from a newer saved revision is reported. */
    const accept = (replied: number): boolean => {
        const check = checkRevision(prov.revision, props.revision, replied);
        if (check === 'newer') {
            error.value = tr('ray.superseded', { replied: replied, captured: prov.revision });
        }
        return check === 'ok';
    };
    try {
        if (launch === null) {
            const data = await callTool('paraxial_trace', { wavelength, field_index: field });
            if (mine === token && accept(data.revision)) paraxial.value = { data, prov };
        } else {
            const data = await callTool('trace_inspect', { wavelength, launch });
            if (mine === token && accept(data.revision)) inspected.value = { data, prov };
        }
    } catch (e) {
        if (mine === token) error.value = errorMessage(e);
    } finally {
        if (mine === token) loading.value = false;
    }
}

// ---- formatting ---------------------------------------------------------------------------------
const num = (v: number): string => fmt(v, 6);
const cells = (...values: number[][]): string[] => values.flat().map(num);
const DASH = '—';
const dashes = (n: number): string[] => Array.from({ length: n }, () => DASH);

const PARAXIAL_GROUPS: GridGroup[] = [
    {
        get label() {
            return tr('ray.groupPoint');
        },
        cols: ['x', 'y', 'z'],
    },
    {
        get label() {
            return tr('ray.groupSlopeIn');
        },
        cols: ['dx/dz', 'dy/dz'],
    },
    {
        get label() {
            return tr('ray.groupSlopeOut');
        },
        cols: ['dx/dz', 'dy/dz'],
    },
    {
        get label() {
            return tr('ray.groupReducedIn');
        },
        cols: ['x', 'y'],
    },
    {
        get label() {
            return tr('ray.groupReducedOut');
        },
        cols: ['x', 'y'],
    },
    {
        get label() {
            return tr('ray.groupSignedIndex');
        },
        cols: ['in', 'out'],
    },
];
function paraxialLabel(r: ParaxialRow): string {
    return r.endpoint === 'object'
        ? tr('ray.rowObject')
        : r.endpoint === 'image'
          ? tr('ray.rowImage')
          : tr('ray.rowSurface', { n: (r.surface_index ?? 0) + 1 });
}
function paraxialRows(rows: ParaxialRow[]): GridRow[] {
    return rows.map((r) => ({
        label: paraxialLabel(r),
        cells: cells(r.point, r.slope_in, r.slope_out, r.reduced_in, r.reduced_out, [
            r.signed_index_in,
            r.signed_index_out,
        ]),
    }));
}

const REAL_GROUPS: GridGroup[] = [
    {
        get label() {
            return tr('ray.groupPoint');
        },
        cols: ['x', 'y', 'z'],
    },
    {
        get label() {
            return tr('ray.groupIncoming');
        },
        cols: ['L', 'M', 'N'],
    },
    {
        get label() {
            return tr('ray.groupNormal');
        },
        cols: ['Nx', 'Ny', 'Nz'],
    },
    {
        get label() {
            return tr('ray.groupOutgoing');
        },
        cols: ['L′', 'M′', 'N′'],
    },
    {
        get label() {
            return tr('ray.groupIndex');
        },
        cols: [tr('remaining.m10'), tr('remaining.m11')],
    },
    {
        get label() {
            return tr('ray.groupOpl');
        },
        cols: [tr('ray.oplSigned')],
    },
];
function failureWhere(f: TraceFailureInfo): string {
    if (f.endpoint === 'object') return tr('ray.rowObject');
    if (f.endpoint === 'image') return tr('ray.rowImage');
    return f.surface === null ? tr('ray.rowLaunch') : tr('ray.rowSurface', { n: f.surface + 1 });
}
const KIND_LABELS: Record<TraceFailureInfo['kind'], string> = {
    get invalid() {
        return tr('ray.failInvalid');
    },
    get intersection() {
        return tr('ray.failIntersection');
    },
    get blocked() {
        return tr('ray.failBlocked');
    },
    get total_internal_reflection() {
        return tr('ray.failTir');
    },
    get aiming() {
        return tr('ray.failAiming');
    },
    get image() {
        return tr('ray.failImage');
    },
};
const realRows = computed<GridRow[]>(() => {
    const d = inspected.value?.data;
    if (!d?.ray) return [];
    const r = d.ray;
    const rows: GridRow[] = [
        {
            get label() {
                return tr('ray.rowLaunch');
            },
            cells: [
                ...cells(r.points[0]!),
                ...dashes(6),
                ...cells(r.launch_direction),
                ...dashes(2),
                '0',
            ],
            get title() {
                return tr('ray.launchTitle');
            },
        },
    ];
    r.segments.forEach((s, i) => {
        rows.push({
            get label() {
                return tr('ray.rowSurface', { n: i + 1 });
            },
            cells: cells(s.point, s.incoming, s.normal, s.direction, [
                s.n_before,
                s.n_after,
                s.opl,
            ]),
        });
    });
    if (r.image_hit) {
        const s = r.image_hit;
        rows.push({
            get label() {
                return tr('ray.rowImage');
            },
            cells: cells(s.point, s.incoming, s.normal, s.direction, [
                s.n_before,
                s.n_after,
                s.opl,
            ]),
        });
    }
    if (d.failure) {
        const f = d.failure;
        // The failed point is shown as attempted coordinates only; no interaction or OPL is invented for it.
        rows.push({
            label: failureWhere(f),
            cells: [...(f.point ? cells(f.point) : dashes(3)), ...dashes(12)],
            failed: true,
            title: f.message,
        });
    }
    return rows;
});

const inspectionRay = computed(() => inspected.value?.data.ray ?? null);
const objectSag = computed(() => inspectionRay.value?.object_point ?? null);
const vec = (p: Point3 | number[]): string => `(${p.map(num).join(', ')})`;
const rawText = computed(() => {
    if (!showRaw.value) return '';
    const value = paraxial.value?.data ?? inspected.value?.data;
    return value ? JSON.stringify(value, null, 1) : '';
});
const shownRevision = computed(
    () => paraxial.value?.prov.revision ?? inspected.value?.prov.revision ?? props.revision,
);
</script>

<template>
    <section class="panel" aria-labelledby="ri-title">
        <div class="panel-head">
            <h3 id="ri-title">{{ tr('ray.title') }}</h3>
            <span>{{ tr('fragments.computedOnTheSavedSystemRevision') }} {{ shownRevision }}</span>
        </div>
        <div v-if="dirty" role="status" class="banner warn inset">
            {{ tr('fragments.resultsUseTheSavedSystemRevision') }} {{ revision
            }}{{ tr('fragments.unappliedDraftChangesAreNotIncludedUntilYouApplyThem') }}
        </div>
        <form class="analysis-controls" @submit.prevent="run" @input="invalidate">
            <div class="controls">
                <label
                    >{{ tr('ray.mode') }}
                    <select v-model="mode" class="wide-select">
                        <option value="paraxial">{{ tr('ray.modeParaxial') }}</option>
                        <option value="pupil">{{ tr('ray.modePupil') }}</option>
                        <option value="object" :disabled="infinite">
                            {{ tr('ray.modeObject')
                            }}{{ infinite ? ' (needs a finite object)' : '' }}
                        </option>
                    </select>
                </label>
                <label v-if="mode !== 'object'"
                    >{{ tr('ray.field') }}
                    <select v-model.number="fieldIndex">
                        <option v-for="(_, i) in system.fields" :key="i" :value="i">
                            {{ fieldLabel(system, i) }}
                        </option>
                    </select>
                </label>
                <label
                    >{{ tr('ray.wavelength') }}
                    <select v-model.number="wavelengthIndex">
                        <option v-for="(w, i) in system.wavelengths" :key="i" :value="i">
                            {{ i + 1 }}: {{ w.value }} nm
                        </option>
                    </select>
                </label>
                <template v-if="mode === 'pupil'">
                    <label
                        >{{ tr('ray.pupilX') }}
                        <input
                            type="number"
                            step="any"
                            required
                            :value="pupilX"
                            @input="pupilX = ($event.target as HTMLInputElement).value"
                    /></label>
                    <label
                        >{{ tr('ray.pupilY') }}
                        <input
                            type="number"
                            step="any"
                            required
                            :value="pupilY"
                            @input="pupilY = ($event.target as HTMLInputElement).value"
                    /></label>
                </template>
                <template v-if="mode === 'object'">
                    <label
                        >{{ tr('ray.sourceX') }}
                        <input
                            type="number"
                            step="any"
                            required
                            :value="sourceX"
                            @input="sourceX = ($event.target as HTMLInputElement).value"
                    /></label>
                    <label
                        >{{ tr('ray.sourceY') }}
                        <input
                            type="number"
                            step="any"
                            required
                            :value="sourceY"
                            @input="sourceY = ($event.target as HTMLInputElement).value"
                    /></label>
                    <label
                        >{{ tr('ray.tanX') }}
                        <input
                            type="number"
                            step="any"
                            required
                            :value="tanX"
                            @input="tanX = ($event.target as HTMLInputElement).value"
                    /></label>
                    <label
                        >{{ tr('ray.tanY') }}
                        <input
                            type="number"
                            step="any"
                            required
                            :value="tanY"
                            @input="tanY = ($event.target as HTMLInputElement).value"
                    /></label>
                </template>
                <button type="submit" class="primary" :disabled="!online || blocked || loading">
                    {{ tr('ray.run') }}
                </button>
            </div>
            <p v-if="mode === 'paraxial'" class="note">
                {{ tr('ray.noteParaxial') }}
            </p>
            <p v-else-if="mode === 'pupil'" class="note">
                {{ tr('ray.notePupil') }}
            </p>
            <p v-else class="note">
                {{ tr('ray.noteObject') }}
            </p>
            <p v-if="blocked" role="status" class="note warn-text">
                {{ tr('ray.noteBlocked') }}
            </p>
        </form>
        <div v-if="error" role="alert" class="banner error inset">{{ error }}</div>
        <p v-if="loading" class="empty" role="status">{{ tr('common.computing') }}</p>
        <p v-else-if="!paraxial && !inspected && !error" class="empty">
            {{ online ? tr('ray.noResult') : tr('common.offline') }}
        </p>

        <div v-if="paraxial" class="result">
            <p class="note" role="status">
                {{ tr('fragments.revision') }} {{ paraxial.prov.revision }} · λ
                {{ paraxial.prov.wavelength }} {{ tr('fragments.nmField') }}
                {{ paraxial.prov.field }} {{ tr('fragments.entrancePupilRadius') }}
                {{ num(paraxial.data.entrance_pupil_radius) }} {{ tr('fragments.mmFieldValue')
                }}{{ paraxial.data.field.map(num).join(', ') }})
            </p>
            <p class="note">{{ paraxial.data.model }}. {{ paraxial.data.basis }}</p>
            <RayGrid
                :caption="tr('ray.gridParaxialAxis')"
                first="Surface"
                :groups="PARAXIAL_GROUPS"
                :rows="paraxialRows(paraxial.data.axis)"
            />
            <RayGrid
                :caption="tr('ray.gridParaxialChief')"
                first="Surface"
                :groups="PARAXIAL_GROUPS"
                :rows="paraxialRows(paraxial.data.chief)"
            />
        </div>

        <div v-if="inspected" class="result">
            <p class="note" role="status">
                {{ tr('fragments.revision') }} {{ inspected.prov.revision }} · λ
                {{ inspected.prov.wavelength }} nm ·
                {{
                    inspected.prov.mode === 'pupil'
                        ? tr('ray.provPupil', { field: inspected.prov.field })
                        : tr('ray.provObject')
                }}
            </p>
            <div v-if="inspected.data.failure" role="alert" class="banner warn">
                <strong
                    >{{ tr('fragments.rayFailedAt') }}
                    {{ failureWhere(inspected.data.failure) }}</strong
                >: {{ KIND_LABELS[inspected.data.failure.kind] }}.
                {{ inspected.data.failure.message }}
                <span v-if="inspected.data.failure.point"
                    >{{ tr('fragments.attemptedPoint') }}
                    {{ vec(inspected.data.failure.point) }} mm.</span
                >
                <span v-if="!inspected.data.ray">{{ tr('ray.noPartial') }}</span>
            </div>
            <dl v-if="inspectionRay" class="kv">
                <div>
                    <dt>{{ tr('ray.launchPoint') }}</dt>
                    <dd>{{ vec(inspectionRay.points[0]!) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('ray.launchDirection') }}</dt>
                    <dd>{{ vec(inspectionRay.launch_direction) }}</dd>
                </div>
                <div v-if="objectSag">
                    <dt>{{ tr('ray.sourcePoint') }}</dt>
                    <dd>{{ vec(objectSag) }}</dd>
                </div>
                <div>
                    <dt>{{ tr('ray.entranceOpl') }}</dt>
                    <dd>{{ num(inspectionRay.entrance_opl) }}</dd>
                </div>
                <div v-if="!inspected.data.failure">
                    <dt>{{ tr('ray.rayOpl') }}</dt>
                    <dd>{{ num(inspectionRay.opl) }}</dd>
                </div>
                <div v-if="inspectionRay.image_hit">
                    <dt>{{ tr('ray.imageHit') }}</dt>
                    <dd>{{ vec(inspectionRay.image_hit.point) }}</dd>
                </div>
            </dl>
            <p class="note">{{ inspected.data.coordinates }}</p>
            <p class="note">{{ inspected.data.optical_path }}</p>
            <RayGrid
                v-if="realRows.length"
                :caption="tr('ray.gridReal')"
                first="Surface"
                :groups="REAL_GROUPS"
                :rows="realRows"
            />
        </div>

        <details
            v-if="paraxial || inspected"
            class="data-table wide"
            @toggle="showRaw = ($event.target as HTMLDetailsElement).open"
        >
            <summary>{{ tr('common.rawJson') }}</summary>
            <pre v-if="showRaw" class="raw">{{ rawText }}</pre>
        </details>
    </section>
</template>
