<script setup lang="ts">
import { tr } from '../i18n';
import { computed, ref, watch } from 'vue';
import NumberField from './NumberField.vue';
import type { Configuration, ConfigurationOverride, OpticalSystem } from '../types';

const props = defineProps<{
    modelValue: OpticalSystem;
    materials: string[];
    online: boolean;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: OpticalSystem] }>();

const configurations = computed(() => props.modelValue.configurations ?? []);
const active = computed(() => props.modelValue.active_configuration ?? 0);
const selected = ref(0);
watch(
    configurations,
    (list) => {
        if (selected.value > Math.max(0, list.length - 1))
            selected.value = Math.max(0, list.length - 1);
    },
    { immediate: true },
);

const KINDS = ['thickness', 'radius', 'conic', 'aperture', 'material', 'object_distance'] as const;
type Kind = (typeof KINDS)[number];

function patch(p: Partial<OpticalSystem>): void {
    emit('update:modelValue', { ...props.modelValue, ...p });
}
function setConfigurations(next: Configuration[], nextActive = active.value): void {
    patch({
        configurations: next,
        active_configuration: Math.max(0, Math.min(nextActive, Math.max(0, next.length - 1))),
    });
}
function addConfiguration(): void {
    const next = [
        ...configurations.value,
        { name: tr('cfg.defaultName', { n: configurations.value.length + 1 }), overrides: [] },
    ];
    setConfigurations(next);
    selected.value = next.length - 1;
}
function removeConfiguration(i: number): void {
    const removedActive = i === active.value;
    setConfigurations(
        configurations.value.filter((_, k) => k !== i),
        removedActive ? 0 : i < active.value ? active.value - 1 : active.value,
    );
}
function rename(i: number, name: string): void {
    const next = configurations.value.slice();
    next[i] = { ...next[i]!, name };
    setConfigurations(next);
}
function mutateOverrides(
    i: number,
    mutate: (list: ConfigurationOverride[]) => ConfigurationOverride[],
): void {
    const next = configurations.value.slice();
    next[i] = { ...next[i]!, overrides: mutate(next[i]!.overrides) };
    setConfigurations(next);
}
function setOverride(i: number, j: number, o: ConfigurationOverride): void {
    mutateOverrides(i, (list) => list.map((x, m) => (m === j ? o : x)));
}
function defaultOverride(kind: Kind): ConfigurationOverride {
    switch (kind) {
        case 'thickness':
            return { kind, surface: 0, value: 0 };
        case 'radius':
            return { kind, surface: 0, value: 0 };
        case 'conic':
            return { kind, surface: 0, value: 0 };
        case 'aperture':
            return { kind, surface: 0, value: 10 };
        case 'material':
            return { kind, surface: 0, material: 'AIR' };
        case 'object_distance':
            return { kind, value: null };
    }
}
function rekind(i: number, j: number, kind: Kind): void {
    const previous = configurations.value[i]!.overrides[j]!;
    const next = defaultOverride(kind);
    if ('surface' in next && 'surface' in previous) next.surface = previous.surface;
    if ('value' in next && 'value' in previous && kind !== 'object_distance')
        next.value = previous.value;
    setOverride(i, j, next);
}
function addOverride(i: number): void {
    mutateOverrides(i, (list) => [...list, defaultOverride('thickness')]);
}
function removeOverride(i: number, j: number): void {
    mutateOverrides(i, (list) => list.filter((_, m) => m !== j));
}
/** Surface-select choices are 1-based display rows over zero-based operand indexes. */
const surfaceCount = computed(() => props.modelValue.surfaces.length);
const OVERRIDE_LIMIT = 256;
const CONFIGURATION_LIMIT = 64;
</script>

