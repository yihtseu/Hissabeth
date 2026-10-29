<script setup lang="ts">
import { computed } from 'vue';
import NumberField from './NumberField.vue';
import { rgbToHex, withHex } from '../color';
import type { Field, FieldType, OpticalSystem, PupilKind, PupilMapping, Vignetting, Wavelength } from '../types';
import { FIELD_TYPES, PUPIL_KINDS, fieldUnit, glassKey, newField, newWavelength } from '../validation';

const props = defineProps<{ modelValue: OpticalSystem }>();
const emit = defineEmits<{ 'update:modelValue': [value: OpticalSystem] }>();

const FIELD_TYPE_LABELS: Record<FieldType, string> = {
  angle: 'Angle (°)',
  object_height: 'Object height (mm)',
  image_height: 'Image height (mm)',
};
const PUPIL_LABELS: Record<PupilKind, { name: string; unit: string }> = {
  entrance_pupil_diameter: { name: 'Entrance pupil diameter', unit: 'mm' },
  f_number: { name: 'F-number', unit: '' },
  object_na: { name: 'Object NA', unit: '' },
  image_na: { name: 'Image NA', unit: '' },
};
const VIGNETTING = ['Upper y', 'Lower y', 'Upper x', 'Lower x'];

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
    reference_wavelength: Math.max(0, Math.min(i < current ? current - 1 : current, remaining - 1)),
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

function setInfinity(infinite: boolean): void {
  patch({ object_distance: infinite ? null : 1000 });
}
</script>

