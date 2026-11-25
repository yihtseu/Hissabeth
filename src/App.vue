<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useWindowSize, useEventListener, useLocalStorage } from '@vueuse/core';
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
    MessageSquare,
    PanelLeft,
    PanelBottom,
    Save,
    Undo2,
    Redo2,
    RotateCcw,
    Crosshair,
    X,
} from 'lucide-vue-next';
import { usePreferencesStore } from './stores/preferences';
import { useAppearance } from './composables/useAppearance';
import { tr } from './i18n';
import { errorMessage } from './api';
import { useWorkbench } from './composables/useWorkbench';
import { hoverSurface } from './composables/useSurfaceSelection';
import AnalysisPanel from './components/AnalysisPanel.vue';
import ApertureTools from './components/ApertureTools.vue';
import ConfigurationPanel from './components/ConfigurationPanel.vue';
import GapSolveEditor from './components/GapSolveEditor.vue';
import HistoryPanel from './components/HistoryPanel.vue';
import ImportExportPanel from './components/ImportExportPanel.vue';
import MaterialsPanel from './components/MaterialsPanel.vue';
import OpticalLayoutView from './components/OpticalLayoutView.vue';
import OptimizePanel from './components/OptimizePanel.vue';
import PrescriptionView from './components/PrescriptionView.vue';
import ChatPanel from './components/ChatPanel.vue';
import MetricsBar from './components/MetricsBar.vue';
import SurfaceTable from './components/SurfaceTable.vue';
import SystemEditor from './components/SystemEditor.vue';
import TolerancePanel from './components/TolerancePanel.vue';
import MtfView from './components/MtfView.vue';
import PrescriptionReport from './components/PrescriptionReport.vue';
import RayInspection from './components/RayInspection.vue';
import RecoveryPanel from './components/RecoveryPanel.vue';
import SpotView from './components/SpotView.vue';
import { TABS, type Tab } from './types';

useAppearance();
const preferences = usePreferencesStore();
const p = computed(() => preferences.preferences);
const savedView = useLocalStorage('hissabeth.activeView', '');
const initialView =
    p.value.workspace.restoreLastView && TABS.includes(savedView.value as Tab)
        ? (savedView.value as Tab)
        : p.value.workspace.defaultView;