<template>
    <section class="panel" aria-labelledby="cfg-title">
        <div class="panel-head">
            <h3 id="cfg-title">{{ tr('cfg.title') }}</h3>
            <button
                type="button"
                class="small"
                :disabled="!online || configurations.length >= CONFIGURATION_LIMIT"
                @click="addConfiguration"
            >
                {{ tr('cfg.add') }}
            </button>
        </div>
        <p class="hint">{{ tr('cfg.hint') }}</p>
        <p v-if="!configurations.length" class="empty" role="status">{{ tr('cfg.empty') }}</p>
        <template v-else>
            <div class="table-scroll" role="region" aria-labelledby="cfg-title" tabindex="0">
                <table>
                    <caption class="sr-only">
                        {{
                            tr('cfg.caption')
                        }}
                    </caption>
                    <thead>
                        <tr>
                            <th scope="col">#</th>
                            <th scope="col">{{ tr('cfg.colActive') }}</th>
                            <th scope="col">{{ tr('cfg.colName') }}</th>
                            <th scope="col">{{ tr('cfg.colOverrides') }}</th>
                            <th scope="col">
                                <span class="sr-only">{{ tr('common.actions') }}</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="(c, i) in configurations" :key="i">
                            <th scope="row">
                                <span class="surface-index">{{ i + 1 }}</span>
                            </th>
                            <td class="center">
                                <input
                                    type="radio"
                                    name="active-configuration"
                                    :aria-label="tr('cfg.activeAria', { n: i + 1 })"
                                    :checked="active === i"
                                    @change="patch({ active_configuration: i })"
                                />
                            </td>
                            <td>
                                <input
                                    type="text"
                                    maxlength="256"
                                    :aria-label="tr('cfg.nameAria', { n: i + 1 })"
                                    :value="c.name"
                                    @focus="selected = i"
                                    @input="rename(i, ($event.target as HTMLInputElement).value)"
                                />
                            </td>
                            <td>
                                <button
                                    type="button"
                                    class="small"
                                    :class="{ primary: selected === i }"
                                    :aria-label="tr('cfg.selectAria', { n: i + 1 })"
                                    @click="selected = i"
                                >
                                    {{ c.overrides.length }}
                                </button>
                            </td>
                            <td>
                                <button
                                    type="button"
                                    class="small"
                                    :aria-label="tr('cfg.removeAria', { n: i + 1 })"
                                    @click="removeConfiguration(i)"
                                >
                                    ×
                                </button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>

            <div class="subpanel">
                <div class="panel-head flush">
                    <h4 id="cfg-ov-title">
                        {{ tr('cfg.overridesTitle', { n: selected + 1 }) }}
                    </h4>
                    <button
                        type="button"
                        class="small"
                        :disabled="
                            !online || configurations[selected]!.overrides.length >= OVERRIDE_LIMIT
                        "
                        @click="addOverride(selected)"
                    >
                        {{ tr('cfg.addOverride') }}
                    </button>
                </div>
                <div class="table-scroll" role="region" aria-labelledby="cfg-ov-title" tabindex="0">
                    <datalist id="cfg-material-options">
                        <option v-for="m in materials" :key="m" :value="m" />
                    </datalist>
                    <table>
                        <thead>
                            <tr>
                                <th scope="col">#</th>
                                <th scope="col">{{ tr('cfg.colKind') }}</th>
                                <th scope="col">{{ tr('cfg.colSurface') }}</th>
                                <th scope="col">{{ tr('cfg.colValue') }}</th>
                                <th scope="col">
                                    <span class="sr-only">{{ tr('common.actions') }}</span>
                                </th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr v-for="(o, j) in configurations[selected]!.overrides" :key="j">
                                <th scope="row">
                                    <span class="surface-index">{{ j + 1 }}</span>
                                </th>
                                <td>
                                    <select
                                        :aria-label="tr('cfg.kindAria', { n: j + 1 })"
                                        :value="o.kind"
                                        @change="
                                            rekind(
                                                selected,
                                                j,
                                                ($event.target as HTMLSelectElement).value as Kind,
                                            )
                                        "
                                    >
                                        <option v-for="k in KINDS" :key="k" :value="k">
                                            {{ tr(`cfg.kinds.${k}`) }}
                                        </option>
                                    </select>
                                </td>
                                <td>
                                    <select
                                        v-if="'surface' in o"
                                        :aria-label="tr('cfg.surfaceAria', { n: j + 1 })"
                                        :value="o.surface"
                                        @change="
                                            setOverride(selected, j, {
                                                ...o,
                                                surface: Number(
                                                    ($event.target as HTMLSelectElement).value,
                                                ),
                                            })
                                        "
                                    >
                                        <option v-for="k in surfaceCount" :key="k" :value="k - 1">
                                            {{ k }}
                                        </option>
                                    </select>
                                    <span v-else class="solve-tag">{{
                                        tr('cfg.sharedObject')
                                    }}</span>
                                </td>
                                <td>
                                    <NumberField
                                        v-if="
                                            o.kind === 'thickness' ||
                                            o.kind === 'radius' ||
                                            o.kind === 'conic'
                                        "
                                        :aria-label="tr('cfg.valueAria', { n: j + 1 })"
                                        :model-value="o.value"
                                        @update:model-value="
                                            (v: number) =>
                                                setOverride(selected, j, { ...o, value: v })
                                        "
                                    />
                                    <NumberField
                                        v-else-if="o.kind === 'aperture'"
                                        min="0"
                                        :aria-label="tr('cfg.valueAria', { n: j + 1 })"
                                        :model-value="o.value"
                                        @update:model-value="
                                            (v: number) =>
                                                setOverride(selected, j, { ...o, value: v })
                                        "
                                    />
                                    <input
                                        v-else-if="o.kind === 'material'"
                                        type="text"
                                        list="cfg-material-options"
                                        autocomplete="off"
                                        spellcheck="false"
                                        :aria-label="tr('cfg.valueAria', { n: j + 1 })"
                                        :value="o.material"
                                        @input="
                                            setOverride(selected, j, {
                                                ...o,
                                                material: ($event.target as HTMLInputElement).value,
                                            })
                                        "
                                    />
                                    <template v-else>
                                        <NumberField
                                            v-if="o.value !== null"
                                            min="0"
                                            :aria-label="tr('cfg.valueAria', { n: j + 1 })"
                                            :model-value="o.value"
                                            @update:model-value="
                                                (v: number) =>
                                                    setOverride(selected, j, {
                                                        ...o,
                                                        value: v,
                                                    })
                                            "
                                        />
                                        <label class="inline"
                                            ><input
                                                type="checkbox"
                                                :aria-label="tr('cfg.infinityAria', { n: j + 1 })"
                                                :checked="o.value === null"
                                                @change="
                                                    setOverride(selected, j, {
                                                        ...o,
                                                        value: ($event.target as HTMLInputElement)
                                                            .checked
                                                            ? null
                                                            : (modelValue.object_distance ?? 1000),
                                                    })
                                                "
                                            />
                                            {{ tr('cfg.infinity') }}</label
                                        >
                                    </template>
                                </td>
                                <td>
                                    <button
                                        type="button"
                                        class="small"
                                        :aria-label="tr('cfg.removeOverrideAria', { n: j + 1 })"
                                        @click="removeOverride(selected, j)"
                                    >
                                        ×
                                    </button>
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </div>
                <small class="hint">{{ tr('cfg.overrideHint') }}</small>
            </div>
        </template>
    </section>
</template>
