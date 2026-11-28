<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open as openFileDialog, save as saveFileDialog } from '@tauri-apps/plugin-dialog';
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
    DropdownMenuRoot,
    DropdownMenuTrigger,
    DropdownMenuPortal,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuLabel,
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
    Activity,
    ArrowLeftRight,
    Boxes,
    CircleDot,
    Crosshair,
    FileText,
    FolderOpen,
    Gauge,
    History,
    Layers,
    ListOrdered,
    PanelLeft,
    PanelBottom,
    PencilRuler,
    Radio,
    Redo2,
    RotateCcw,
    Route,
    Save,
    Settings2,
    SlidersHorizontal,
    MessageSquare,
    Target,
    Undo2,
    Waves,
    X,
} from 'lucide-vue-next';
import { usePreferencesStore } from './stores/preferences';
import { useAppearance } from './composables/useAppearance';
import { tr } from './i18n';
import { callTool, errorMessage } from './api';
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
import AnalysisNavigator, { type AnalysisView } from './components/AnalysisNavigator.vue';
import ChatPanel from './components/ChatPanel.vue';
import type { BenchTarget } from './stores/chat';
import MetricsBar from './components/MetricsBar.vue';
import SurfaceTable from './components/SurfaceTable.vue';
import SystemEditor from './components/SystemEditor.vue';
import TolerancePanel from './components/TolerancePanel.vue';
import MtfView from './components/MtfView.vue';
import PrescriptionReport from './components/PrescriptionReport.vue';
import RayInspection from './components/RayInspection.vue';
import RecoveryPanel from './components/RecoveryPanel.vue';
import SpotView from './components/SpotView.vue';
import { TABS, type OpticalSystem, type ProjectListResult, type Tab } from './types';

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
/** Prescription stays reachable through the dock and the narrow-screen view select; the grouped navigator drops the redundant entry. */
const navGroups: { key: string; items: WorkbenchView[] }[] = [
    { key: 'inspect', items: ['Layout', 'Spot diagram', 'MTF', 'Analysis', 'Ray inspection'] },
    { key: 'design', items: ['Edit', 'Optimize', 'Tolerance', 'Configurations'] },
    {
        key: 'project',
        items: ['Prescription report', 'Materials', 'History', 'Import / Export'],
    },
];
const NAV_ICONS: Partial<Record<WorkbenchView, unknown>> = {
    Layout: Route,
    'Spot diagram': Target,
    MTF: Activity,
    Analysis: Gauge,
    'Ray inspection': Crosshair,
    Edit: PencilRuler,
    Optimize: SlidersHorizontal,
    Tolerance: CircleDot,
    Configurations: Layers,
    'Prescription report': FileText,
    Materials: Boxes,
    History: History,
    'Import / Export': ArrowLeftRight,
};
const wantMtf = computed(() => tab.value === 'MTF');
const wb = useWorkbench(wantMtf);
/**
 * Draft edits from the shell (dock, configuration quick switch) go straight into the shared M6
 * draft. Live Sync commits them through useWorkbench's own debounce; with Live Sync off they wait
 * for Apply. The shell never runs a second commit path of its own.
 */
function setDraft(next: OpticalSystem): void {
    wb.draft.value = next;
}
/** Live Sync is owned by useWorkbench (M6); the shell only persists the user's choice across sessions. */
const liveSyncStored = useLocalStorage('hissabeth.liveSync', false);
wb.liveSync.value = liveSyncStored.value;
const liveSync = computed<boolean>({
    get: () => wb.liveSync.value,
    set: (value) => {
        wb.liveSync.value = value;
        liveSyncStored.value = value;
    },
});

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

