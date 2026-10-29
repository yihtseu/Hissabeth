<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue';
import { configureAgents, detectAgents, errorMessage } from './api';
import { useWorkbench } from './composables/useWorkbench';
import AgentPanel from './components/AgentPanel.vue';
import AnalysisPanel from './components/AnalysisPanel.vue';
import ApertureTools from './components/ApertureTools.vue';
import GapSolveEditor from './components/GapSolveEditor.vue';
import ImportExportPanel from './components/ImportExportPanel.vue';
import JsonEditor from './components/JsonEditor.vue';
import MaterialsPanel from './components/MaterialsPanel.vue';
import LayoutView from './components/LayoutView.vue';
import MetricsBar from './components/MetricsBar.vue';
import MtfView from './components/MtfView.vue';
import RayInspection from './components/RayInspection.vue';
import SpotView from './components/SpotView.vue';
import SurfaceTable from './components/SurfaceTable.vue';
import SystemEditor from './components/SystemEditor.vue';
import { TABS, type AgentInfo, type AgentRegistration, type Tab } from './types';

const tab = ref<Tab>('Layout');
const wantMtf = computed(() => tab.value === 'MTF');
const wb = useWorkbench(wantMtf);

const agents = ref<AgentInfo[]>([]);
const registration = ref<AgentRegistration[]>([]);
const agentError = ref('');

async function refreshAgents(): Promise<void> {
  try {
    agents.value = await detectAgents();
  } catch (e) {
    agentError.value = errorMessage(e);
  }
}
async function connect(): Promise<void> {
  try {
    registration.value = await configureAgents();
    agentError.value = '';
  } catch (e) {
    agentError.value = errorMessage(e);
  }
  await refreshAgents();
}
onMounted(() => {
  void refreshAgents();
});

const showTable = computed(() => ['Layout', 'Spot diagram', 'MTF', 'Prescription'].includes(tab.value));
const showMetrics = computed(() => showTable.value || tab.value === 'Analysis');
const conjugate = computed(() => {
  const d = wb.system.value?.object_distance;
  return d === null || d === undefined ? 'Infinity conjugate' : `Object at ${d} mm`;
});
/** Autofocus moves the image distance, which an active solve on the final gap already controls. */
const imageSolved = computed(() => {
  const surfaces = wb.system.value?.surfaces;
  return !!surfaces && surfaces.length > 0 && surfaces[surfaces.length - 1].gap_solve.kind !== 'fixed';
});
const canSave = computed(
  () => wb.online.value && !wb.busy.value && !wb.stale.value && !wb.refreshPending.value && wb.dirty.value && wb.problems.value.length === 0,
);

function onTabKey(e: KeyboardEvent, index: number): void {
  const step = e.key === 'ArrowDown' || e.key === 'ArrowRight' ? 1 : e.key === 'ArrowUp' || e.key === 'ArrowLeft' ? -1 : 0;
  const target = e.key === 'Home' ? 0 : e.key === 'End' ? TABS.length - 1 : step ? (index + step + TABS.length) % TABS.length : -1;
  if (target < 0) return;
  e.preventDefault();
  tab.value = TABS[target] as Tab;
  void nextTick(() => document.getElementById(`tab-${target}`)?.focus());
}
</script>

