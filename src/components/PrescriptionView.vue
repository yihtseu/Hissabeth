<script lang="ts">
import type { InjectionKey, Ref } from 'vue';

/**
 * Shared surface hover/selection state; same shape as the useSurfaceSelection() composable
 * contract, provided by the app shell and injected by every surface table/view.
 */
export interface SurfaceSelectionState {
    hoveredSurfaceIndex: Ref<number | null>;
    selectedSurfaceIndex: Ref<number | null>;
    hoverSurface: (index: number | null) => void;
    selectSurface: (index: number | null) => void;
    clearSurfaceSelection: () => void;
}
export const SURFACE_SELECTION: InjectionKey<SurfaceSelectionState> = Symbol(
    'hissabeth.surface-selection',
);
</script>
<script setup lang="ts">
import { computed, inject, nextTick, ref, watch } from 'vue';
import { PenLine } from 'lucide-vue-next';
import { tr } from '../i18n';
import type { OpticalSystem, OptimizeVariable, Surface } from '../types';
import { glassKey } from '../validation';

const props = defineProps<{
    system: OpticalSystem;
    /** Draft system to edit; when bound, inline edits emit update:modelValue instead of staying read-only. */
    modelValue?: OpticalSystem | null;
    compact?: boolean;
    chatOpen?: boolean;
    /** Allow switching into inline edit mode; without it the view is read-only. */
    editable?: boolean;
    materials?: string[];
    /** Shared optimization variables (same contract as SurfaceTable). */
    variables?: OptimizeVariable[];
}>();
const emit = defineEmits<{
    'update:modelValue': [value: OpticalSystem];
    'toggle-variable': [variable: OptimizeVariable];
}>();

/** Editable content comes from the bound draft; otherwise fall back to the saved system (read-only). */
const source = computed(() => props.modelValue ?? props.system);
const editing = ref(false);
const canEdit = computed(() => !!props.editable && !!props.modelValue);

const rows = computed(() => {
    let z = 0;
    let material = source.value.object_material;
    return source.value.surfaces.map((surface, i) => {
        const vertex = z;
        z += surface.thickness;
        if (surface.interaction === 'refract') material = surface.material;
        return {
            label: surface.label,
            interaction: surface.interaction,
            radius: surface.radius,
            thickness: surface.thickness,
            clear_aperture: surface.clear_aperture,
            aperture: surface.aperture,
            conic: surface.conic,
            vertex,
            material,
            stop: i === source.value.stop_index,
        };
    });
});
const number = (value: number) => Number(value.toPrecision(10)).toString();

const known = computed(() => {
    const set = new Set(props.materials ?? [source.value.object_material]);
    for (const g of source.value.custom_glasses) {
        set.add(glassKey(g));
        set.add(g.name);
    }
    return [...set];
});

// ---- shared surface hover/selection (locate + highlight) ---------------------
const selection = inject(SURFACE_SELECTION, null);
const localHovered = ref<number | null>(null);
const localSelected = ref<number | null>(null);
const hoveredSurfaceIndex = selection?.hoveredSurfaceIndex ?? localHovered;
const selectedSurfaceIndex = selection?.selectedSurfaceIndex ?? localSelected;
function hoverSurface(index: number | null): void {
    if (selection) selection.hoverSurface(index);
    else localHovered.value = index;
}
function selectSurface(index: number | null): void {
    if (selection) selection.selectSurface(index);
    else localSelected.value = index;
}
const scrollBox = ref<HTMLElement>();
watch(selectedSurfaceIndex, (index) => {
    if (index === null) return;
    void nextTick(() => {
        scrollBox.value
            ?.querySelector(`[data-surface-row="${index}"]`)
            ?.scrollIntoView({ block: 'nearest' });
    });
});