// ---- sequential / laser-bench mode + unified save --------------------------------
type OpticalMode = 'section' | '2d' | '3d';
const opticalMode = ref<OpticalMode>('section');
/** The first loaded system picks the sensible default (bench projects open in bench mode); after that the user owns the choice. */
let modeInitialized = false;
watch(
    () => !!wb.system.value?.bench,
    (hasBench) => {
        if (modeInitialized || !wb.system.value) return;
        opticalMode.value = hasBench ? '2d' : 'section';
        modeInitialized = true;
    },
    { immediate: true },
);
const effectiveMode = computed<OpticalMode>({
    get: () => (wb.system.value?.bench ? opticalMode.value : 'section'),
    set: (mode) => {
        opticalMode.value = mode;
    },
});
function setMode(mode: OpticalMode): void {
    opticalMode.value = mode;
}
const layoutRef = ref<InstanceType<typeof OpticalLayoutView> | null>(null);
/** Bench save/discard are exposed by the layout view (parallel contract); absent until that lands. */
type LayoutApi = InstanceType<typeof OpticalLayoutView> & {
    saveBench?: () => Promise<unknown>;
    discardBench?: () => void;
    locate?: (target: BenchTarget) => Promise<boolean>;
};
const layoutApi = computed(() => (layoutRef.value ?? null) as LayoutApi | null);
/** A chat review card's "locate on bench": open the bench and show its target. */
async function locateOnBench(target: BenchTarget): Promise<void> {
    tab.value = 'Layout';
    await nextTick();
    await layoutApi.value?.locate?.(target);
}
/** Bench-side unsaved state, notified by the layout view through the v-model:bench-dirty contract. */
const benchDirty = ref(false);
const canSaveBench = computed(
    () =>
        benchDirty.value &&
        wb.online.value &&
        !wb.recovery.value &&
        !wb.busy.value &&
        !wb.stale.value &&
        !wb.refreshPending.value &&
        !wb.dirty.value,
);
const hasUnsaved = computed(() => wb.dirty.value || benchDirty.value);
async function unifiedSave(): Promise<void> {
    if (effectiveMode.value !== 'section' && benchDirty.value && layoutApi.value?.saveBench) {
        await layoutApi.value.saveBench();
        return;
    }
    if (canSave.value) await wb.save();
}
const canApplyAll = computed(
    () =>
        canSave.value ||
        (effectiveMode.value !== 'section' && canSaveBench.value && !!layoutApi.value?.saveBench),
);

// ---- active configuration quick switch (applies immediately through the draft) ----
const configurations = computed(() => wb.draft.value?.configurations ?? []);
const activeConfiguration = computed(() => wb.draft.value?.active_configuration ?? 0);
function switchConfiguration(index: number): void {
    const draft = wb.draft.value;
    if (!draft || !draft.configurations?.[index] || index === activeConfiguration.value) return;
    // A clean draft switches immediately; pending edits keep the switch in the draft so Apply (or
    // Live Sync) commits everything together instead of silently pushing unrelated changes.
    const wasClean = !wb.dirty.value;
    setDraft({ ...draft, active_configuration: index });
    if (wasClean) void wb.applyDraft();
}

