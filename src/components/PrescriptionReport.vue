<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { callTool, errorMessage } from '../api';
import { checkRevision } from '../provenance';
import type { OpticalSystem, PrescriptionReport as ReportData, ReportCell, SectionId } from '../types';

const props = defineProps<{ system: OpticalSystem; revision: number; online: boolean; dirty: boolean }>();

const SECTION_DEFS: { id: SectionId; label: string }[] = [
  { id: 'general', label: 'General information' },
  { id: 'specification', label: 'Optical specification' },
  { id: 'first_order', label: 'First-order data' },
  { id: 'lens_data', label: 'Lens data' },
  { id: 'surface_data', label: 'Surface profiles' },
];
const DIAGNOSTIC_CAP = 200;

/** Request inputs frozen when Generate report starts; revision checks use it, never live controls. */
interface Prov {
  revision: number;
  sections: SectionId[];
  wavelength: number;
  precision: number;
}

const selected = ref<Record<SectionId, boolean>>({
  general: true,
  specification: true,
  first_order: true,
  lens_data: true,
  surface_data: true,
});
const wavelengthIndex = ref(props.system.reference_wavelength);
// Raw text: blank or invalid input stays editable and blocks generation; it is parsed only when a request is built.
const precisionText = ref('6');
const parsedPrecision = computed(() => {
  const t = precisionText.value.trim();
  if (t === '') return null;
  const n = Number(t);
  return Number.isInteger(n) && n >= 0 && n <= 15 ? n : null;
});
const report = ref<{ data: ReportData; prov: Prov } | null>(null);
const error = ref('');
const loading = ref(false);
const showText = ref(false);
const showJson = ref(false);
let token = 0;

// Sections go to the engine in canonical order; the reply keeps that order regardless of the click sequence.
const requestSections = computed(() => SECTION_DEFS.filter((d) => selected.value[d.id]).map((d) => d.id));
const sectionLabel = (id: SectionId): string => SECTION_DEFS.find((d) => d.id === id)?.label ?? id;

// Keep the chosen wavelength entry across revisions: by index while it still holds the value, else by value.
let selectedValue = props.system.wavelengths[wavelengthIndex.value]?.value;
watch(wavelengthIndex, (i) => {
  selectedValue = props.system.wavelengths[i]?.value;
});
watch(
  () => props.system,
  (sys) => {
    if (sys.wavelengths[wavelengthIndex.value]?.value === selectedValue) return;
    const kept = sys.wavelengths.findIndex((w) => w.value === selectedValue);
    wavelengthIndex.value = kept >= 0 ? kept : sys.reference_wavelength;
  },
);

/** Inputs that matter to the report; any change drops the displayed result, error and download. */
const inputKey = computed(() => JSON.stringify([requestSections.value, wavelengthIndex.value, precisionText.value]));
/** Drops the displayed result and cancels any in-flight request; never starts a new one. */
function invalidate(): void {
  token++;
  loading.value = false;
  report.value = null;
  error.value = '';
  showText.value = false;
  showJson.value = false;
}
watch([inputKey, () => props.revision, () => props.online], invalidate);

async function generate(): Promise<void> {
  if (!props.online || loading.value) return;
  const precision = parsedPrecision.value;
  if (precision === null) return;
  const mine = ++token;
  const sys = props.system;
  if (wavelengthIndex.value >= sys.wavelengths.length) wavelengthIndex.value = sys.reference_wavelength;
  const prov: Prov = {
    revision: props.revision,
    sections: requestSections.value,
    wavelength: sys.wavelengths[wavelengthIndex.value]!.value,
    precision,
  };
  report.value = null;
  error.value = '';
  loading.value = true;
  /** Superseded replies are dropped quietly; a reply from a newer saved revision is reported. */
  const accept = (replied: number): boolean => {
    const check = checkRevision(prov.revision, props.revision, replied);
    if (check === 'newer') {
      error.value = `Prescription report: the saved system is now at revision ${replied} while this view shows revision ${prov.revision}. The result was discarded. Use Generate report again after the saved revision loads.`;
    }
    return check === 'ok';
  };
  try {
    const data = await callTool('prescription_report', {
      sections: [...prov.sections],
      wavelength: prov.wavelength,
      precision: prov.precision,
    });
    if (mine === token && accept(data.revision)) report.value = { data, prov };
  } catch (e) {
    if (mine === token) {
      report.value = null;
      error.value = errorMessage(e);
    }
  } finally {
    if (mine === token) loading.value = false;
  }
}