const tab = ref<Tab>(initialView === 'Agent connections' ? 'Layout' : initialView);
const views = TABS.filter((view) => view !== 'Agent connections');
type WorkbenchView = (typeof views)[number];
const navGroups: { key: string; items: WorkbenchView[] }[] = [
    { key: 'inspect', items: ['Layout', 'Spot diagram', 'MTF', 'Analysis', 'Ray inspection'] },
    { key: 'design', items: ['Edit', 'Optimize', 'Tolerance', 'Configurations'] },
    {
        key: 'project',
        items: ['Prescription', 'Prescription report', 'Materials', 'History', 'Import / Export'],
    },
];
const wantMtf = computed(() => tab.value === 'MTF');
const wb = useWorkbench(wantMtf);
const { width } = useWindowSize();
const narrow = computed(() => width.value < 800);
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
const canQuickAction = computed(
    () =>
        wb.online.value &&
        !wb.recovery.value &&
        !wb.busy.value &&
        !wb.stale.value &&
        !wb.refreshPending.value &&
        !wb.dirty.value &&
        !!wb.system.value,
);
const activeFileName = computed(() => {
    const filePath = wb.activeProjectPath.value;
    if (!filePath) return '';
    return filePath.split(/[\\/]/).pop() ?? filePath;
});
const navDrawer = ref(false);
const settingsError = ref('');
const chatOpen = useLocalStorage('hissabeth.assistantOpen', true);
const chatSize = useLocalStorage('hissabeth.assistantSize', 32);
watch(tab, (value) => {
    savedView.value = value;
    hoverSurface(null);
});
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
const exchange = ref<InstanceType<typeof ImportExportPanel>>();
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
    if (event.event && chatOpen.value && !narrow.value) {
        const pane = event.panes[event.panes.length - 1];
        if (pane) chatSize.value = pane.size;
    }
    if (event.event && event.panes[0] && p.value.workspace.showNavigator && !narrow.value)
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
    const target = event.target as HTMLElement | null;
    const typing =
        !!target &&
        (target.tagName === 'INPUT' ||
            target.tagName === 'TEXTAREA' ||
            target.tagName === 'SELECT' ||
            target.isContentEditable);
    if (event.ctrlKey || event.metaKey) {
        if (event.key === ',') {
            event.preventDefault();
            void openSettings();
        } else if (event.key.toLowerCase() === 's') {
            event.preventDefault();
            if (canSave.value) void wb.save();
        } else if (!typing && event.key.toLowerCase() === 'z' && !event.shiftKey) {
            if (canQuickAction.value && wb.canUndo.value) {
                event.preventDefault();
                void wb.undo();
            }
        } else if (
            !typing &&
            ((event.key.toLowerCase() === 'z' && event.shiftKey) || event.key.toLowerCase() === 'y')
        ) {
            if (canQuickAction.value && wb.canRedo.value) {
                event.preventDefault();
                void wb.redo();
            }
        } else if (event.key.toLowerCase() === 'b') {
            event.preventDefault();
            toggleNavigator();
        } else if (event.key.toLowerCase() === 'j') {
            event.preventDefault();
            toggleDock();
        }
    }
});
const layoutSection = ref(true);
const showTable = computed(
    () =>
        (tab.value === 'Layout' && layoutSection.value) ||
        ['Spot diagram', 'MTF', 'Prescription'].includes(tab.value),
);
const showMetrics = computed(
    () => showTable.value || ['Analysis', 'Optimize', 'Tolerance'].includes(tab.value),
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
                <MenubarRoot class="menubar" :aria-label="tr('app.workbenchViews')">
                    <MenubarMenu
                        ><MenubarTrigger>{{ tr('menu.file') }}</MenubarTrigger
                        ><MenubarPortal
                            ><MenubarContent class="menu-popup"
                                ><MenubarItem @select="newSystem">{{
                                    tr('menu.newSystem')
                                }}</MenubarItem
                                ><MenubarItem @select="activate('Import / Export')">{{
                                    tr('menu.importExport')
                                }}</MenubarItem
                                ><MenubarSeparator /><MenubarItem
                                    @select="getCurrentWindow().close()"
                                    >{{ tr('menu.quit') }}</MenubarItem
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
                                ><MenubarSeparator /><MenubarItem
                                    :disabled="!canQuickAction || !wb.canUndo.value"
                                    @select="wb.undo()"
                                    >{{ tr('menu.undo') }}<kbd>Ctrl+Z</kbd></MenubarItem
                                ><MenubarItem
                                    :disabled="!canQuickAction || !wb.canRedo.value"
                                    @select="wb.redo()"
                                    >{{ tr('menu.redo') }}<kbd>Ctrl+Shift+Z</kbd></MenubarItem
                                ><MenubarSeparator /><MenubarItem
                                    :disabled="!canQuickAction"
                                    @select="wb.autofocus()"
                                    >{{ tr('menu.paraxialFocus') }}</MenubarItem
                                ></MenubarContent
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
                        ><MenubarTrigger>{{ tr('menu.tools') }}</MenubarTrigger
                        ><MenubarPortal
                            ><MenubarContent class="menu-popup"
                                ><MenubarItem @select="openSettings('agents')">{{
                                    tr('menu.agentConnections')
                                }}</MenubarItem
                                ><MenubarItem @select="openSettings"
                                    >{{ tr('menu.settings') }}<kbd>Ctrl+,</kbd></MenubarItem
                                ></MenubarContent
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
                    ><RotateCcw :size="16"
                /></ToolbarButton>
                <ToolbarSeparator />
                <ToolbarButton
                    class="secondary-tool"
                    :disabled="!canQuickAction || !wb.canUndo.value"
                    :title="tr('menu.undo') + ' (Ctrl+Z)'"
                    :aria-label="tr('menu.undo')"
                    @click="wb.undo()"
                    ><Undo2 :size="16"
                /></ToolbarButton>
                <ToolbarButton
                    class="secondary-tool"
                    :disabled="!canQuickAction || !wb.canRedo.value"
                    :title="tr('menu.redo') + ' (Ctrl+Shift+Z)'"
                    :aria-label="tr('menu.redo')"
                    @click="wb.redo()"
                    ><Redo2 :size="16"
                /></ToolbarButton>
                <ToolbarButton
                    class="secondary-tool"
                    :disabled="!canQuickAction"
                    :title="tr('app.paraxialFocus')"
                    :aria-label="tr('app.paraxialFocus')"
                    @click="wb.autofocus()"
                    ><Crosshair :size="16" /><span class="tool-label">{{
                        tr('app.paraxialFocus')
                    }}</span></ToolbarButton
                >
                <select
                    v-if="narrow || !p.workspace.showNavigator"
                    :aria-label="tr('app.workbenchViews')"
                    :value="tab"
                    @change="activate(($event.target as HTMLSelectElement).value as Tab)"
                >
                    <option v-for="view in views" :key="view" :value="view">
                        {{ tr(`tabs.${view}`) }}
                    </option>
                </select>
                <div class="doc-inline">
                    <strong>{{
                        wb.system.value
                            ? wb.system.value.name || tr('app.unnamedSystem')
                            : tr('app.noSystem')
                    }}</strong>
                    <span
                        v-if="activeFileName"
                        class="file-pill"
                        :title="wb.activeProjectPath.value ?? ''"
                        >{{ activeFileName }}</span
                    >
                    <span v-if="wb.dirty.value" class="unsaved">● {{ tr('statusbar.draft') }}</span>
                    <span class="doc-rev">{{ tr('app.revision', { n: wb.revision.value }) }}</span>
                </div>
                <div class="spacer"></div>
                <ToolbarButton
                    :aria-label="tr('menu.prescriptionDock')"
                    :title="tr('menu.prescriptionDock')"
                    @click="toggleDock"
                    ><PanelBottom :size="16"
                /></ToolbarButton>
                <ToolbarButton
                    :aria-label="tr('chat.title')"
                    :title="tr('chat.title')"
                    :aria-pressed="chatOpen"
                    @click="chatOpen = !chatOpen"
                >
                    <MessageSquare :size="16" />
                </ToolbarButton>
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
            <button
                v-if="chatOpen && narrow"
                class="assistant-backdrop"
                :aria-label="tr('chat.close')"
                @click="chatOpen = false"
            ></button>
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
                            <template v-for="group in navGroups" :key="group.key">
                                <div class="nav-group-label">{{ tr(`nav.${group.key}`) }}</div>
                                <button
                                    v-for="view in group.items"
                                    :id="`view-${TABS.indexOf(view)}`"
                                    :key="view"
                                    type="button"
                                    role="tab"
                                    :aria-selected="tab === view"
                                    aria-controls="workspace-panel"
                                    :tabindex="tab === view ? 0 : -1"
                                    @click="activate(view)"
                                    @keydown="onTabKey($event, views.indexOf(view))"
                                >
                                    {{ tr(`tabs.${view}`) }}
                                </button>
                            </template>
                        </div>
                        <div class="navigator-meta">CPU · f64<br />mm / nm</div>
                    </aside></Pane
                >
                <Pane :min-size="30">
                    <Splitpanes
                        horizontal
                        @resized="resizeDock"
                        @splitter-dblclick="preferences.patchWorkspace({ dockSize: 35 })"
                    >
                        <Pane :min-size="40"
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
                                        :chat-open="chatOpen"
                                        @chat="chatOpen = true"
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

                                        <OpticalLayoutView
                                            v-show="tab === 'Layout'"
                                            :system="wb.system.value"
                                            :saved-to-draft="wb.savedToDraftSurfaceIndexes.value"
                                            :layout="wb.layout.value"
                                            :wavelength="wb.layoutWavelength.value"
                                            :revision="wb.revision.value"
                                            :online="wb.online.value"
                                            :dirty="wb.dirty.value"
                                            :busy="wb.busy.value"
                                            :stale="wb.stale.value"
                                            :refresh-pending="wb.refreshPending.value"
                                            :mutate="wb.mutate"
                                            :chat-open="chatOpen"
                                            @chat="chatOpen = true"
                                            @scope="layoutSection = $event"
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
                                            :model-value="wb.system.value"
                                            :online="wb.online.value"
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
                                        <template v-if="tab === 'Edit'">
                                            <SystemEditor v-model="wb.draft.value" />
                                            <SurfaceTable
                                                v-model="wb.draft.value"
                                                v-model:expanded="wb.expandedSurfaceDetails.value"
                                                :surface-keys="wb.draftSurfaceKeys.value"
                                                :materials="wb.materials.value"
                                                :dirty="wb.dirty.value"
                                                :online="wb.online.value"
                                                :variables="
                                                    wb.optimizationVariables.value ?? undefined
                                                "
                                                @toggle-variable="wb.toggleOptimizationVariable"
                                                @structure-edit="wb.applySurfaceEdit"
                                            />
                                            <GapSolveEditor
                                                :system="wb.system.value"
                                                :online="wb.online.value"
                                                :dirty="wb.dirty.value"
                                                :stale="wb.stale.value || wb.refreshPending.value"
                                                :busy="wb.busy.value"
                                                :mutate="wb.mutate"
                                            />
                                            <ApertureTools
                                                :system="wb.system.value"
                                                :revision="wb.revision.value"
                                                :online="wb.online.value"
                                                :dirty="wb.dirty.value"
                                                :stale="wb.stale.value || wb.refreshPending.value"
                                                :busy="wb.busy.value"
                                                :mutate="wb.mutate"
                                            />
                                        </template>
                                        <OptimizePanel
                                            v-if="tab === 'Optimize'"
                                            v-model:variables="wb.optimizationVariables.value"
                                            :system="wb.draft.value"
                                            :draft-structure-changed="
                                                wb.draftStructureChanged.value
                                            "
                                            :paraxial="wb.paraxial.value"
                                            :revision="wb.revision.value"
                                            :online="wb.online.value"
                                            :dirty="wb.dirty.value"
                                            :busy="wb.busy.value"
                                            :stale="wb.stale.value"
                                            :refresh-pending="wb.refreshPending.value"
                                            :mutate="wb.mutate"
                                        />
                                        <TolerancePanel
                                            v-if="tab === 'Tolerance'"
                                            :system="wb.system.value"
                                            :revision="wb.revision.value"
                                            :online="wb.online.value"
                                        />
                                        <HistoryPanel
                                            v-if="tab === 'History'"
                                            :revision="wb.revision.value"
                                            :online="wb.online.value"
                                            :dirty="wb.dirty.value"
                                            :busy="wb.busy.value"
                                            :stale="wb.stale.value"
                                            :refresh-pending="wb.refreshPending.value"
                                            :mutate="wb.mutate"
                                        />
                                        <ConfigurationPanel
                                            v-if="tab === 'Configurations'"
                                            v-model="wb.draft.value"
                                            :materials="wb.materials.value"
                                            :online="wb.online.value"
                                        />
                                        <PrescriptionView
                                            v-if="tab === 'Prescription'"
                                            :system="wb.system.value"
                                            :chat-open="chatOpen"
                                        />
                                    </template>
                                </div></main
                        ></Pane>
                        <Pane
                            v-if="
                                showTable &&
                                p.workspace.showPrescriptionDock &&
                                !wb.recovery.value &&
                                wb.system.value
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
                                <PrescriptionView :system="wb.system.value!" compact /></section
                        ></Pane>
                    </Splitpanes>
                </Pane>
                <Pane
                    v-if="chatOpen"
                    :size="narrow ? 0 : chatSize"
                    :min-size="narrow ? 0 : 24"
                    :max-size="55"
                    class="assistant-pane"
                    :class="{ 'assistant-overlay': narrow }"
                >
                    <ChatPanel
                        :visible="chatOpen"
                        :online="wb.online.value"
                        :dirty="wb.dirty.value"
                        :locked="wb.busy.value || wb.stale.value || wb.refreshPending.value"
                        @settings="openSettings('agents')"
                        @close="chatOpen = false"
                    />
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
                            <template v-for="group in navGroups" :key="group.key">
                                <div class="nav-group-label">{{ tr(`nav.${group.key}`) }}</div>
                                <button
                                    v-for="view in group.items"
                                    :id="`view-${TABS.indexOf(view)}`"
                                    :key="view"
                                    type="button"
                                    role="tab"
                                    :aria-selected="tab === view"
                                    aria-controls="workspace-panel"
                                    :tabindex="tab === view ? 0 : -1"
                                    @click="activate(view)"
                                    @keydown="onTabKey($event, views.indexOf(view))"
                                >
                                    {{ tr(`tabs.${view}`) }}
                                </button>
                            </template>
                        </div></DialogContent
                    ></DialogPortal
                ></DialogRoot
            >
        </div>
    </TooltipProvider>
</template>