// ---- project menu (recent / new / native dialogs with path fallback) ----
const projects = ref<ProjectListResult | null>(null);
const projectMenuOpen = ref(false);
const projectPath = ref('');
const projectMenuError = ref('');
const projectMenuFallback = ref(false);
const projectBusy = ref(false);
const pathInput = ref<HTMLInputElement>();
async function refreshProjects(): Promise<void> {
    if (!wb.online.value || wb.recovery.value) return;
    try {
        projects.value = await callTool('project_list', {});
        projectPath.value ||= projects.value.active_file ?? '';
    } catch {
        // keep the cached list; the menu stays usable
    }
}
watch(projectMenuOpen, async (open) => {
    if (!open) {
        projectMenuError.value = '';
        projectMenuFallback.value = false;
        return;
    }
    await refreshProjects();
});
type ProjectCommand = 'project_open' | 'project_save_as';
const projectPicking = ref(false);
const canUseProject = computed(
    () => canQuickAction.value && !benchDirty.value && !projectBusy.value && !projectPicking.value,
);
function showProjectError(message: string): void {
    projectMenuError.value = message;
    projectMenuOpen.value = true;
}
async function runProjectPath(
    command: ProjectCommand,
    path: string,
    revision = wb.revision.value,
): Promise<void> {
    const trimmed = path.trim();
    if (!trimmed || !canUseProject.value) return;
    projectBusy.value = true;
    projectMenuError.value = '';
    try {
        const ok = await wb.mutate((current) => {
            if (current !== revision) throw new Error(tr('proj.pickerStale'));
            return callTool(command, { path: trimmed, revision });
        });
        if (ok) projectMenuOpen.value = false;
        else
            showProjectError(
                tr(command === 'project_open' ? 'proj.openFailed' : 'proj.saveFailed', {
                    reason: wb.actionError.value,
                }),
            );
    } finally {
        projectBusy.value = false;
    }
}
async function browseProject(command: ProjectCommand): Promise<void> {
    if (!canUseProject.value) return;
    const revision = wb.revision.value;
    projectPicking.value = true;
    let path: string | null;
    try {
        path =
            command === 'project_open'
                ? await openFileDialog({
                      title: tr('proj.openDialog'),
                      filters: [{ name: tr('proj.openFilter'), extensions: ['json', 'zmx'] }],
                  })
                : await saveFileDialog({
                      title: tr('proj.saveAsDialog'),
                      defaultPath: /\.zmx$/i.test(activeFileName.value)
                          ? undefined
                          : activeFileName.value || undefined,
                      filters: [{ name: tr('proj.saveFilter'), extensions: ['json'] }],
                  });
    } catch {
        await showPathFallback();
        return;
    } finally {
        projectPicking.value = false;
    }
    if (!path) return;
    if (revision !== wb.revision.value || !canUseProject.value) {
        showProjectError(tr('proj.pickerStale'));
        return;
    }
    await runProjectPath(command, path, revision);
}
/** The dialog plugin is unavailable (browser or unregistered): reopen the menu on the path fallback. */
async function showPathFallback(): Promise<void> {
    projectMenuFallback.value = true;
    projectMenuOpen.value = true;
    await nextTick();
    pathInput.value?.focus();
}