function download(): void {
  const r = report.value;
  if (!r) return;
  const base = r.data.name.replace(/[\\/:*?"<>|\s]+/g, '_').replace(/^_+|_+$/g, '') || 'system';
  const url = URL.createObjectURL(new Blob([r.data.text], { type: 'text/plain' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = `${base}-revision-${r.data.revision}-prescription.txt`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

// ---- rendering ----------------------------------------------------------------------------------
const endpointRow = (id: string): boolean => id === 'object' || id === 'image';
function cellClass(cell: ReportCell): string {
  if (cell.kind === 'number') return 'num';
  if (cell.kind === 'infinite') return 'infinite';
  if (cell.kind === 'not_applicable') return 'na';
  if (cell.kind === 'unavailable') return 'unavailable';
  return '';
}
function reason(cell: ReportCell): string | undefined {
  return cell.kind === 'not_applicable' || cell.kind === 'unavailable' ? cell.reason : undefined;
}
const diagnostics = computed(() => report.value?.data.diagnostics ?? []);
const shownDiagnostics = computed(() => diagnostics.value.slice(0, DIAGNOSTIC_CAP));
const rawJson = computed(() => (showJson.value && report.value ? JSON.stringify(report.value.data, null, 1) : ''));
</script>

<template>
  <section class="panel" aria-labelledby="pr-title">
    <div class="panel-head">
      <h3 id="pr-title">Prescription report</h3>
      <span>Assembled from the saved system · revision {{ report ? report.data.revision : revision }}</span>
    </div>
    <div v-if="dirty" role="status" class="banner warn inset">
      Results use the saved system (revision {{ revision }}). Unapplied draft changes are not included until you apply them.
    </div>
    <form class="analysis-controls" @submit.prevent="generate">
      <fieldset class="report-sections">
        <legend class="sr-only">Report sections</legend>
        <label v-for="d in SECTION_DEFS" :key="d.id" class="inline">
          <input v-model="selected[d.id]" type="checkbox" /> {{ d.label }}
        </label>
      </fieldset>
      <div class="controls">
        <label>Report wavelength
          <select v-model.number="wavelengthIndex">
            <option v-for="(w, i) in system.wavelengths" :key="i" :value="i">{{ i + 1 }}: {{ w.value }} nm</option>
          </select>
        </label>
        <label>Display decimals
          <input
            type="number"
            min="0"
            max="15"
            step="1"
            required
            :value="precisionText"
            :aria-invalid="parsedPrecision === null"
            @input="precisionText = ($event.target as HTMLInputElement).value"
          />
        </label>
        <button type="submit" class="primary" :disabled="!online || loading || parsedPrecision === null">Generate report</button>
        <button type="button" :disabled="!report" @click="download">Download report</button>
      </div>
      <p v-if="parsedPrecision === null" class="note warn-text" role="status">
        Display decimals must be a whole number from 0 to 15. Generation stays disabled until the input is valid.
      </p>
      <p class="note">
        Cell text and the download are the backend's canonical formatting at the chosen precision; the report is read-only.
        With no sections selected the report holds the header only. Any control, revision or connection change clears the result and the download.
      </p>
    </form>
    <div v-if="error" role="alert" class="banner error inset">{{ error }}</div>
    <p v-if="loading" class="empty" role="status">Generating…</p>
    <p v-else-if="!report && !error" class="empty">{{ online ? 'Generate report to assemble the prescription.' : 'Engine offline.' }}</p>

    <div v-if="report" class="result" :aria-busy="loading">
      <p class="note" role="status">
        “{{ report.data.name }}” · revision {{ report.data.revision }} · λ {{ report.data.wavelength }} nm · display decimals {{ report.data.precision }}
        · {{ report.data.sections.length ? report.data.sections.map((s) => s.title).join(', ') : 'header only, no sections' }}
      </p>
      <details v-for="s in report.data.sections" :key="s.id" open class="report-section">
        <summary>{{ s.title }}</summary>
        <p v-for="(p, pi) in s.paragraphs" :key="pi" class="report-para">{{ p }}</p>
        <div v-for="t in s.tables" :key="t.id" class="table-scroll">
          <table class="report-table">
            <caption>{{ t.title }}</caption>
            <thead>
              <tr>
                <th v-for="c in t.columns" :key="c.id" scope="col">{{ c.label }}<template v-if="c.unit"> [{{ c.unit }}]</template></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="r in t.rows" :key="r.id" :class="{ endpoint: endpointRow(r.id) }">
                <th scope="row">{{ r.cells[0]?.text }}</th>
                <td
                  v-for="(cell, ci) in r.cells.slice(1)"
                  :key="ci"
                  :class="cellClass(cell)"
                  :tabindex="reason(cell) !== undefined ? 0 : undefined"
                  :title="reason(cell)"
                >
                  {{ cell.text }}<span v-if="reason(cell) !== undefined" class="sr-only">: {{ reason(cell) }}</span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </details>

      <div v-if="diagnostics.length" class="subpanel">
        <h4>Cell diagnostics ({{ diagnostics.length }})</h4>
        <p v-if="diagnostics.length > DIAGNOSTIC_CAP" class="note" role="status">
          Showing the first {{ DIAGNOSTIC_CAP }} of {{ diagnostics.length }}.
        </p>
        <div class="table-scroll">
          <table class="report-table">
            <caption>Unavailable cells and their reasons</caption>
            <thead>
              <tr>
                <th scope="col">Section</th>
                <th scope="col">Table</th>
                <th scope="col">Row</th>
                <th scope="col">Column</th>
                <th scope="col">Message</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(d, i) in shownDiagnostics" :key="i">
                <th scope="row">{{ sectionLabel(d.section) }}</th>
                <td>{{ d.table }}</td>
                <td>{{ d.row }}</td>
                <td>{{ d.column }}</td>
                <td>{{ d.message }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <details class="data-table wide" @toggle="showText = ($event.target as HTMLDetailsElement).open">
        <summary>Canonical report text</summary>
        <pre v-if="showText" class="raw">{{ report.data.text }}</pre>
      </details>
      <details class="data-table wide" @toggle="showJson = ($event.target as HTMLDetailsElement).open">
        <summary>Raw response JSON</summary>
        <pre v-if="showJson" class="raw">{{ rawJson }}</pre>
      </details>
    </div>
  </section>
</template>
