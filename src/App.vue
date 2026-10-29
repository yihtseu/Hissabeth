<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useWindowSize, useEventListener } from '@vueuse/core';
import { Splitpanes, Pane, type SplitpanesResizedPayload } from 'splitpanes';
import {
    MenubarRoot,
    MenubarMenu,
    MenubarTrigger,
    MenubarPortal,
    MenubarContent,
    MenubarItem,
    MenubarSeparator,
    ToolbarRoot,
    ToolbarButton,
    ToolbarSeparator,
    DialogRoot,
    DialogPortal,
    DialogOverlay,
    DialogContent,
    DialogTitle,
    DialogClose,
    TooltipProvider,
    TooltipRoot,
    TooltipTrigger,
    TooltipPortal,
    TooltipContent,
} from 'reka-ui';
import {
    Settings2,
    PanelLeft,
    PanelBottom,
    Save,
    Undo2,
    Focus,
    FilePlus2,
    FileInput,
    X,
} from 'lucide-vue-next';
import { usePreferencesStore } from './stores/preferences';
import { useAppearance } from './composables/useAppearance';
import { tr } from './i18n';
import { errorMessage } from './api';
import { useWorkbench } from './composables/useWorkbench';
import AnalysisPanel from './components/AnalysisPanel.vue';
import ApertureTools from './components/ApertureTools.vue';
import GapSolveEditor from './components/GapSolveEditor.vue';
import ImportExportPanel from './components/ImportExportPanel.vue';
import JsonEditor from './components/JsonEditor.vue';
import MaterialsPanel from './components/MaterialsPanel.vue';
import LayoutView from './components/LayoutView.vue';
import BenchView from './components/BenchView.vue';
import MetricsBar from './components/MetricsBar.vue';
import MtfView from './components/MtfView.vue';
import PrescriptionReport from './components/PrescriptionReport.vue';
import RayInspection from './components/RayInspection.vue';
import RecoveryPanel from './components/RecoveryPanel.vue';
import SpotView from './components/SpotView.vue';
import SurfaceTable from './components/SurfaceTable.vue';
import SystemEditor from './components/SystemEditor.vue';
import { TABS, type Tab } from './types';

useAppearance();
const preferences = usePreferencesStore();
const p = computed(() => preferences.preferences);
const savedView = localStorage.getItem('hissabeth.activeView');
const initialView =
    p.value.workspace.restoreLastView && TABS.includes(savedView as Tab)
        ? (savedView as Tab)
        : p.value.workspace.defaultView;