// ---- inline editing -----------------------------------------------------------
function patchSurface(index: number, patch: Partial<Surface>): void {
    if (!props.modelValue) return;
    emit('update:modelValue', {
        ...props.modelValue,
        surfaces: props.modelValue.surfaces.map((s, k) => (k === index ? { ...s, ...patch } : s)),
    });
}
function parseCell(raw: string): number | null {
    const value = Number.parseFloat(raw);
    return Number.isFinite(value) ? value : null;
}
function variableFor(kind: 'radius' | 'thickness', surface: number): OptimizeVariable | undefined {
    return props.variables?.find((v) => v.kind === kind && v.surface === surface);
}
function toggleVariable(kind: 'radius' | 'thickness', surface: number): void {
    emit('toggle-variable', { kind, surface });
}
</script>
<template>
    <section class="prescription-view">
        <div v-if="!compact || canEdit" class="rx-bar">
            <span v-if="canEdit" class="badge" :class="editing ? 'badge-info' : 'badge-ok'">{{
                editing ? tr('rx.editMode') : tr('rx.readOnly')
            }}</span>
            <button
                v-if="canEdit"
                type="button"
                class="rx-edit-toggle"
                :aria-pressed="editing"
                :aria-label="tr('rx.toggleEdit')"
                :title="tr('rx.toggleEdit')"
                @click="editing = !editing"
            >
                <PenLine :size="13" />
            </button>
        </div>
        <p v-if="!compact" class="optical-context">
            {{ tr('optical.currentPrescription')
            }}<template v-if="!chatOpen"> · {{ tr('optical.chatEdits') }}</template>
        </p>
        <div
            ref="scrollBox"
            class="table-scroll"
            tabindex="0"
            :aria-label="tr('tabs.Prescription')"
        >
            <table>
                <thead>
                    <tr>
                        <th>#</th>
                        <th>{{ tr('bench.label') }}</th>
                        <th>{{ tr('optical.interaction') }}</th>
                        <th>{{ tr('optical.radius') }} (mm)</th>
                        <th>{{ tr('optical.gap') }} (mm)</th>
                        <th>{{ tr('optical.material') }}</th>
                        <th>{{ tr('bench.aperture') }} (mm)</th>
                        <th>{{ tr('optical.vertex') }} (mm)</th>
                        <th>{{ tr('optical.conic') }}</th>
                    </tr>
                </thead>
                <tbody>
                    <tr
                        v-for="(s, i) in rows"
                        :key="i"
                        :data-surface-row="i"
                        :class="{
                            'row-hover': hoveredSurfaceIndex === i,
                            'row-selected': selectedSurfaceIndex === i,
                        }"
                        @mouseenter="hoverSurface(i)"
                        @mouseleave="hoverSurface(null)"
                        @click="selectSurface(i)"
                    >
                        <th scope="row">{{ i + 1 }}{{ s.stop ? ' ◉' : '' }}</th>
                        <td>{{ s.label || '—' }}</td>
                        <td>{{ tr('optical.' + s.interaction) }}</td>
                        <td v-if="editing && canEdit" class="rx-cell">
                            <span class="rx-input">
                                <input
                                    type="number"
                                    step="any"
                                    :value="number(s.radius)"
                                    :aria-label="tr('optical.radius')"
                                    @change="
                                        parseCell(($event.target as HTMLInputElement).value) !==
                                            null &&
                                        patchSurface(i, {
                                            radius: parseCell(
                                                ($event.target as HTMLInputElement).value,
                                            )!,
                                        })
                                    "
                                />
                                <button
                                    type="button"
                                    class="rx-var"
                                    :aria-pressed="!!variableFor('radius', i)"
                                    :aria-label="tr('rx.toggleVariable')"
                                    :title="tr('rx.toggleVariable')"
                                    @click.stop="toggleVariable('radius', i)"
                                >
                                    V
                                </button>
                            </span>
                        </td>
                        <td v-else>{{ number(s.radius) }}</td>
                        <td v-if="editing && canEdit" class="rx-cell">
                            <span class="rx-input">
                                <input
                                    type="number"
                                    step="any"
                                    :value="number(s.thickness)"
                                    :aria-label="tr('optical.gap')"
                                    @change="
                                        parseCell(($event.target as HTMLInputElement).value) !==
                                            null &&
                                        patchSurface(i, {
                                            thickness: parseCell(
                                                ($event.target as HTMLInputElement).value,
                                            )!,
                                        })
                                    "
                                />
                                <button
                                    type="button"
                                    class="rx-var"
                                    :aria-pressed="!!variableFor('thickness', i)"
                                    :aria-label="tr('rx.toggleVariable')"
                                    :title="tr('rx.toggleVariable')"
                                    @click.stop="toggleVariable('thickness', i)"
                                >
                                    V
                                </button>
                            </span>
                        </td>
                        <td v-else>{{ number(s.thickness) }}</td>
                        <td v-if="editing && canEdit && s.interaction === 'refract'">
                            <select
                                :value="source.surfaces[i].material"
                                :aria-label="tr('optical.material')"
                                @change="
                                    patchSurface(i, {
                                        material: ($event.target as HTMLSelectElement).value,
                                    })
                                "
                            >
                                <option v-for="m in known" :key="m" :value="m">{{ m }}</option>
                            </select>
                        </td>
                        <td v-else>{{ s.material }}</td>
                        <td v-if="editing && canEdit" class="rx-aperture">
                            <input
                                type="checkbox"
                                :checked="s.clear_aperture"
                                :aria-label="tr('rx.colClip')"
                                @change="
                                    patchSurface(i, {
                                        clear_aperture: ($event.target as HTMLInputElement).checked,
                                    })
                                "
                            />
                            <input
                                v-if="s.clear_aperture"
                                type="number"
                                step="any"
                                :value="number(s.aperture)"
                                :aria-label="tr('bench.aperture')"
                                @change="
                                    parseCell(($event.target as HTMLInputElement).value) !== null &&
                                    patchSurface(i, {
                                        aperture: parseCell(
                                            ($event.target as HTMLInputElement).value,
                                        )!,
                                    })
                                "
                            />
                        </td>
                        <td v-else>{{ s.clear_aperture ? number(s.aperture) : '—' }}</td>
                        <td>{{ number(s.vertex) }}</td>
                        <td>{{ number(s.conic) }}</td>
                    </tr>
                </tbody>
            </table>
        </div>
        <template v-if="!compact">
            <dl class="optical-properties prescription-spec">
                <dt>{{ tr('optical.object') }}</dt>
                <dd>
                    {{ source.object_label }} ·
                    {{ source.object_distance === null ? '∞' : source.object_distance + ' mm' }} ·
                    {{ source.object_material }}
                </dd>
                <dt>{{ tr('optical.image') }}</dt>
                <dd>{{ source.image_label }}</dd>
                <dt>{{ tr('optical.pupil') }}</dt>
                <dd>
                    {{ source.pupil.kind }}: {{ source.pupil.value }} · {{ source.pupil_mapping }}
                </dd>
                <dt>{{ tr('optical.environment') }}</dt>
                <dd>{{ source.temperature }} °C · {{ source.pressure }} Pa</dd>
                <template v-if="source.configurations?.length">
                    <dt>{{ tr('cfg.title') }}</dt>
                    <dd>
                        {{
                            tr('cfg.activeLine', {
                                name:
                                    source.configurations[source.active_configuration ?? 0]?.name ||
                                    '—',
                                i: (source.active_configuration ?? 0) + 1,
                                n: source.configurations.length,
                            })
                        }}
                    </dd>
                </template>
                <dt>{{ tr('optical.wavelength') }}</dt>
                <dd>
                    {{
                        source.wavelengths
                            .map(
                                (w, i) =>
                                    `${w.value} nm (${w.weight})${i === source.reference_wavelength ? ' ◉' : ''}`,
                            )
                            .join(' / ')
                    }}
                </dd>
                <dt>{{ tr('optical.fields') }}</dt>
                <dd>
                    {{ source.field_type }}:
                    {{ source.fields.map((f) => `(${f.x}, ${f.y}; ${f.weight})`).join(' / ') }}
                </dd>
            </dl>
            <p v-if="source.note">{{ source.note }}</p>
            <p>{{ tr('optical.fullReport') }}</p>
        </template>
    </section>
</template>