<template>
  <section class="panel" aria-labelledby="sys-title">
    <div class="panel-head"><h3 id="sys-title">System</h3></div>
    <div class="form-grid">
      <div class="field">
        <label for="sys-name">Name</label>
        <input id="sys-name" type="text" maxlength="256" :value="s.name" @input="patch({ name: input($event) })" />
      </div>
      <div class="field">
        <label for="sys-ftype">Field type</label>
        <select id="sys-ftype" :value="s.field_type" @change="patch({ field_type: input($event) as FieldType })">
          <option v-for="t in FIELD_TYPES" :key="t" :value="t">{{ FIELD_TYPE_LABELS[t] }}</option>
        </select>
      </div>
      <div class="field">
        <label for="sys-pkind">Pupil specification</label>
        <select id="sys-pkind" :value="s.pupil.kind" @change="patch({ pupil: { ...s.pupil, kind: input($event) as PupilKind } })">
          <option v-for="k in PUPIL_KINDS" :key="k" :value="k">{{ PUPIL_LABELS[k].name }}</option>
        </select>
      </div>
      <div class="field">
        <label for="sys-pvalue">{{ PUPIL_LABELS[s.pupil.kind].name }} {{ PUPIL_LABELS[s.pupil.kind].unit && `(${PUPIL_LABELS[s.pupil.kind].unit})` }}</label>
        <NumberField id="sys-pvalue" min="0" :model-value="s.pupil.value" @update:model-value="(v: number) => patch({ pupil: { ...s.pupil, value: v } })" />
        <small>Must be positive{{ s.pupil.kind === 'object_na' ? ' and below the refractive index of the object medium' : '' }}.</small>
      </div>
      <div class="field">
        <label for="sys-pmap">Pupil ray mapping</label>
        <select id="sys-pmap" :value="s.pupil_mapping" aria-describedby="sys-pmap-help" @change="patch({ pupil_mapping: input($event) as PupilMapping })">
          <option value="reference_entrance_pupil">Reference entrance pupil</option>
          <option value="real_stop">Real stop aiming</option>
        </select>
        <small id="sys-pmap-help">{{ s.pupil_mapping === 'real_stop'
          ? 'Aims each ray at its prescribed stop coordinate.'
          : 'Aims the reference chief ray, then traces fixed entrance-plane offsets across wavelengths.' }} First-order pupil data is unchanged.</small>
      </div>
      <div class="field">
        <label for="sys-obj">Object distance (mm)</label>
        <NumberField v-if="s.object_distance !== null" id="sys-obj" min="0" :model-value="s.object_distance" @update:model-value="(v: number) => patch({ object_distance: v })" />
        <input v-else id="sys-obj" type="text" value="∞" disabled aria-label="Object distance: infinity" />
        <label class="inline"><input type="checkbox" :checked="s.object_distance === null" @change="setInfinity(($event.target as HTMLInputElement).checked)" /> Object at infinity</label>
        <small v-if="s.field_type !== 'angle' && s.object_distance === null" class="warn-text">Height fields need a finite distance.</small>
      </div>
      <div class="field">
        <label for="sys-omat">Object material</label>
        <input id="sys-omat" type="text" list="material-options" autocomplete="off" spellcheck="false" :value="s.object_material" @input="patch({ object_material: input($event) })" />
        <small>Medium between the object and the first surface (finite or infinite object).</small>
      </div>
      <div class="field">
        <label for="sys-olabel">Object label</label>
        <input id="sys-olabel" type="text" maxlength="256" :value="s.object_label" @input="patch({ object_label: input($event) })" />
      </div>
      <div class="field">
        <label for="sys-ilabel">Image label</label>
        <input id="sys-ilabel" type="text" maxlength="256" :value="s.image_label" @input="patch({ image_label: input($event) })" />
      </div>
      <div class="field">
        <label for="sys-temp">Temperature (°C)</label>
        <NumberField id="sys-temp" :model-value="s.temperature" @update:model-value="(v: number) => patch({ temperature: v })" />
      </div>
      <div class="field">
        <label for="sys-press">Pressure (Pa)</label>
        <NumberField id="sys-press" min="0" :model-value="s.pressure" @update:model-value="(v: number) => patch({ pressure: v })" />
      </div>
      <div class="field wide-field">
        <label for="sys-note">Note</label>
        <textarea id="sys-note" rows="2" maxlength="100000" :value="s.note" @input="patch({ note: input($event) })" />
      </div>
    </div>

    <div class="subpanel">
      <div class="panel-head flush">
        <h4 id="wl-title">Wavelengths</h4>
        <button type="button" class="small" :disabled="s.wavelengths.length >= 32" @click="addWavelength">+ Wavelength</button>
      </div>
      <div class="table-scroll" role="region" aria-labelledby="wl-title" tabindex="0">
        <table>
          <caption class="sr-only">Wavelengths in nm with weights and plot colors; one is the reference wavelength.</caption>
          <thead><tr><th scope="col">#</th><th scope="col">Wavelength (nm)</th><th scope="col">Weight</th><th scope="col">Color</th><th scope="col">Reference</th><th scope="col"><span class="sr-only">Actions</span></th></tr></thead>
          <tbody>
            <tr v-for="(w, i) in s.wavelengths" :key="i">
              <th scope="row"><span class="surface-index">{{ i + 1 }}</span></th>
              <td><NumberField :aria-label="`Wavelength ${i + 1} (nm)`" :model-value="w.value" @update:model-value="(v: number) => setWavelength(i, { value: v })" /></td>
              <td><NumberField min="0" :aria-label="`Wavelength ${i + 1} weight`" :model-value="w.weight" @update:model-value="(v: number) => setWavelength(i, { weight: v })" /></td>
              <td><input type="color" :aria-label="`Wavelength ${i + 1} color`" :value="rgbToHex(w.color)" @input="setWavelengthColor(i, input($event))" /></td>
              <td><input type="radio" name="ref-wl" :aria-label="`Use wavelength ${i + 1} as reference`" :checked="s.reference_wavelength === i" @change="patch({ reference_wavelength: i })" /></td>
              <td><button type="button" class="small" :disabled="s.wavelengths.length === 1" :aria-label="`Remove wavelength ${i + 1}`" @click="removeWavelength(i)">×</button></td>
            </tr>
          </tbody>
        </table>
      </div>
      <small class="hint">1–32 wavelengths, 100–20000 nm, at least one weight positive.</small>
    </div>

    <div class="subpanel">
      <div class="panel-head flush">
        <h4 id="fl-title">Fields</h4>
        <button type="button" class="small" :disabled="s.fields.length >= 64" @click="addField">+ Field</button>
      </div>
      <div class="table-scroll" role="region" aria-labelledby="fl-title" tabindex="0">
        <table>
          <caption class="sr-only">Fields with weights, vignetting factors and plot colors. Fields are referred to by their number.</caption>
          <thead><tr>
            <th scope="col">#</th><th scope="col">X ({{ unit }})</th><th scope="col">Y ({{ unit }})</th><th scope="col">Weight</th>
            <th v-for="v in VIGNETTING" :key="v" scope="col">Vig. {{ v }}</th><th scope="col">Color</th><th scope="col"><span class="sr-only">Actions</span></th>
          </tr></thead>
          <tbody>
            <tr v-for="(f, i) in s.fields" :key="i">
              <th scope="row"><span class="surface-index">{{ i + 1 }}</span></th>
              <td><NumberField :aria-label="`Field ${i + 1} x`" :model-value="f.x" @update:model-value="(v: number) => setField(i, { x: v })" /></td>
              <td><NumberField :aria-label="`Field ${i + 1} y`" :model-value="f.y" @update:model-value="(v: number) => setField(i, { y: v })" /></td>
              <td><NumberField min="0" :aria-label="`Field ${i + 1} weight`" :model-value="f.weight" @update:model-value="(v: number) => setField(i, { weight: v })" /></td>
              <td v-for="(label, j) in VIGNETTING" :key="j"><NumberField :aria-label="`Field ${i + 1} vignetting ${label.toLowerCase()}`" :model-value="f.vignetting[j]!" @update:model-value="(v: number) => setVignetting(i, j, v)" /></td>
              <td><input type="color" :aria-label="`Field ${i + 1} color`" :value="rgbToHex(f.color)" @input="setFieldColor(i, input($event))" /></td>
              <td><button type="button" class="small" :disabled="s.fields.length === 1" :aria-label="`Remove field ${i + 1}`" @click="removeField(i)">×</button></td>
            </tr>
          </tbody>
        </table>
      </div>
      <small class="hint">1–64 fields. Angles within ±80°. Vignetting factors &lt; 1: positive trims the boundary, negative enlarges it (no negative floor), 0 = unvignetted. Auto and Clear vignetting are under Aperture &amp; vignetting.</small>
    </div>

    <div v-if="s.custom_glasses.length" class="subpanel">
      <h4 id="cg-title">Custom glasses ({{ s.custom_glasses.length }})</h4>
      <div class="table-scroll cg" role="region" aria-labelledby="cg-title" tabindex="0">
        <table>
          <caption class="sr-only">Custom glasses stored with the system. Dispersion data is preserved when editing.</caption>
          <thead><tr><th scope="col">Key</th><th scope="col">nd</th><th scope="col">Vd</th><th scope="col">Formula</th><th scope="col">Comment</th><th scope="col"><span class="sr-only">Actions</span></th></tr></thead>
          <tbody>
            <tr v-for="(g, i) in s.custom_glasses" :key="glassKey(g)">
              <th scope="row">{{ glassKey(g) }}</th><td>{{ g.nd }}</td><td>{{ g.vd }}</td><td>{{ g.formula }}</td><td>{{ g.comment }}</td>
              <td><button type="button" class="small" :aria-label="`Remove custom glass ${glassKey(g)}`" @click="removeGlass(i)">×</button></td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>
</template>