const tab = ref<Tab>(initialView === 'Agent connections' ? 'Layout' : initialView);
const views = TABS.filter((view) => view !== 'Agent connections');
const wantMtf = computed(() => tab.value === 'MTF');
const wb = useWorkbench(wantMtf);
const { width } = useWindowSize();
const narrow = computed(() => width.value < 800);
const navDrawer = ref(false);
const settingsError = ref('');
const exchange = ref<InstanceType<typeof ImportExportPanel>>();
watch(tab, (value) => localStorage.setItem('hissabeth.activeView', value));
async function openSettings(section: unknown = 'general'): Promise<void> {
    try {
        await invoke('open_settings', { section: section === 'agents' ? 'agents' : 'general' });
        settingsError.value = '';
    } catch (error) {
        settingsError.value = errorMessage(error);
    }
}
function activate(view: Tab): void {
    tab.value = view;
    navDrawer.value = false;
}
async function newSystem(): Promise<void> {
    tab.value = 'Import / Export';
    await nextTick();
    exchange.value?.openNew();
}
function toggleNavigator(): void {
    if (narrow.value) navDrawer.value = !navDrawer.value;
    else void preferences.patchWorkspace({ showNavigator: !p.value.workspace.showNavigator });
}
function toggleDock(): void {
    void preferences.patchWorkspace({
        showPrescriptionDock: !p.value.workspace.showPrescriptionDock,
    });
}
function resizeNav(event: SplitpanesResizedPayload): void {
    if (event.event && event.panes[0])
        void preferences.patchWorkspace({
            navigatorSize: Math.max(10, Math.min(40, Math.round(event.panes[0].size))),
        });
}
function resizeDock(event: SplitpanesResizedPayload): void {
    if (event.event && event.panes[1])
        void preferences.patchWorkspace({
            dockSize: Math.max(15, Math.min(60, Math.round(event.panes[1].size))),
        });
}
useEventListener('keydown', (event: KeyboardEvent) => {
    if (event.ctrlKey || event.metaKey) {
        if (event.key === ',') {
            event.preventDefault();
            void openSettings();
        } else if (event.key.toLowerCase() === 's') {
            event.preventDefault();
            if (canSave.value) void wb.save();
        } else if (event.key.toLowerCase() === 'b') {
            event.preventDefault();
            toggleNavigator();
        } else if (event.key.toLowerCase() === 'j') {
            event.preventDefault();
            toggleDock();
        }
    }
});
const showTable = computed(() =>
    ['Layout', 'Spot diagram', 'MTF', 'Prescription'].includes(tab.value),
);
const showMetrics = computed(() => showTable.value || tab.value === 'Analysis');
/** Autofocus moves the image distance, which an active solve on the final gap already controls. */
const imageSolved = computed(() => {
    const surfaces = wb.system.value?.surfaces;
    return (
        !!surfaces &&
        surfaces.length > 0 &&
        surfaces[surfaces.length - 1].gap_solve.kind !== 'fixed'
    );
});
const canSave = computed(
    () =>
        wb.online.value &&
        !wb.recovery.value &&
        !wb.busy.value &&
        !wb.stale.value &&
        !wb.refreshPending.value &&
        wb.dirty.value &&
        wb.problems.value.length === 0,
);