async function openSettings(section: unknown = 'general'): Promise<void> {
    try {
        await invoke('open_settings', { section: section === 'agents' ? 'agents' : 'general' });
        settingsError.value = '';
    } catch (error) {
        settingsError.value = errorMessage(error);
    }
}
async function openGlassCatalogs(): Promise<void> {
    try {
        await invoke('open_glass_catalogs');
        settingsError.value = '';
    } catch (error) {
        settingsError.value = errorMessage(error);
    }
}
/** Matches `material::MISSING_CATALOG` errors from the engine. */
const missingCatalog = computed(() =>
    [wb.actionError.value, wb.analysisError.value, ...wb.problems.value].some((text) =>
        text?.includes('glass catalog, which is not installed'),
    ),
);
function activate(view: Tab): void {
    tab.value = view;
    navDrawer.value = false;
}
const exchange = ref<InstanceType<typeof ImportExportPanel>>();
/** New workspace reuses the existing guarded project_new flow (two-step confirmation in Import / Export). */
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
            if (event.shiftKey) void browseProject('project_save_as');
            else void unifiedSave();
        } else if (event.key.toLowerCase() === 'o') {
            event.preventDefault();
            void browseProject('project_open');
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
/** Spot / MTF / Analysis share the merged analysis sub-navigation. */
const ANALYSIS_VIEWS: readonly AnalysisView[] = ['Spot diagram', 'MTF', 'Analysis'];
const analysisView = computed<AnalysisView | null>(() =>
    (ANALYSIS_VIEWS as readonly string[]).includes(tab.value) ? (tab.value as AnalysisView) : null,
);
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
                                ><MenubarItem :disabled="!canApplyAll" @select="unifiedSave()"
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
                                ><MenubarItem @select="openGlassCatalogs">{{
                                    tr('menu.glassCatalogs')
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
                <div
                    v-if="wb.system.value?.bench"
                    class="mode-switch"
                    role="group"
                    :aria-label="tr('app.modeTip')"
                >
                    <ToolbarButton
                        :aria-pressed="effectiveMode === 'section'"
                        :title="tr('app.modeTip')"
                        @click="setMode('section')"
                        ><ListOrdered :size="14" /><span class="tool-label">{{
                            tr('app.modeSequential')
                        }}</span></ToolbarButton
                    >
                    <ToolbarButton
                        :aria-pressed="effectiveMode !== 'section'"
                        :title="tr('app.modeTip')"
                        @click="setMode(effectiveMode === '3d' ? '3d' : '2d')"
                        ><Waves :size="14" /><span class="tool-label">{{
                            tr('app.modeBench')
                        }}</span></ToolbarButton
                    >
                </div>
                <ToolbarSeparator v-if="wb.system.value?.bench" />
                <ToolbarButton
                    :disabled="!canApplyAll"
                    :title="tr('app.applyChanges')"
                    :aria-label="tr('app.applyChanges')"
                    @click="unifiedSave()"
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
                <ToolbarSeparator
                    v-if="configurations.length || narrow || !p.workspace.showNavigator"
                />
                <select
                    v-if="configurations.length"
                    class="config-quick"
                    :aria-label="tr('app.configQuick')"
                    :value="activeConfiguration"
                    @change="
                        switchConfiguration(Number(($event.target as HTMLSelectElement).value))
                    "
                >
                    <option
                        v-for="(cfg, i) in configurations"
                        :key="i"
                        :value="i"
                        :selected="i === activeConfiguration"
                    >
                        {{ cfg.name || tr('cfg.defaultName', { n: i + 1 }) }}
                    </option>
                </select>
                <ToolbarButton
                    :aria-pressed="liveSync"
                    :title="tr('app.liveSyncTip')"
                    :aria-label="tr('app.liveSync')"
                    @click="liveSync = !liveSync"
                    ><Radio :size="16" /><span class="tool-label">{{
                        tr('app.liveSync')
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
                    <DropdownMenuRoot v-model:open="projectMenuOpen"
                        ><DropdownMenuTrigger as-child
                            ><button
                                type="button"
                                class="file-pill project-menu-trigger"
                                :title="wb.activeProjectPath.value ?? tr('app.projectMenu')"
                                :aria-label="
                                    tr('proj.menuTrigger', {
                                        name: activeFileName || tr('app.noSystem'),
                                    })
                                "
                            >
                                <FolderOpen :size="12" /><span class="file-pill-name">{{
                                    activeFileName || tr('app.projectMenu')
                                }}</span>
                            </button></DropdownMenuTrigger
                        ><DropdownMenuPortal
                            ><DropdownMenuContent
                                class="menu-popup project-menu"
                                :side-offset="4"
                                align="start"
                                ><DropdownMenuLabel class="menu-label">{{
                                    tr('app.projectMenu')
                                }}</DropdownMenuLabel
                                ><DropdownMenuItem
                                    class="menu-item"
                                    :disabled="!canUseProject"
                                    @select="browseProject('project_open')"
                                    >{{ tr('proj.openDialog') }}<kbd>Ctrl+O</kbd></DropdownMenuItem
                                ><DropdownMenuItem
                                    class="menu-item"
                                    :disabled="!canUseProject"
                                    @select="browseProject('project_save_as')"
                                    >{{ tr('proj.saveAsDialog')
                                    }}<kbd>Ctrl+Shift+S</kbd></DropdownMenuItem
                                ><DropdownMenuItem class="menu-item" @select="newSystem()">{{
                                    tr('proj.newWorkspace')
                                }}</DropdownMenuItem
                                ><DropdownMenuSeparator />
                                <DropdownMenuLabel class="menu-label">{{
                                    tr('proj.recent')
                                }}</DropdownMenuLabel
                                ><template v-if="projects?.recent.length">
                                    <DropdownMenuItem
                                        v-for="path in projects.recent"
                                        :key="path"
                                        class="menu-item recent-item"
                                        :disabled="!canUseProject"
                                        :title="path"
                                        @select="runProjectPath('project_open', path)"
                                        >{{ path.split(/[\\/]/).pop() ?? path }}</DropdownMenuItem
                                    >
                                </template>
                                <DropdownMenuItem v-else class="menu-item" :disabled="true">{{
                                    tr('proj.noRecent')
                                }}</DropdownMenuItem>
                                <DropdownMenuSeparator />
                                <div class="project-path-fallback">
                                    <p v-if="projectMenuFallback" class="menu-note">
                                        {{ tr('proj.dialogUnavailable') }}
                                    </p>
                                    <label class="menu-label" for="project-path">{{
                                        tr('proj.pathLabel')
                                    }}</label>
                                    <div class="project-path-row">
                                        <input
                                            id="project-path"
                                            ref="pathInput"
                                            v-model="projectPath"
                                            type="text"
                                            :placeholder="tr('proj.pathPlaceholder')"
                                            @keydown.enter.prevent="
                                                runProjectPath('project_open', projectPath)
                                            "
                                        />
                                        <button
                                            type="button"
                                            :disabled="!projectPath.trim() || !canUseProject"
                                            @click="runProjectPath('project_open', projectPath)"
                                        >
                                            {{ tr('proj.openPath') }}
                                        </button>
                                        <button
                                            type="button"
                                            :disabled="!projectPath.trim() || !canUseProject"
                                            @click="runProjectPath('project_save_as', projectPath)"
                                        >
                                            {{ tr('proj.savePath') }}
                                        </button>
                                    </div>
                                    <p v-if="projectMenuError" class="menu-error" role="alert">
                                        {{ projectMenuError }}
                                    </p>
                                </div></DropdownMenuContent
                            ></DropdownMenuPortal
                        ></DropdownMenuRoot
                    >
                    <span
                        v-if="hasUnsaved"
                        class="unsaved"
                        :title="
                            wb.dirty.value && benchDirty
                                ? tr('app.unsaved') + ' · ' + tr('app.benchUnsaved')
                                : wb.dirty.value
                                  ? tr('app.unsaved')
                                  : tr('app.benchUnsaved')
                        "
                        >●
                        {{
                            wb.dirty.value
                                ? benchDirty
                                    ? tr('app.unsaved') + ' + ' + tr('app.benchUnsaved')
                                    : tr('app.unsaved')
                                : tr('app.benchUnsaved')
                        }}</span
                    >
                    <span class="doc-rev">{{ tr('app.revision', { n: wb.revision.value }) }}</span>
                </div>
                <MetricsBar
                    v-if="p.workspace.showMetrics && wb.paraxial.value && !narrow && !showMetrics"
                    compact
                    class="toolbar-metrics"
                    :paraxial="wb.paraxial.value"
                    :spot="wb.spot.value"
                />
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
                                    <component
                                        :is="NAV_ICONS[view]"
                                        v-if="NAV_ICONS[view]"
                                        :size="14"
                                        aria-hidden="true"
                                    />
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

                                    <div v-if="missingCatalog" role="alert" class="banner warn">
                                        {{ tr('banner.missingCatalog') }}
                                        <button type="button" @click="openGlassCatalogs">
                                            {{ tr('banner.openGlassCatalogs') }}
                                        </button>
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
                                            ref="layoutRef"
                                            v-show="tab === 'Layout'"
                                            v-model:mode="effectiveMode"
                                            v-model:bench-dirty="benchDirty"
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
                                            :paraxial="wb.paraxial.value"
                                            :chat-open="chatOpen"
                                            @chat="chatOpen = true"
                                            @scope="layoutSection = $event"
                                        />
                                        <AnalysisNavigator
                                            v-if="analysisView"
                                            :active="analysisView"
                                            @navigate="tab = $event"
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
                                            :saved-to-draft="wb.savedToDraftSurfaceIndexes.value"
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
                                <PrescriptionView
                                    :system="wb.system.value!"
                                    :model-value="wb.draft.value"
                                    compact
                                    editable
                                    :materials="wb.materials.value"
                                    :variables="wb.optimizationVariables.value ?? undefined"
                                    @update:model-value="setDraft"
                                    @toggle-variable="wb.toggleOptimizationVariable"
                                /></section
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
                        @locate="locateOnBench"
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
                                    <component
                                        :is="NAV_ICONS[view]"
                                        v-if="NAV_ICONS[view]"
                                        :size="14"
                                        aria-hidden="true"
                                    />
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
