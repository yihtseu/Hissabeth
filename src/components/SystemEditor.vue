<script setup lang="ts">
import { tr } from '../i18n';
import { computed, watch } from 'vue';
import EndpointEditor from './EndpointEditor.vue';
import NumberField from './NumberField.vue';
import { rgbToHex, withHex } from '../color';
import type {
    EndpointSurface,
    Field,
    FieldType,
    OpticalSystem,
    PupilKind,
    PupilMapping,
    Vignetting,
    Wavelength,
} from '../types';
import {
    FIELD_TYPES,
    PUPIL_KINDS,
    fieldUnit,
    glassKey,
    infiniteObjectProblem,
    newField,
    newWavelength,
} from '../validation';

const props = defineProps<{ modelValue: OpticalSystem }>();
const emit = defineEmits<{ 'update:modelValue': [value: OpticalSystem] }>();

const FIELD_TYPE_LABELS: Record<FieldType, string> = {
    get angle() {
        return tr('sys.fieldTypes.angle');
    },
    get object_height() {
        return tr('sys.fieldTypes.object_height');
    },
    get image_height() {
        return tr('sys.fieldTypes.image_height');
    },
};
const PUPIL_LABELS: Record<PupilKind, { name: string; unit: string }> = {
    entrance_pupil_diameter: {
        get name() {
            return tr('sys.pupilKinds.entrance_pupil_diameter');
        },
        unit: 'mm',
    },
    f_number: { name: 'F-number', unit: '' },
    object_na: {
        get name() {
            return tr('sys.pupilKinds.object_na');
        },
        unit: '',
    },
    image_na: {
        get name() {
            return tr('sys.pupilKinds.image_na');
        },
        unit: '',
    },
};
const VIGNETTING = computed(() => [
    tr('aperture.dirUpperY'),
    tr('aperture.dirLowerY'),
    tr('aperture.dirUpperX'),
    tr('aperture.dirLowerX'),
]);

const s = computed(() => props.modelValue);
const unit = computed(() => fieldUnit(s.value.field_type));

function patch(p: Partial<OpticalSystem>): void {
    emit('update:modelValue', { ...props.modelValue, ...p });
}
const input = (e: Event): string => (e.target as HTMLInputElement).value;

// ---- wavelengths -----------------------------------------------------------------
function setWavelength(i: number, p: Partial<Wavelength>): void {
    patch({ wavelengths: s.value.wavelengths.map((w, k) => (k === i ? { ...w, ...p } : w)) });
}
function addWavelength(): void {
    patch({ wavelengths: [...s.value.wavelengths, newWavelength(s.value.wavelengths.length)] });
}
function removeWavelength(i: number): void {
    const current = s.value.reference_wavelength;
    const remaining = s.value.wavelengths.length - 1;
    patch({
        wavelengths: s.value.wavelengths.filter((_, k) => k !== i),
        reference_wavelength: Math.max(
            0,
            Math.min(i < current ? current - 1 : current, remaining - 1),
        ),
    });
}

/** RGB from a color input; the stored alpha is kept exactly and an unchanged color is not rewritten. */
function setWavelengthColor(i: number, hex: string): void {
    const next = withHex(s.value.wavelengths[i]!.color, hex);
    if (next) setWavelength(i, { color: next });
}
function setFieldColor(i: number, hex: string): void {
    const next = withHex(s.value.fields[i]!.color, hex);
    if (next) setField(i, { color: next });
}

// ---- fields ---------------------------------------------------------------------------
function setField(i: number, p: Partial<Field>): void {
    patch({ fields: s.value.fields.map((f, k) => (k === i ? { ...f, ...p } : f)) });
}
function setVignetting(i: number, j: number, v: number): void {
    const next = [...s.value.fields[i]!.vignetting] as Vignetting;
    next[j] = v;
    setField(i, { vignetting: next });
}
function addField(): void {
    patch({ fields: [...s.value.fields, newField(s.value.fields.length)] });
}
function removeField(i: number): void {
    patch({ fields: s.value.fields.filter((_, k) => k !== i) });
}

// ---- custom glasses (read-only fields; removal keeps every other glass intact) -------------
function removeGlass(i: number): void {
    patch({ custom_glasses: s.value.custom_glasses.filter((_, k) => k !== i) });
}

/** Last finite distance, restored when infinity is switched off. Geometry is never reset by the toggle. */
let lastFinite = s.value.object_distance ?? 1000;
watch(
    () => s.value.object_distance,
    (d) => {
        if (d !== null) lastFinite = d;
    },
);