function onTabKey(e: KeyboardEvent, index: number): void {
    const step =
        e.key === 'ArrowDown' || e.key === 'ArrowRight'
            ? 1
            : e.key === 'ArrowUp' || e.key === 'ArrowLeft'
              ? -1
              : 0;
    const target =
        e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? views.length - 1
              : step
                ? (index + step + views.length) % views.length
                : -1;
    if (target < 0) return;
    e.preventDefault();
    tab.value = views[target] as Tab;
    void nextTick(() => document.getElementById(`view-${target}`)?.focus());
}
</script>
<template>
    <TooltipProvider>
        <div class="industrial-app">
            <a class="skip-link" href="#main">{{ tr('app.skip') }}</a>
            <div class="menubar-shell">
                <span class="instrument-brand"><img src="/app-icon.png" alt="" />Hissabeth</span>
                <MenubarRoot class="menubar" :aria-label="tr('app.workbenchViews')">
                    <MenubarMenu
                        ><MenubarTrigger>{{ tr('menu.file') }}</MenubarTrigger
                        ><MenubarPortal
                            ><MenubarContent class="menu-popup"
                                ><MenubarItem
                                    @select="newSystem"
                                    :disabled="
                                        !wb.system.value ||
                                        !!wb.recovery.value ||
                                        wb.busy.value ||
                                        wb.dirty.value
                                    "
                                    >{{ tr('menu.newSystem') }}</MenubarItem
                                ><MenubarItem @select="activate('Import / Export')">{{
                                    tr('menu.importExport')
                                }}</MenubarItem
                                ><MenubarItem @select="activate('Prescription report')">{{
                                    tr('menu.exportReport')
                                }}</MenubarItem
                                ><MenubarSeparator /><MenubarItem @select="openSettings"
                                    >{{ tr('menu.settings') }}<kbd>Ctrl+,</kbd></MenubarItem
                                ></MenubarContent
                            ></MenubarPortal
                        ></MenubarMenu
                    >
                    <MenubarMenu
                        ><MenubarTrigger>{{ tr('menu.edit') }}</MenubarTrigger
                        ><MenubarPortal
                            ><MenubarContent class="menu-popup"
                                ><MenubarItem :disabled="!canSave" @select="wb.save()"
                                    >{{ tr('menu.applyChanges') }}<kbd>Ctrl+S</kbd></MenubarItem
                                ><MenubarItem
                                    :disabled="!wb.dirty.value || wb.busy.value"
                                    @select="wb.revert()"
                                    >{{ tr('menu.revertDraft') }}</MenubarItem
                                ><MenubarItem @select="activate('Prescription')">{{
                                    tr('tabs.Prescription')
                                }}</MenubarItem></MenubarContent
                            ></MenubarPortal
                        ></MenubarMenu
                    >
                    <MenubarMenu
                        ><MenubarTrigger>{{ tr('menu.view') }}</MenubarTrigger
                        ><MenubarPortal
                            ><MenubarContent class="menu-popup"
                                ><MenubarItem
                                    v-for="view in views"
                                    :key="view"
                                    @select="activate(view)"
                                    >{{ tr(`tabs.${view}`)
                                    }}<span v-if="tab === view">✓</span></MenubarItem
                                ><MenubarSeparator /><MenubarItem @select="toggleNavigator"
                                    >{{ tr('menu.navigator') }}<kbd>Ctrl+B</kbd></MenubarItem
                                ><MenubarItem @select="toggleDock"
                                    >{{ tr('menu.prescriptionDock') }}<kbd>Ctrl+J</kbd></MenubarItem
                                ><MenubarItem
                                    @select="
                                        preferences.patchWorkspace({
                                            showMetrics: !p.workspace.showMetrics,
                                        })
                                    "
                                    >{{ tr('menu.metricsBar') }}
                                    {{ p.workspace.showMetrics ? '✓' : '' }}</MenubarItem
                                ><MenubarItem
                                    @select="
                                        preferences.patchWorkspace({
                                            showStatusBar: !p.workspace.showStatusBar,
                                        })
                                    "
                                    >{{ tr('menu.statusBar') }}
                                    {{ p.workspace.showStatusBar ? '✓' : '' }}</MenubarItem
                                ></MenubarContent
                            ></MenubarPortal
                        ></MenubarMenu
                    >
                    <MenubarMenu
                        ><MenubarTrigger>{{ tr('menu.analysis') }}</MenubarTrigger
                        ><MenubarPortal
                            ><MenubarContent class="menu-popup"
                                ><MenubarItem
                                    v-for="view in [
                                        'Layout',
                                        'Laser bench',
                                        'Spot diagram',
                                        'MTF',
                                        'Analysis',
                                        'Ray inspection',
                                        'Prescription report',
                                    ] as const"
                                    :key="view"
                                    @select="activate(view)"
                                    >{{ tr(`tabs.${view}`) }}</MenubarItem
                                ></MenubarContent
                            ></MenubarPortal
                        ></MenubarMenu
                    >
                    <MenubarMenu
                        ><MenubarTrigger>{{ tr('menu.tools') }}</MenubarTrigger
                        ><MenubarPortal
                            ><MenubarContent class="menu-popup"
                                ><MenubarItem @select="activate('Materials')">{{
                                    tr('menu.materials')
                                }}</MenubarItem
                                ><MenubarItem @select="openSettings('agents')">{{
                                    tr('menu.agentConnections')
                                }}</MenubarItem
                                ><MenubarItem @select="openSettings">{{
                                    tr('menu.settings')
                                }}</MenubarItem></MenubarContent
                            ></MenubarPortal
                        ></MenubarMenu
                    >
                </MenubarRoot>
            </div>
            <ToolbarRoot class="command-toolbar" :aria-label="tr('app.workbenchViews')">
                <ToolbarButton
                    :aria-label="tr('app.navigator')"
                    :title="tr('app.navigator')"
                    @click="toggleNavigator"
                    ><PanelLeft :size="16"
                /></ToolbarButton>
                <ToolbarSeparator />
                <ToolbarButton
                    class="secondary-tool"
                    :aria-label="tr('menu.newSystem')"
                    :title="tr('menu.newSystem')"
                    :disabled="
                        !wb.system.value || !!wb.recovery.value || wb.busy.value || wb.dirty.value
                    "
                    @click="newSystem"
                    ><FilePlus2 :size="16"
                /></ToolbarButton>
                <ToolbarButton
                    class="secondary-tool"
                    :aria-label="tr('menu.importExport')"
                    :title="tr('menu.importExport')"
                    @click="activate('Import / Export')"
                    ><FileInput :size="16"
                /></ToolbarButton>
                <ToolbarButton
                    :disabled="!canSave"
                    :title="tr('app.applyChanges')"
                    :aria-label="tr('app.applyChanges')"
                    @click="wb.save()"
                    ><Save :size="16" /><span class="tool-label">{{
                        tr('app.applyChanges')
                    }}</span></ToolbarButton
                >
                <ToolbarButton
                    :disabled="!wb.dirty.value || wb.busy.value || !!wb.recovery.value"
                    :title="tr('app.revert')"
                    :aria-label="tr('app.revert')"
                    @click="wb.revert()"
                    ><Undo2 :size="16"
                /></ToolbarButton>
                <ToolbarSeparator />
                <ToolbarButton
                    :disabled="
                        !wb.online.value ||
                        !wb.system.value ||
                        !!wb.recovery.value ||
                        wb.dirty.value ||
                        wb.busy.value ||
                        wb.stale.value ||
                        wb.refreshPending.value ||
                        imageSolved
                    "
                    :aria-label="tr('app.paraxialFocus')"
                    :title="tr(imageSolved ? 'app.paraxialFocusBlocked' : 'app.paraxialFocus')"
                    @click="wb.autofocus()"
                    ><Focus :size="16" /><span class="tool-label">{{
                        tr('app.paraxialFocus')
                    }}</span></ToolbarButton
                >
                <select
                    :aria-label="tr('app.workbenchViews')"
                    :value="tab"
                    @change="activate(($event.target as HTMLSelectElement).value as Tab)"
                >
                    <option v-for="view in views" :key="view" :value="view">
                        {{ tr(`tabs.${view}`) }}
                    </option>
                </select>
                <div class="spacer"></div>
                <ToolbarButton
                    :aria-label="tr('menu.prescriptionDock')"
                    :title="tr('menu.prescriptionDock')"
                    @click="toggleDock"
                    ><PanelBottom :size="16"
                /></ToolbarButton>
                <TooltipRoot
                    ><TooltipTrigger as-child
                        ><ToolbarButton :aria-label="tr('settings.title')" @click="openSettings"
                            ><Settings2 :size="16" /></ToolbarButton></TooltipTrigger
                    ><TooltipPortal
                        ><TooltipContent class="tooltip"
                            >{{ tr('app.openSettings') }} (Ctrl+,)</TooltipContent
                        ></TooltipPortal
                    ></TooltipRoot
                >
            </ToolbarRoot>
            <div class="document-strip">
                <strong>{{
                    wb.draft.value
                        ? wb.draft.value.name || tr('app.unnamedSystem')
                        : tr('app.noSystem')
                }}</strong
                ><span v-if="wb.dirty.value" class="unsaved">● {{ tr('statusbar.draft') }}</span
                ><span class="spacer"></span
                ><span>{{ tr('app.revision', { n: wb.revision.value }) }}</span>
            </div>
            <div
                v-if="
                    !preferences.loaded ||
                    preferences.syncError ||
                    preferences.saveError ||
                    preferences.conflictMessage ||
                    settingsError
                "
                class="banner warn"
                role="alert"
            >
                {{
                    tr('app.prefsUnavailable', {
                        reason:
                            preferences.loadError ||
                            preferences.syncError ||
                            preferences.saveError ||
                            preferences.conflictMessage ||
                            settingsError,
                    })
                }}<button @click="preferences.retry()">{{ tr('app.prefsRetry') }}</button>
            </div>
            <Splitpanes class="workbench-split" @resized="resizeNav">
                <Pane
                    v-if="p.workspace.showNavigator && !narrow"
                    :size="p.workspace.navigatorSize"
                    :min-size="10"
                    :max-size="40"
                    ><aside class="navigator">
                        <div class="dock-heading">
                            {{ tr('app.navigator')
                            }}<button
                                :aria-label="tr('app.navigatorHide')"
                                @click="toggleNavigator"
                            >
                                <X :size="14" />
                            </button>
                        </div>
                        <div
                            role="tablist"
                            :aria-label="tr('app.workbenchViews')"
                            aria-orientation="vertical"
                            class="view-list"
                        >
                            <button
                                v-for="view in views"
                                :id="`view-${TABS.indexOf(view)}`"
                                :key="view"
                                type="button"
                                role="tab"
                                :aria-selected="tab === view"
                                aria-controls="workspace-panel"
                                :tabindex="tab === view ? 0 : -1"
                                @click="activate(view)"
                                @keydown="onTabKey($event, TABS.indexOf(view))"
                            >
                                {{ tr(`tabs.${view}`) }}
                            </button>
                        </div>
                        <div class="navigator-meta">CPU · f64<br />mm / nm</div>
                    </aside></Pane
                >
                <Pane
                    :size="
                        p.workspace.showNavigator && !narrow ? 100 - p.workspace.navigatorSize : 100
                    "
                    :min-size="60"
                >
                    <Splitpanes
                        horizontal
                        @resized="resizeDock"
                        @splitter-dblclick="preferences.patchWorkspace({ dockSize: 35 })"
                    >
                        <Pane
                            :size="
                                showTable && p.workspace.showPrescriptionDock && !wb.recovery.value
                                    ? 100 - p.workspace.dockSize
                                    : 100
                            "
                            :min-size="40"
                            ><main id="main" class="analysis-workspace" tabindex="-1">
                                <div
                                    id="workspace-panel"
                                    role="tabpanel"
                                    :aria-label="tr(`tabs.${tab}`)"
                                >
                                    <div class="view-heading">
                                        <h1>{{ tr(`tabs.${tab}`) }}</h1>
                                        <span role="status" class="connection-state"
                                            ><i
                                                class="status-dot"
                                                :class="{ offline: !wb.online.value }"
                                            ></i
                                            >{{
                                                tr(
                                                    !wb.online.value
                                                        ? 'app.engineOffline'
                                                        : wb.recovery.value
                                                          ? 'app.engineRecovery'
                                                          : 'app.engineConnected',
                                                )
                                            }}</span
                                        >
                                    </div>
                                    <div v-if="!wb.online.value" role="alert" class="banner error">
                                        {{ tr('fragments.engineOffline') }}
                                        {{ wb.offlineReason.value || tr('banner.offlineNoReason')
                                        }}{{
                                            tr(
                                                'fragments.retryingAutomaticallyAnalysisResultsAreClearedUntilTheEngineResponds',
                                            )
                                        }}
                                    </div>
                                    <div
                                        v-if="wb.stale.value && wb.recovery.value"
                                        role="status"
                                        class="banner warn"
                                    >
                                        {{ tr('banner.staleRecovery') }}
                                    </div>
                                    <div
                                        v-else-if="wb.stale.value"
                                        role="alert"
                                        class="banner warn"
                                    >
                                        {{ tr('banner.stale') }}
                                        <button type="button" @click="wb.discardAndReload()">
                                            {{ tr('banner.discardReload') }}
                                        </button>
                                    </div>
                                    <div
                                        v-if="wb.refreshPending.value"
                                        role="alert"
                                        class="banner warn"
                                    >
                                        <template v-if="wb.stale.value && wb.recovery.value">
                                            {{ tr('banner.refreshKeptRecovery') }}
                                        </template>
                                        <template v-else-if="wb.stale.value">
                                            {{ tr('banner.refreshKeptStale') }}
                                        </template>
                                        <template v-else>
                                            {{ tr('banner.refreshPlain') }}
                                        </template>
                                    </div>
                                    <div
                                        v-if="wb.actionError.value"
                                        role="alert"
                                        class="banner error"
                                    >
                                        {{ wb.actionError.value }}
                                    </div>
                                    <div
                                        v-if="wb.recoveryNotice.value"
                                        role="status"
                                        class="banner ok"
                                    >
                                        <span>
                                            {{ tr('banner.readyAgain') }}
                                            <template v-if="wb.recoveryNotice.value.path"
                                                >{{ tr('fragments.thePreviousFileWasPreservedAt') }}
                                                <code>{{ wb.recoveryNotice.value.path }}</code
                                                >.</template
                                            >
                                            <template v-else>{{
                                                tr('banner.nothingPreserved')
                                            }}</template>
                                        </span>
                                        <button type="button" @click="wb.dismissRecoveryNotice()">
                                            {{ tr('app.dismiss') }}
                                        </button>
                                    </div>
                                    <div
                                        v-if="wb.analysisError.value"
                                        role="alert"
                                        class="banner error"
                                    >
                                        {{ wb.analysisError.value }}
                                    </div>

                                    <ul
                                        v-if="wb.dirty.value && wb.problems.value.length"
                                        class="problems"
                                        :aria-label="tr('banner.draftProblems')"
                                    >
                                        <li v-for="p in wb.problems.value" :key="p">{{ p }}</li>
                                    </ul>

                                    <p
                                        v-if="!wb.recovery.value && !wb.system.value"
                                        class="empty"
                                        role="status"
                                    >
                                        {{
                                            wb.online.value
                                                ? tr('app.loadingSystem')
                                                : tr('app.noSystemLoaded')
                                        }}
                                    </p>

                                    <RecoveryPanel
                                        v-if="wb.recovery.value"
                                        :recovery="wb.recovery.value"
                                        :online="wb.online.value"
                                        :busy="wb.recoveryBusy.value || wb.recoveryPending.value"
                                        :error="wb.recoveryError.value"
                                        :text="wb.recoveryText.value"
                                        :format="wb.recoveryFormat.value"
                                        @update:text="wb.recoveryText.value = $event"
                                        @update:format="wb.recoveryFormat.value = $event"
                                        @retry="wb.retryFromDisk()"
                                        @restore="wb.restoreFromText()"
                                        @create="wb.createNewSystem()"
                                    />

                                    <template
                                        v-if="
                                            !wb.recovery.value && wb.system.value && wb.draft.value
                                        "
                                    >
                                        <MetricsBar
                                            v-if="
                                                wb.paraxial.value &&
                                                showMetrics &&
                                                p.workspace.showMetrics
                                            "
                                            :paraxial="wb.paraxial.value"
                                            :spot="wb.spot.value"
                                        />

                                        <LayoutView
                                            v-if="tab === 'Layout'"
                                            :system="wb.system.value"
                                            :layout="wb.layout.value"
                                            :wavelength="wb.layoutWavelength.value"
                                        />
                                        <BenchView
                                            v-if="tab === 'Laser bench'"
                                            :model-value="wb.draft.value.bench"
                                            :revision="wb.revision.value"
                                            :online="wb.online.value"
                                            :locked="
                                                wb.busy.value ||
                                                wb.stale.value ||
                                                wb.refreshPending.value
                                            "
                                            :can-save="canSave"
                                            @update:model-value="wb.draft.value.bench = $event"
                                            @save="wb.save()"
                                        />
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
                                        <PrescriptionReport
                                            v-if="tab === 'Prescription report'"
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
                                            ref="exchange"
                                            v-if="tab === 'Import / Export'"
                                            :system="wb.system.value"
                                            :revision="wb.revision.value"
                                            :stale="wb.stale.value"
                                            :refresh-pending="wb.refreshPending.value"
                                            :online="wb.online.value"
                                            :dirty="wb.dirty.value"
                                            :busy="wb.busy.value"
                                            :mutate="wb.mutate"
                                        />
                                        <SystemEditor
                                            v-if="tab === 'Prescription'"
                                            v-model="wb.draft.value"
                                        />
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
                                        <JsonEditor
                                            v-if="tab === 'Prescription JSON'"
                                            v-model="wb.draft.value"
                                        />
                                    </template>
                                </div></main
                        ></Pane>
                        <Pane
                            v-if="
                                showTable &&
                                p.workspace.showPrescriptionDock &&
                                !wb.recovery.value &&
                                wb.draft.value
                            "
                            :size="p.workspace.dockSize"
                            :min-size="15"
                            :max-size="60"
                            ><section class="prescription-dock">
                                <div class="dock-heading">
                                    {{ tr('tabs.Prescription')
                                    }}<button :aria-label="tr('app.dockHide')" @click="toggleDock">
                                        <X :size="14" />
                                    </button>
                                </div>
                                <SurfaceTable
                                    v-model="wb.draft.value"
                                    :materials="wb.materials.value"
                                    :dirty="wb.dirty.value"
                                    :online="wb.online.value"
                                /></section
                        ></Pane>
                    </Splitpanes>
                </Pane>
            </Splitpanes>
            <footer v-if="p.workspace.showStatusBar" class="statusbar">
                <span
                    ><i class="status-dot" :class="{ offline: !wb.online.value }"></i
                    >{{
                        tr(
                            !wb.online.value
                                ? 'statusbar.offline'
                                : wb.recovery.value
                                  ? 'statusbar.recovery'
                                  : wb.busy.value
                                    ? 'statusbar.busy'
                                    : 'statusbar.online',
                        )
                    }}</span
                ><span>{{ tr('statusbar.revision', { n: wb.revision.value }) }}</span
                ><span>{{ tr(wb.dirty.value ? 'statusbar.draft' : 'statusbar.saved') }}</span
                ><span class="spacer"></span><span>CPU · f64</span
                ><span class="status-poll">{{
                    tr('statusbar.polling', { ms: p.analysis.pollIntervalMs })
                }}</span
                ><span>mm / nm</span>
            </footer>
            <DialogRoot v-model:open="navDrawer"
                ><DialogPortal
                    ><DialogOverlay class="drawer-overlay" /><DialogContent class="navigator-drawer"
                        ><DialogTitle>{{ tr('app.navigator') }}</DialogTitle
                        ><DialogClose class="drawer-close" :aria-label="tr('common.close')"
                            ><X :size="18"
                        /></DialogClose>
                        <div
                            role="tablist"
                            :aria-label="tr('app.workbenchViews')"
                            aria-orientation="vertical"
                            class="view-list"
                        >
                            <button
                                v-for="view in views"
                                :id="`view-${TABS.indexOf(view)}`"
                                :key="view"
                                type="button"
                                role="tab"
                                :aria-selected="tab === view"
                                aria-controls="workspace-panel"
                                :tabindex="tab === view ? 0 : -1"
                                @click="activate(view)"
                                @keydown="onTabKey($event, TABS.indexOf(view))"
                            >
                                {{ tr(`tabs.${view}`) }}
                            </button>
                        </div></DialogContent
                    ></DialogPortal
                ></DialogRoot
            >
        </div>
    </TooltipProvider>
</template>