<template>
  <div class="app">
    <a class="skip-link" href="#main">Skip to workbench</a>
    <header>
      <div class="brand"><span class="mark" aria-hidden="true">◈</span> Hissabeth <small>Optical workbench</small></div>
      <div class="header-actions">
        <span role="status"><span class="status-dot" :class="{ offline: !wb.online.value }" aria-hidden="true"></span>{{ wb.online.value ? 'Engine connected' : 'Engine offline' }}</span>
        <button type="button" @click="connect">Connect agents</button>
      </div>
    </header>
    <div class="workspace">
      <aside aria-label="Project">
        <div class="project-label">Current system</div>
        <h2>{{ wb.draft.value?.name || 'No system loaded' }}</h2>
        <p>Sequential optics<br />Millimeters · {{ conjugate }}</p>
        <div role="tablist" aria-label="Workbench views" aria-orientation="vertical">
          <button
            v-for="(t, i) in TABS"
            :id="`tab-${i}`"
            :key="t"
            type="button"
            role="tab"
            :aria-selected="tab === t"
            aria-controls="main"
            :tabindex="tab === t ? 0 : -1"
            @click="tab = t"
            @keydown="onTabKey($event, i)"
          >{{ t }}</button>
        </div>
        <div class="aside-bottom">
          <span class="status-dot" aria-hidden="true"></span>CPU · f64 precision
          <p>Small optical workloads stay on CPU.</p>
          <p>Revision {{ wb.revision.value }}</p>
        </div>
      </aside>
      <main id="main" tabindex="-1" role="tabpanel" :aria-labelledby="`tab-${TABS.indexOf(tab)}`">
        <div class="toolbar">
          <div><h1>{{ tab }}</h1><span>Design through your agent. Inspect every result.</span></div>
          <div>
            <button
              type="button"
              :disabled="!wb.online.value || !wb.system.value || wb.dirty.value || wb.busy.value || wb.stale.value || wb.refreshPending.value || imageSolved"
              :title="imageSolved ? 'The final gap is controlled by a gap solve; remove that solve to use paraxial focus.' : undefined"
              @click="wb.autofocus()"
            >Paraxial focus</button>
            <button type="button" :disabled="!wb.dirty.value || wb.busy.value" @click="wb.revert()">Revert</button>
            <button type="button" class="primary" :disabled="!canSave" @click="wb.save()">{{ wb.dirty.value ? 'Apply changes' : 'Save system' }}</button>
          </div>
        </div>

        <div v-if="!wb.online.value" role="alert" class="banner error">
          Engine offline — {{ wb.offlineReason.value || 'no response from the optical engine' }}. Retrying automatically; analysis results are cleared until the engine responds.
        </div>
        <div v-if="wb.stale.value" role="alert" class="banner warn">
          The saved system changed while your draft was open, so the draft must be reloaded before it can be applied.
          <button type="button" @click="wb.discardAndReload()">Discard draft and reload</button>
        </div>
        <div v-if="wb.refreshPending.value" role="alert" class="banner warn">
          <template v-if="wb.stale.value">
            Your change was saved, but the view could not refresh, and your draft has changed since. It will not be replaced automatically: use “Discard draft and reload” to see the saved system. Do not repeat the operation.
          </template>
          <template v-else>
            Your change was saved, but the view could not refresh. It will update automatically once the engine responds (unless you edit the draft first); do not repeat the operation.
          </template>
        </div>
        <div v-if="wb.actionError.value" role="alert" class="banner error">{{ wb.actionError.value }}</div>
        <div v-if="wb.analysisError.value" role="alert" class="banner error">{{ wb.analysisError.value }}</div>
        <div v-if="agentError" role="alert" class="banner error">{{ agentError }}</div>
        <ul v-if="wb.dirty.value && wb.problems.value.length" class="problems" aria-label="Draft problems">
          <li v-for="p in wb.problems.value" :key="p">{{ p }}</li>
        </ul>

        <p v-if="!wb.system.value && tab !== 'Agent connections'" class="empty" role="status">
          {{ wb.online.value ? 'Loading system…' : 'No system loaded.' }}
        </p>

        <template v-if="wb.system.value && wb.draft.value">
          <MetricsBar v-if="wb.paraxial.value && showMetrics" :paraxial="wb.paraxial.value" :spot="wb.spot.value" />

          <LayoutView v-if="tab === 'Layout'" :system="wb.system.value" :layout="wb.layout.value" :wavelength="wb.layoutWavelength.value" />
          <SpotView
            v-if="tab === 'Spot diagram'"
            v-model:field-index="wb.fieldIndex.value"
            v-model:wavelength-index="wb.wavelengthIndex.value"
            :system="wb.system.value"
            v-model:sampling="wb.spotSampling.value"
            :spot="wb.spot.value"
            :spot-run="wb.spotRun.value"
            :spot-pending="wb.spotPending.value"
            :revision="wb.revision.value"
            :online="wb.online.value"
          />
          <MtfView
            v-if="tab === 'MTF'"
            v-model:field-index="wb.fieldIndex.value"
            v-model:wavelength-index="wb.wavelengthIndex.value"
            v-model:axis="wb.mtfAxis.value"
            v-model:max-frequency="wb.mtfMaxFrequency.value"
            v-model:sampling="wb.mtfSampling.value"
            v-model:spectral-mode="wb.mtfSpectralMode.value"
            :system="wb.system.value"
            :mtf="wb.mtf.value"
            :run="wb.mtfRun.value"
            :pending="wb.mtfPending.value"
          />
          <AnalysisPanel
            v-if="tab === 'Analysis'"
            :system="wb.system.value"
            :revision="wb.revision.value"
            :online="wb.online.value"
            :dirty="wb.dirty.value"
          />
          <RayInspection
            v-if="tab === 'Ray inspection'"
            :system="wb.system.value"
            :revision="wb.revision.value"
            :online="wb.online.value"
            :dirty="wb.dirty.value"
          />
          <MaterialsPanel
            v-if="tab === 'Materials'"
            v-model="wb.draft.value"
            :online="wb.online.value"
            :dirty="wb.dirty.value"
            :busy="wb.busy.value"
            :mutate="wb.mutate"
          />
          <ImportExportPanel
            v-if="tab === 'Import / Export'"
            :system="wb.system.value"
            :online="wb.online.value"
            :dirty="wb.dirty.value"
            :busy="wb.busy.value"
            :mutate="wb.mutate"
          />
          <SystemEditor v-if="tab === 'Prescription'" v-model="wb.draft.value" />
          <GapSolveEditor
            v-if="tab === 'Prescription'"
            :system="wb.system.value"
            :online="wb.online.value"
            :dirty="wb.dirty.value"
            :stale="wb.stale.value || wb.refreshPending.value"
            :busy="wb.busy.value"
            :mutate="wb.mutate"
          />
          <ApertureTools
            v-if="tab === 'Prescription' && wb.system.value"
            :system="wb.system.value"
            :revision="wb.revision.value"
            :online="wb.online.value"
            :dirty="wb.dirty.value"
            :stale="wb.stale.value || wb.refreshPending.value"
            :busy="wb.busy.value"
            :mutate="wb.mutate"
          />
          <JsonEditor v-if="tab === 'Prescription JSON'" v-model="wb.draft.value" />
          <SurfaceTable v-if="showTable" v-model="wb.draft.value" :materials="wb.materials.value" :dirty="wb.dirty.value" />
        </template>

        <AgentPanel v-if="tab === 'Agent connections'" :agents="agents" :registration="registration" @connect="connect" />
      </main>
    </div>
    <footer><span>Hissabeth / Sequential design</span><span>Rust optical core · MCP tools · Local workspace</span></footer>
  </div>
</template>