function setInfinity(infinite: boolean): void {
    patch({ object_distance: infinite ? null : lastFinite });
}

const infiniteProblem = computed(() => infiniteObjectProblem(s.value));
const objectNote = computed(() =>
    s.value.object_distance === null
        ? tr('sys.objectNoteInfinite')
        : tr('sys.objectNoteFinite', { d: s.value.object_distance }),
);
</script>

<template>
    <section class="panel" aria-labelledby="sys-title">
        <div class="panel-head">
            <h3 id="sys-title">{{ tr('sys.title') }}</h3>
        </div>
        <div class="form-grid">
            <div class="field">
                <label for="sys-name">{{ tr('sys.name') }}</label>
                <input
                    id="sys-name"
                    type="text"
                    maxlength="256"
                    :value="s.name"
                    @input="patch({ name: input($event) })"
                />
            </div>
            <div class="field">
                <label for="sys-ftype">{{ tr('sys.fieldType') }}</label>
                <select
                    id="sys-ftype"
                    :value="s.field_type"
                    @change="patch({ field_type: input($event) as FieldType })"
                >
                    <option v-for="t in FIELD_TYPES" :key="t" :value="t">
                        {{ FIELD_TYPE_LABELS[t] }}
                    </option>
                </select>
            </div>
            <div class="field">
                <label for="sys-pkind">{{ tr('sys.pupilSpec') }}</label>
                <select
                    id="sys-pkind"
                    :value="s.pupil.kind"
                    @change="patch({ pupil: { ...s.pupil, kind: input($event) as PupilKind } })"
                >
                    <option v-for="k in PUPIL_KINDS" :key="k" :value="k">
                        {{ PUPIL_LABELS[k].name }}
                    </option>
                </select>
            </div>
            <div class="field">
                <label for="sys-pvalue"
                    >{{ PUPIL_LABELS[s.pupil.kind].name }}
                    {{
                        PUPIL_LABELS[s.pupil.kind].unit && `(${PUPIL_LABELS[s.pupil.kind].unit})`
                    }}</label
                >
                <NumberField
                    id="sys-pvalue"
                    min="0"
                    :model-value="s.pupil.value"
                    @update:model-value="(v: number) => patch({ pupil: { ...s.pupil, value: v } })"
                />
                <small
                    >{{ tr('sys.pupilPositive')
                    }}{{ s.pupil.kind === 'object_na' ? tr('sys.pupilObjectNa') : '' }}.</small
                >
            </div>
            <div class="field">
                <label for="sys-pmap">{{ tr('sys.pupilMapping') }}</label>
                <select
                    id="sys-pmap"
                    :value="s.pupil_mapping"
                    aria-describedby="sys-pmap-help"
                    @change="patch({ pupil_mapping: input($event) as PupilMapping })"
                >
                    <option value="reference_entrance_pupil">{{ tr('sys.mappingRef') }}</option>
                    <option value="real_stop">{{ tr('sys.mappingStop') }}</option>
                </select>
                <small id="sys-pmap-help"
                    >{{
                        s.pupil_mapping === 'real_stop'
                            ? tr('sys.mappingHelpStop')
                            : tr('sys.mappingHelpRef')
                    }}
                    {{ tr('sys.mappingHelpTail') }}</small
                >
            </div>
            <div class="field">
                <label for="sys-obj">{{ tr('sys.objectDistance') }}</label>
                <NumberField
                    v-if="s.object_distance !== null"
                    id="sys-obj"
                    min="0"
                    :model-value="s.object_distance"
                    @update:model-value="(v: number) => patch({ object_distance: v })"
                />
                <input
                    v-else
                    id="sys-obj"
                    type="text"
                    value="∞"
                    disabled
                    :aria-label="tr('sys.objectInfinity')"
                />
                <label class="inline"
                    ><input
                        type="checkbox"
                        :checked="s.object_distance === null"
                        @change="setInfinity(($event.target as HTMLInputElement).checked)"
                    />
                    {{ tr('sys.atInfinity') }}</label
                >
                <small
                    v-if="s.field_type !== 'angle' && s.object_distance === null"
                    class="warn-text"
                    >{{ tr('sys.heightNeedsFinite') }}</small
                >
            </div>
            <div class="field">
                <label for="sys-omat">{{ tr('sys.objectMaterial') }}</label>
                <input
                    id="sys-omat"
                    type="text"
                    list="material-options"
                    autocomplete="off"
                    spellcheck="false"
                    :value="s.object_material"
                    @input="patch({ object_material: input($event) })"
                />
                <small>{{ tr('sys.objectMaterialHelp') }}</small>
            </div>
            <div class="field">
                <label for="sys-olabel">{{ tr('sys.objectLabel') }}</label>
                <input
                    id="sys-olabel"
                    type="text"
                    maxlength="256"
                    :value="s.object_label"
                    @input="patch({ object_label: input($event) })"
                />
            </div>
            <div class="field">
                <label for="sys-ilabel">{{ tr('sys.imageLabel') }}</label>
                <input
                    id="sys-ilabel"
                    type="text"
                    maxlength="256"
                    :value="s.image_label"
                    @input="patch({ image_label: input($event) })"
                />
            </div>
            <div class="field">
                <label for="sys-temp">{{ tr('sys.temperature') }}</label>
                <NumberField
                    id="sys-temp"
                    :model-value="s.temperature"
                    @update:model-value="(v: number) => patch({ temperature: v })"
                />
            </div>
            <div class="field">
                <label for="sys-press">{{ tr('sys.pressure') }}</label>
                <NumberField
                    id="sys-press"
                    min="0"
                    :model-value="s.pressure"
                    @update:model-value="(v: number) => patch({ pressure: v })"
                />
            </div>
            <div class="field wide-field">
                <label for="sys-note">{{ tr('sys.note') }}</label>
                <textarea
                    id="sys-note"
                    rows="2"
                    maxlength="100000"
                    :value="s.note"
                    @input="patch({ note: input($event) })"
                />
            </div>
        </div>

        <EndpointEditor
            name="Object"
            :model-value="s.object_surface"
            :note="objectNote"
            :problem="infiniteProblem"
            @update:model-value="(v: EndpointSurface) => patch({ object_surface: v })"
        />
        <EndpointEditor
            name="Image"
            :model-value="s.image_surface"
            :note="tr('sys.imageNote')"
            @update:model-value="(v: EndpointSurface) => patch({ image_surface: v })"
        />

        <div class="subpanel">
            <div class="panel-head flush">
                <h4 id="wl-title">{{ tr('sys.wavelengths') }}</h4>
                <button
                    type="button"
                    class="small"
                    :disabled="s.wavelengths.length >= 32"
                    @click="addWavelength"
                >
                    {{ tr('sys.addWavelength') }}
                </button>
            </div>
            <div class="table-scroll" role="region" aria-labelledby="wl-title" tabindex="0">
                <table>
                    <caption class="sr-only">
                        {{
                            tr('sys.wlCaption')
                        }}
                    </caption>
                    <thead>
                        <tr>
                            <th scope="col">#</th>
                            <th scope="col">{{ tr('sys.colWl') }}</th>
                            <th scope="col">{{ tr('sys.colWeight') }}</th>
                            <th scope="col">{{ tr('sys.colColor') }}</th>
                            <th scope="col">{{ tr('sys.colReference') }}</th>
                            <th scope="col">
                                <span class="sr-only">{{ tr('common.actions') }}</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="(w, i) in s.wavelengths" :key="i">
                            <th scope="row">
                                <span class="surface-index">{{ i + 1 }}</span>
                            </th>
                            <td>
                                <NumberField
                                    :aria-label="tr('sys.wlAria', { n: i + 1 })"
                                    :model-value="w.value"
                                    @update:model-value="
                                        (v: number) => setWavelength(i, { value: v })
                                    "
                                />
                            </td>
                            <td>
                                <NumberField
                                    min="0"
                                    :aria-label="tr('sys.wlWeightAria', { n: i + 1 })"
                                    :model-value="w.weight"
                                    @update:model-value="
                                        (v: number) => setWavelength(i, { weight: v })
                                    "
                                />
                            </td>
                            <td>
                                <input
                                    type="color"
                                    :aria-label="tr('sys.wlColorAria', { n: i + 1 })"
                                    :value="rgbToHex(w.color)"
                                    @input="setWavelengthColor(i, input($event))"
                                />
                            </td>
                            <td>
                                <input
                                    type="radio"
                                    name="ref-wl"
                                    :aria-label="tr('sys.wlRefAria', { n: i + 1 })"
                                    :checked="s.reference_wavelength === i"
                                    @change="patch({ reference_wavelength: i })"
                                />
                            </td>
                            <td>
                                <button
                                    type="button"
                                    class="small"
                                    :disabled="s.wavelengths.length === 1"
                                    :aria-label="tr('sys.wlRemoveAria', { n: i + 1 })"
                                    @click="removeWavelength(i)"
                                >
                                    ×
                                </button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
            <small class="hint">{{ tr('sys.wlHint') }}</small>
        </div>

        <div class="subpanel">
            <div class="panel-head flush">
                <h4 id="fl-title">{{ tr('sys.fields') }}</h4>
                <button
                    type="button"
                    class="small"
                    :disabled="s.fields.length >= 64"
                    @click="addField"
                >
                    {{ tr('sys.addField') }}
                </button>
            </div>
            <div class="table-scroll" role="region" aria-labelledby="fl-title" tabindex="0">
                <table>
                    <caption class="sr-only">
                        {{
                            tr('sys.fieldsCaption')
                        }}
                    </caption>
                    <thead>
                        <tr>
                            <th scope="col">#</th>
                            <th scope="col">X ({{ unit }})</th>
                            <th scope="col">Y ({{ unit }})</th>
                            <th scope="col">{{ tr('sys.colWeight') }}</th>
                            <th v-for="v in VIGNETTING" :key="v" scope="col">
                                {{ tr('fragments.vig') }} {{ v }}
                            </th>
                            <th scope="col">{{ tr('sys.colColor') }}</th>
                            <th scope="col">
                                <span class="sr-only">{{ tr('common.actions') }}</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="(f, i) in s.fields" :key="i">
                            <th scope="row">
                                <span class="surface-index">{{ i + 1 }}</span>
                            </th>
                            <td>
                                <NumberField
                                    :aria-label="tr('sys.fieldXAria', { n: i + 1 })"
                                    :model-value="f.x"
                                    @update:model-value="(v: number) => setField(i, { x: v })"
                                />
                            </td>
                            <td>
                                <NumberField
                                    :aria-label="tr('sys.fieldYAria', { n: i + 1 })"
                                    :model-value="f.y"
                                    @update:model-value="(v: number) => setField(i, { y: v })"
                                />
                            </td>
                            <td>
                                <NumberField
                                    min="0"
                                    :aria-label="tr('sys.fieldWeightAria', { n: i + 1 })"
                                    :model-value="f.weight"
                                    @update:model-value="(v: number) => setField(i, { weight: v })"
                                />
                            </td>
                            <td v-for="(label, j) in VIGNETTING" :key="j">
                                <NumberField
                                    :aria-label="
                                        tr('sys.fieldVigAria', {
                                            n: i + 1,
                                            dir: label.toLowerCase(),
                                        })
                                    "
                                    :model-value="f.vignetting[j]!"
                                    @update:model-value="(v: number) => setVignetting(i, j, v)"
                                />
                            </td>
                            <td>
                                <input
                                    type="color"
                                    :aria-label="tr('sys.fieldColorAria', { n: i + 1 })"
                                    :value="rgbToHex(f.color)"
                                    @input="setFieldColor(i, input($event))"
                                />
                            </td>
                            <td>
                                <button
                                    type="button"
                                    class="small"
                                    :disabled="s.fields.length === 1"
                                    :aria-label="tr('sys.fieldRemoveAria', { n: i + 1 })"
                                    @click="removeField(i)"
                                >
                                    ×
                                </button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
            <small class="hint">{{ tr('sys.fieldsHint') }}</small>
        </div>

        <div v-if="s.custom_glasses.length" class="subpanel">
            <h4 id="cg-title">{{ tr('fragments.customGlasses') }}{{ s.custom_glasses.length }})</h4>
            <div class="table-scroll cg" role="region" aria-labelledby="cg-title" tabindex="0">
                <table>
                    <caption class="sr-only">
                        {{
                            tr('sys.cgCaption')
                        }}
                    </caption>
                    <thead>
                        <tr>
                            <th scope="col">{{ tr('sys.colKey') }}</th>
                            <th scope="col">nd</th>
                            <th scope="col">Vd</th>
                            <th scope="col">{{ tr('sys.colFormula') }}</th>
                            <th scope="col">{{ tr('sys.colComment') }}</th>
                            <th scope="col">
                                <span class="sr-only">{{ tr('common.actions') }}</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr v-for="(g, i) in s.custom_glasses" :key="glassKey(g)">
                            <th scope="row">{{ glassKey(g) }}</th>
                            <td>{{ g.nd }}</td>
                            <td>{{ g.vd }}</td>
                            <td>{{ g.formula }}</td>
                            <td>{{ g.comment }}</td>
                            <td>
                                <button
                                    type="button"
                                    class="small"
                                    :aria-label="tr('sys.cgRemoveAria', { key: glassKey(g) })"
                                    @click="removeGlass(i)"
                                >
                                    ×
                                </button>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>
    </section>
</template>
