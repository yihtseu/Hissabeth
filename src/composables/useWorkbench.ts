import { tr } from '../i18n';
import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { callTool, errorMessage, inDesktop } from '../api';
import type {
    GeometricMtf,
    ImportFormat,
    Layout,
    MtfAxis,
    OpticalSystem,
    OptimizeVariable,
    Paraxial,
    PupilSampling,
    RecoveryWorkspace,
    RunInfo,
    SpotDiagram,
    Spectrum,
    SurfaceEditResult,
    SystemSnapshot,
} from '../types';
import { type SpectralMode } from '../sampling';
import { usePreferencesStore } from '../stores/preferences';
import { samplingPreset } from '../preferences';
import { FALLBACK_COLOR } from '../color';
import { checkRevision, savedSystemChanged } from '../provenance';
import { canonicalJson, clone, fieldLabel, validateSystem } from '../validation';
import {
    hoveredSurfaceIndex,
    selectedSurfaceIndex,
    syncSurfaceSelection,
} from './useSurfaceSelection';

const FALLBACK_MATERIALS = ['AIR'];
const DEFAULT_WAVELENGTH = 587.5618;

/** Stable identity for an optimization variable (kind + addressing fields). */
function variableKey(v: OptimizeVariable): string {
    const parts: (string | number)[] = [v.kind, v.surface];
    if ('index' in v) parts.push(v.index);
    if ('axis' in v) parts.push(v.axis);
    if ('configuration' in v) parts.push(v.configuration);
    return parts.join(':');
}

export function useWorkbench(wantMtf: Ref<boolean>) {
    const preferences = usePreferencesStore();
    const online = ref(false);
    const offlineReason = ref('');
    const system = ref<OpticalSystem | null>(null);
    const draft = ref<OpticalSystem | null>(null);
    const revision = ref(0);
    const stale = ref(false);
    /** An operation was confirmed committed but the follow-up snapshot read failed; the view is outdated. */
    const refreshPending = ref(false);
    const busy = ref(false);
    const actionError = ref('');
    const analysisError = ref('');
    const materials = ref<string[]>(FALLBACK_MATERIALS);
    const canUndo = ref(false);
    const canRedo = ref(false);
    const activeProjectPath = ref<string | null>(null);

    /** Recovery state from the last workspace_status poll; null when the workspace is ready. */
    const recovery = ref<RecoveryWorkspace | null>(null);
    /** Repair text and format for Restore from text; never written to the engine until the user acts. */
    const recoveryText = ref('');
    const recoveryFormat = ref<ImportFormat>('hissabeth');
    const recoveryBusy = ref(false);
    const recoveryError = ref('');
    /** Set after a confirmed replacement; kept across refresh failures until dismissed. */
    const recoveryNotice = ref<{ path: string | null } | null>(null);
    /** True after a confirmed retry/replacement until a fresh snapshot is read and applied or flagged stale. */
    const recoveryPending = ref(false);
    /** Live Sync: on = a valid draft commits automatically shortly after edits settle; off = manual Apply. */
    const liveSync = ref(false);
    /** Shared optimization variable list: the surface table toggles entries, the optimize panel edits bounds. */
    const optimizationVariables = ref<OptimizeVariable[] | null>(null);
    const savedSurfaceKeys = ref<number[]>([]);
    const draftSurfaceKeys = ref<number[]>([]);
    const expandedSurfaceDetails = ref<ReadonlyMap<number, boolean>>(new Map());
    let nextSurfaceKey = 0;
    // The write and the snapshot read are separate. Promote these keys when its revision is read.
    let savedDraftStructure: { revision: number; keys: number[] } | null = null;
    const savedToDraftSurfaceIndexes = computed(() =>
        savedSurfaceKeys.value.map((key) => {
            const index = draftSurfaceKeys.value.indexOf(key);
            return index < 0 ? null : index;
        }),
    );
    const draftStructureChanged = computed(
        () =>
            draftSurfaceKeys.value.length !== savedSurfaceKeys.value.length ||
            savedToDraftSurfaceIndexes.value.some((index, previous) => index !== previous),
    );
    /** Last server-observed source text; the repair text is re-seeded only while it still matches. */
    let recoverySeed: string | null = null;
    /** Draft text captured when the current recovery action started; null outside one. */
    let recoveryDraftAtRequest: string | null = null;
    /** Bumped when a recovery action starts; an in-flight poll holding an older value discards its replies. */
    let recoveryEpoch = 0;
    /** True while a confirmed recovery must not let the poll adopt over the kept draft. */
    let recoveryKeepsDraft = false;

    /** Stored field number and wavelength number (indices into the saved system). */
    const fieldIndex = ref(0);
    const wavelengthIndex = ref(0);
    const mtfAxis = ref<MtfAxis>('y');
    const mtfMaxFrequency = ref(100);
    /** Shared ray sampling; replaced wholesale on user action only, never reset by revisions. */
    const spotSampling = ref<PupilSampling>(samplingPreset(preferences.preferences.analysis));
    const mtfSampling = ref<PupilSampling>(samplingPreset(preferences.preferences.analysis));
    const mtfSpectralMode = ref<SpectralMode>('mono');
    watch(
        () => ({ ...preferences.preferences.analysis }),
        (next, previous) => {
            const oldPreset = samplingPreset(previous);
            for (const sampling of [spotSampling, mtfSampling]) {
                if (canonicalJson(sampling.value) === canonicalJson(oldPreset))
                    sampling.value = samplingPreset(next);
            }
        },
    );

    const paraxial = ref<Paraxial | null>(null);
    const layout = ref<Layout>({ rays: [], field_indices: [], rejected: [], revision: -1 });
    const spot = ref<SpotDiagram | null>(null);
    const mtf = ref<GeometricMtf | null>(null);
    /** Request provenance captured when the call started; null whenever the result is cleared. */
    const spotRun = ref<RunInfo | null>(null);
    const mtfRun = ref<RunInfo | null>(null);
    const spotPending = ref(false);
    const mtfPending = ref(false);

    const wavelength = computed(
        () => system.value?.wavelengths[wavelengthIndex.value]?.value ?? DEFAULT_WAVELENGTH,
    );
    /** Wavelength the backend layout() always traces: the saved system's reference wavelength (matches Ray.wavelength). */
    const layoutWavelength = computed(
        () =>
            system.value?.wavelengths[system.value.reference_wavelength]?.value ??
            DEFAULT_WAVELENGTH,
    );
    const dirty = computed(
        () =>
            !!system.value &&
            !!draft.value &&
            canonicalJson(system.value) !== canonicalJson(draft.value),
    );
    const problems = computed(() => (draft.value ? validateSystem(draft.value) : []));

    let timer: ReturnType<typeof setInterval> | undefined;
    let unlistenWorkspace: UnlistenFn | undefined;
    let polling = false;
    let materialsLoaded = false;
    /** Serialized draft captured when a request starts; the poll may adopt over the draft only while it still matches. */
    let pendingDraftText: string | null = null;
    let paraxialToken = 0;
    let layoutToken = 0;
    let spotToken = 0;
    let mtfToken = 0;

    async function refreshWorkspaceMeta(): Promise<void> {
        if (recovery.value || !online.value) return;
        const rev = revision.value;
        const [hist, proj] = await Promise.allSettled([
            callTool('history_list', {}),
            callTool('project_list', {}),
        ]);
        if (rev !== revision.value || recovery.value) return;
        if (hist.status === 'fulfilled') {
            canUndo.value = hist.value.can_undo;
            canRedo.value = hist.value.can_redo;
        }
        if (proj.status === 'fulfilled') {
            activeProjectPath.value = proj.value.active_file;
        }
    }

    // Keep index-based state on the same physical rows as the draft changes.
    function setDraftSurfaceKeys(keys: number[]): void {
        if (keys === draftSurfaceKeys.value) return;
        const indexes = new Map(keys.map((key, index) => [key, index]));
        const previous = draftSurfaceKeys.value;
        const remap = (index: number | null): number | null =>
            index === null ? null : (indexes.get(previous[index]!) ?? null);
        selectedSurfaceIndex.value = remap(selectedSurfaceIndex.value);
        hoveredSurfaceIndex.value = remap(hoveredSurfaceIndex.value);
        syncSurfaceSelection(keys.length);
        if (optimizationVariables.value) {
            optimizationVariables.value = optimizationVariables.value.flatMap((variable) => {
                const surface = remap(variable.surface);
                return surface === null ? [] : [{ ...variable, surface }];
            });
        }
        expandedSurfaceDetails.value = new Map(
            [...expandedSurfaceDetails.value].filter(([key]) => indexes.has(key)),
        );
        draftSurfaceKeys.value = keys;
    }

    function applySurfaceEdit(result: SurfaceEditResult): void {
        const keys = Array<number>(result.system.surfaces.length);
        result.old_to_new.forEach((index, previous) => {
            if (index !== null) keys[index] = draftSurfaceKeys.value[previous]!;
        });
        for (const index of result.inserted) keys[index] = nextSurfaceKey++;
        setDraftSurfaceKeys(keys);
        draft.value = result.system;
    }

    function adopt(snapshot: SystemSnapshot): void {
        if (snapshot.revision !== revision.value) materialsLoaded = false; // custom glasses may have changed
        const previous = system.value;
        const selectedValue = previous?.wavelengths[wavelengthIndex.value]?.value;
        const count = snapshot.system.surfaces.length;
        if (savedDraftStructure?.revision === snapshot.revision) {
            savedSurfaceKeys.value = savedDraftStructure.keys;
        } else if (savedSurfaceKeys.value.length !== count) {
            savedSurfaceKeys.value = Array.from({ length: count }, () => nextSurfaceKey++);
        }
        savedDraftStructure = null;
        setDraftSurfaceKeys(savedSurfaceKeys.value);
        system.value = clone(snapshot.system);
        draft.value = clone(snapshot.system);
        revision.value = snapshot.revision;
        stale.value = false;
        refreshPending.value = false;
        pendingDraftText = null;
        recoveryKeepsDraft = false;
        recoveryDraftAtRequest = null;
        const { fields, wavelengths, reference_wavelength } = snapshot.system;
        if (fieldIndex.value >= fields.length) fieldIndex.value = 0;
        // First adoption selects the reference wavelength; later edits keep the selected value if it still exists.
        if (selectedValue === undefined) {
            wavelengthIndex.value = reference_wavelength;
        } else if (wavelengths[wavelengthIndex.value]?.value !== selectedValue) {
            const kept = wavelengths.findIndex((w) => w.value === selectedValue);
            wavelengthIndex.value = kept >= 0 ? kept : reference_wavelength;
        }
        if (wavelengthIndex.value >= wavelengths.length) wavelengthIndex.value = 0;
        void loadMaterials();
        void refreshWorkspaceMeta();
    }

    async function loadMaterials(): Promise<void> {
        if (materialsLoaded || recovery.value) return;
        const epoch = recoveryEpoch;
        const rev = revision.value;
        try {
            const list = await callTool('materials_list', {});
            // Drop the reply when recovery began, a recovery action ran, or the saved system changed meanwhile.
            if (recovery.value || epoch !== recoveryEpoch || rev !== revision.value) return;
            materials.value = list;
            materialsLoaded = true;
        } catch {
            // keep the current list; retried on the next successful poll
        }
    }

    /** Read the engine and replace local state, discarding any draft. */
    async function reload(): Promise<void> {
        if (recovery.value) return; // system_get is blocked while the workspace is in recovery
        adopt(await callTool('system_get', {}));
        online.value = true;
        offlineReason.value = '';
    }

    /** Store a recovery status; the repair text follows the observed source only while unedited. */
    function applyRecoveryStatus(status: RecoveryWorkspace): void {
        const prev = recovery.value;
        const changed =
            !prev ||
            prev.revision !== status.revision ||
            prev.source_text !== status.source_text ||
            prev.reason !== status.reason;
        if (!changed) return;
        recovery.value = status;
        if (!prev || recoveryText.value === recoverySeed) {
            recoverySeed = status.source_text ?? '';
            recoveryText.value = recoverySeed;
        } else {
            // The user is repairing; keep their text and only move the last observed source.
            recoverySeed = status.source_text ?? '';
        }
    }

    /**
     * Leaving recovery becomes final only with a fresh snapshot in hand: adopt it when no draft is
     * pending and none was edited since the action started, otherwise keep the draft and flag stale.
     */
    function settleRecoverySnapshot(snapshot: SystemSnapshot, draftAtRequest: string): void {
        recoveryPending.value = false;
        recoveryDraftAtRequest = null;
        recovery.value = null;
        recoverySeed = null;
        recoveryText.value = '';
        if (!system.value || (!dirty.value && canonicalJson(draft.value) === draftAtRequest)) {
            adopt(snapshot);
        } else {
            refreshPending.value = false; // the read succeeded; the kept draft is handled by the stale banner
            stale.value = true;
        }
    }

    /**
     * The engine confirmed the workspace is ready. The recovery view stays up, disabled, until a fresh
     * snapshot is read; a failed read leaves recoveryPending and refreshPending set so the poll finishes
     * the transition, and the confirmed action is never repeated. A kept draft is never adopted over:
     * pendingDraftText stays null so the poll's unchanged-draft replay cannot apply to it.
     */
    async function finishRecovery(
        preserved: string | null | undefined,
        draftAtRequest: string,
    ): Promise<void> {
        recoveryError.value = '';
        recoveryKeepsDraft = true;
        recoveryPending.value = true;
        refreshPending.value = true;
        if (preserved !== undefined) recoveryNotice.value = { path: preserved };
        try {
            const snapshot = await callTool('system_get', {});
            settleRecoverySnapshot(snapshot, draftAtRequest);
        } catch {
            pendingDraftText = null;
        }
    }

    /** Recovery actions are blocked while another write runs, once a result is confirmed but unread, or while offline. */
    function recoveryBlocked(): boolean {
        return (
            !recovery.value ||
            recoveryPending.value ||
            recoveryBusy.value ||
            busy.value ||
            !online.value
        );
    }

    async function retryFromDisk(): Promise<void> {
        const rec = recovery.value;
        if (!rec || recoveryBlocked()) return;
        recoveryEpoch++;
        recoveryBusy.value = true;
        recoveryError.value = '';
        const draftAtRequest = canonicalJson(draft.value);
        recoveryDraftAtRequest = draftAtRequest;
        try {
            const status = await callTool('workspace_retry', { revision: rec.revision });
            if (status.state === 'recovery') {
                applyRecoveryStatus(status);
                recoveryError.value = status.reason;
            } else {
                await finishRecovery(undefined, draftAtRequest);
            }
        } catch (e) {
            // Not confirmed: the retry may still have loaded; the next poll discovers the state.
            recoveryError.value = errorMessage(e);
        } finally {
            recoveryBusy.value = false;
        }
    }

    async function restoreFromText(): Promise<void> {
        const rec = recovery.value;
        const text = recoveryText.value;
        if (!rec || recoveryBlocked()) return;
        if (!text.trim()) {
            recoveryError.value = tr('workbench.m0');
            return;
        }
        recoveryEpoch++;
        recoveryBusy.value = true;
        recoveryError.value = '';
        const draftAtRequest = canonicalJson(draft.value);
        recoveryDraftAtRequest = draftAtRequest;
        try {
            const r = await callTool('workspace_recover', {
                revision: rec.revision,
                source: { kind: 'project', format: recoveryFormat.value, text },
            });
            await finishRecovery(r.preserved_path, draftAtRequest);
        } catch (e) {
            recoveryError.value = errorMessage(e);
        } finally {
            recoveryBusy.value = false;
        }
    }

    async function createNewSystem(): Promise<void> {
        const rec = recovery.value;
        if (!rec || recoveryBlocked()) return;
        recoveryEpoch++;
        recoveryBusy.value = true;
        recoveryError.value = '';
        const draftAtRequest = canonicalJson(draft.value);
        recoveryDraftAtRequest = draftAtRequest;
        try {
            const r = await callTool('workspace_recover', {
                revision: rec.revision,
                source: { kind: 'default' },
            });
            await finishRecovery(r.preserved_path, draftAtRequest);
        } catch (e) {
            recoveryError.value = errorMessage(e);
        } finally {
            recoveryBusy.value = false;
        }
    }

    function dismissRecoveryNotice(): void {
        recoveryNotice.value = null;
    }

    async function poll(): Promise<void> {
        if (polling || busy.value || recoveryBusy.value) return;
        polling = true;
        const epoch = recoveryEpoch;
        let statusOk = false;
        try {
            const status = await callTool('workspace_status', {});
            if (epoch !== recoveryEpoch) return; // a recovery action started meanwhile; its reply owns the state
            statusOk = true;
            online.value = true;
            offlineReason.value = '';
            if (status.state === 'recovery') {
                recoveryNotice.value = null; // a stale notice belongs to the previous ready period
                if (recoveryPending.value) {
                    // A new invalid file superseded the confirmed write before the view refreshed.
                    recoveryPending.value = false;
                    refreshPending.value = false;
                }
                applyRecoveryStatus(status);
                return; // every optical tool is blocked; nothing else may be read
            }
            // Fast path: skip heavy system_get serialization when revision is unchanged and already synced.
            if (
                !recovery.value &&
                !refreshPending.value &&
                system.value !== null &&
                status.revision === revision.value
            ) {
                void loadMaterials();
                return;
            }
            const snapshot = await callTool('system_get', {});
            if (epoch !== recoveryEpoch) return;
            if (recovery.value) {
                // The workspace answered ready while recovery was still shown (confirmed or found fixed).
                settleRecoverySnapshot(
                    snapshot,
                    recoveryDraftAtRequest ?? canonicalJson(draft.value),
                );
                return;
            }
            void loadMaterials();
            // After a confirmed commit whose refresh failed, adopt even at an unchanged revision number.
            const changed = snapshot.revision !== revision.value || refreshPending.value;
            const draftUnchanged =
                refreshPending.value &&
                !recoveryKeepsDraft &&
                pendingDraftText !== null &&
                canonicalJson(draft.value) === pendingDraftText;
            if (!system.value || (changed && (!dirty.value || draftUnchanged))) {
                adopt(snapshot);
            } else if (changed) {
                stale.value = true; // an agent edited the system (or our write completed) while a local draft is pending
            }
        } catch (e) {
            if (epoch !== recoveryEpoch) return; // a recovery action started meanwhile; its reply owns the state
            if (statusOk && (recovery.value || recoveryPending.value)) {
                // The status read succeeded; only the snapshot read failed. Stay connected and keep the pending view.
            } else {
                online.value = false;
                offlineReason.value = errorMessage(e);
            }
        } finally {
            polling = false;
        }
    }

    /** Per-source error slots so one analysis cannot erase another's message. */
    const errorSlots = new Map<string, string>();
    function setError(slot: string, message: string): void {
        if (message) errorSlots.set(slot, message);
        else errorSlots.delete(slot);
        analysisError.value = [...errorSlots.values()].join(' · ');
    }

    /** Saved color of the selected wavelength entry, captured at dispatch. */
    const selectedColor = (sys: OpticalSystem) =>
        sys.wavelengths[wavelengthIndex.value]?.color ?? FALLBACK_COLOR;

    async function analyzeParaxial(): Promise<void> {
        const token = ++paraxialToken;
        const captured = revision.value;
        paraxial.value = null;
        setError('paraxial', '');
        if (!online.value || !system.value || recovery.value) return;
        const w = wavelength.value;
        try {
            const result = await callTool('paraxial', { wavelength: w });
            if (token !== paraxialToken || revision.value !== captured) return;
            if (checkRevision(captured, revision.value, result.revision) !== 'ok') {
                setError('paraxial', savedSystemChanged('Paraxial', captured, result.revision));
                return;
            }
            paraxial.value = result;
        } catch (e) {
            if (token === paraxialToken) setError('paraxial', `Paraxial: ${errorMessage(e)}`);
        }
    }

    async function analyzeLayout(): Promise<void> {
        const token = ++layoutToken;
        const captured = revision.value;
        if (layout.value.revision !== captured)
            layout.value = { rays: [], field_indices: [], rejected: [], revision: -1 };
        setError('layout', '');
        if (!online.value || !system.value || recovery.value) {
            layout.value = { rays: [], field_indices: [], rejected: [], revision: -1 };
            return;
        }
        try {
            const result = await callTool('layout', {});
            if (token !== layoutToken || revision.value !== captured) return;
            if (checkRevision(captured, revision.value, result.revision) !== 'ok') {
                layout.value = { rays: [], field_indices: [], rejected: [], revision: -1 };
                setError('layout', savedSystemChanged('Layout', captured, result.revision));
                return;
            }
            layout.value = result;
        } catch (e) {
            if (token === layoutToken) setError('layout', `Layout: ${errorMessage(e)}`);
        }
    }

    /** Drops the previous spot immediately, then requests the current field/wavelength/sampling. */
    async function analyzeSpot(): Promise<void> {
        const token = ++spotToken;
        spot.value = null;
        spotRun.value = null;
        spotPending.value = false;
        setError('spot', '');
        if (!online.value || !system.value || recovery.value) return;
        const run: RunInfo = {
            revision: revision.value,
            field: fieldLabel(system.value, fieldIndex.value),
            wavelength: wavelength.value,
            color: selectedColor(system.value),
            mapping: system.value.pupil_mapping,
        };
        spotPending.value = true;
        try {
            const result = await callTool('spot_diagram', {
                field_index: fieldIndex.value,
                wavelength: run.wavelength,
                sampling: { ...spotSampling.value },
            });
            if (token !== spotToken) return;
            const check = checkRevision(run.revision, revision.value, result.revision);
            if (check === 'superseded') return;
            if (check === 'newer') {
                setError('spot', savedSystemChanged('Spot', run.revision, result.revision));
                return;
            }
            spot.value = result;
            spotRun.value = run;
        } catch (e) {
            if (token === spotToken) setError('spot', `Spot: ${errorMessage(e)}`);
        } finally {
            if (token === spotToken) spotPending.value = false;
        }
    }

    const mtfSpectrum = computed<Spectrum>(() =>
        mtfSpectralMode.value === 'poly'
            ? { kind: 'polychromatic' }
            : { kind: 'monochromatic', wavelength: wavelength.value },
    );

    /** Drops the previous MTF immediately; only the visible MTF tab requests a new one. */
    async function analyzeMtf(): Promise<void> {
        const token = ++mtfToken;
        mtf.value = null;
        mtfRun.value = null;
        mtfPending.value = false;
        setError('mtf', '');
        if (!online.value || !system.value || !wantMtf.value || recovery.value) return;
        const run: RunInfo = {
            revision: revision.value,
            field: fieldLabel(system.value, fieldIndex.value),
            wavelength: wavelength.value,
            color: selectedColor(system.value),
            mapping: system.value.pupil_mapping,
        };
        mtfPending.value = true;
        try {
            const result = await callTool('geometric_mtf', {
                field_index: fieldIndex.value,
                sampling: { ...mtfSampling.value },
                spectrum: { ...mtfSpectrum.value },
                max_frequency: mtfMaxFrequency.value,
                axis: mtfAxis.value,
            });
            if (token !== mtfToken) return;
            const check = checkRevision(run.revision, revision.value, result.revision);
            if (check === 'superseded') return;
            if (check === 'newer') {
                setError('mtf', savedSystemChanged('MTF', run.revision, result.revision));
                return;
            }
            mtf.value = result;
            mtfRun.value = run;
        } catch (e) {
            if (token === mtfToken) setError('mtf', `MTF: ${errorMessage(e)}`);
        } finally {
            if (token === mtfToken) mtfPending.value = false;
        }
    }

    /**
     * Shared path for every operation that writes the saved system. Resolving `op` confirms a commit;
     * rejecting is unconfirmed (the engine may still have committed, e.g. a lost reply), so the poll
     * adopts any revision change. After a confirmed commit a failed or superseded refresh leaves
     * `refreshPending` set instead of reporting failure. A draft edited since entry is never replaced.
     */
    let commitInflight: Promise<boolean> | null = null;
    async function commit(op: () => Promise<unknown>): Promise<boolean> {
        const draftAtRequest = canonicalJson(draft.value);
        busy.value = true;
        const run = (async (): Promise<boolean> => {
            try {
                await op();
            } catch (e) {
                actionError.value = errorMessage(e); // not confirmed: the write may or may not have been applied
                return false;
            }
            try {
                actionError.value = '';
                const snapshot = await callTool('system_get', {});
                if (canonicalJson(draft.value) === draftAtRequest) {
                    adopt(snapshot);
                    online.value = true;
                    offlineReason.value = '';
                } else {
                    // The user edited while the write ran: keep that edit and let the poll flag it for explicit reload.
                    pendingDraftText = draftAtRequest;
                    refreshPending.value = true;
                }
            } catch {
                pendingDraftText = draftAtRequest; // the poll adopts the saved snapshot once the engine answers, if the draft is unchanged
                refreshPending.value = true;
            }
            return true;
        })();
        commitInflight = run;
        try {
            return await run;
        } finally {
            if (commitInflight === run) commitInflight = null;
            busy.value = false;
        }
    }

    async function save(next: OpticalSystem | null = draft.value): Promise<boolean> {
        if (!next || !system.value || !online.value || busy.value || recovery.value) return false;
        if (refreshPending.value) {
            actionError.value = tr('workbench.m1');
            return false;
        }
        if (stale.value) {
            actionError.value = tr('workbench.m2');
            return false;
        }
        const keys = next === draft.value ? draftSurfaceKeys.value : null;
        return commit(async () => {
            const saved = await callTool('system_set', {
                system: clone(next),
                revision: revision.value,
            });
            if (keys) savedDraftStructure = { revision: saved.revision, keys };
        });
    }

    /**
     * The shared auto-apply path: commit the pending draft only when it is valid. Live Sync calls
     * this once edits settle; mutate and autofocus chain it before their target operation so the
     * engine works from the freshest revision. Refused states (offline, stale, refresh pending,
     * recovery, busy, invalid draft) leave the draft untouched and resolve false.
     */
    async function applyDraft(): Promise<boolean> {
        if (!dirty.value) return true;
        if (problems.value.length > 0) return false; // surfaced by the draft problems list
        return save();
    }

    /** Live Sync debounce: one pending auto-commit at a time, re-armed on every relevant change. */
    let liveSyncTimer: ReturnType<typeof setTimeout> | undefined;
    watch(
        [dirty, problems, liveSync, online, stale, refreshPending, recovery, busy],
        () => {
            if (liveSyncTimer !== undefined) {
                clearTimeout(liveSyncTimer);
                liveSyncTimer = undefined;
            }
            if (!liveSync.value || !dirty.value || problems.value.length > 0) return;
            if (!online.value || stale.value || refreshPending.value || recovery.value || busy.value)
                return;
            liveSyncTimer = setTimeout(() => {
                liveSyncTimer = undefined;
                void applyDraft();
            }, 350);
        },
    );

    async function autofocus(): Promise<boolean> {
        if (recovery.value) return false;
        if (commitInflight) await commitInflight; // a Live Sync auto-commit may be finishing; run after it
        if (busy.value || stale.value || refreshPending.value) return false;
        // Chain a valid pending draft first so the focus acts on the freshest prescription.
        if (dirty.value && !(await applyDraft())) {
            if (!actionError.value) actionError.value = tr('workbench.m3');
            return false;
        }
        if (!system.value) return false;
        // The engine rejects the call if this revision is no longer current, so a stale view cannot apply it.
        return commit(() =>
            callTool('autofocus', { wavelength: wavelength.value, revision: revision.value }),
        );
    }

    async function undo(): Promise<boolean> {
        if (!canUndo.value) return false;
        return mutate((rev) => callTool('history_undo', { revision: rev }));
    }

    async function redo(): Promise<boolean> {
        if (!canRedo.value) return false;
        return mutate((rev) => callTool('history_redo', { revision: rev }));
    }

    /**
     * Run a backend operation that rewrites the saved system (import, catalog import, solves,
     * vignetting, optimization, bench edits), then adopt the result. A valid pending draft is
     * chained into a system_set first so it is never lost; an invalid draft refuses the write.
     * Resolves true once the operation is confirmed, even if the refresh afterwards failed.
     */
    async function mutate(
        op: (revision: number, system: OpticalSystem) => Promise<unknown>,
    ): Promise<boolean> {
        if (recovery.value) return false;
        if (commitInflight) await commitInflight; // a Live Sync auto-commit may be finishing; run after it
        if (busy.value) return false;
        if (refreshPending.value) {
            actionError.value = tr('workbench.m1');
            return false;
        }
        if (stale.value) {
            actionError.value = tr('workbench.m2');
            return false;
        }
        if (dirty.value && !(await applyDraft())) {
            if (!actionError.value) actionError.value = tr('workbench.m3');
            return false;
        }
        return commit(() => {
            const sys = system.value;
            if (!sys) throw new Error('No system loaded');
            return op(revision.value, sys);
        });
    }

    /** Add the variable when absent, remove it when present; surface V-badges drive this. */
    function toggleOptimizationVariable(variable: OptimizeVariable): void {
        const key = variableKey(variable);
        const variables = optimizationVariables.value ?? [];
        optimizationVariables.value = variables.some((v) => variableKey(v) === key)
            ? variables.filter((v) => variableKey(v) !== key)
            : [...variables, variable];
    }

    function revert(): void {
        if (system.value) {
            setDraftSurfaceKeys(savedSurfaceKeys.value);
            draft.value = clone(system.value);
        }
        actionError.value = '';
    }

    async function discardAndReload(): Promise<void> {
        try {
            await reload();
            actionError.value = '';
        } catch (e) {
            actionError.value = errorMessage(e);
        }
    }

    const hasSystem = computed(() => system.value !== null);
    watch([revision, wavelengthIndex, hasSystem, online, recovery], () => void analyzeParaxial());
    watch([revision, hasSystem, online, recovery], () => void analyzeLayout());
    watch(
        [revision, fieldIndex, wavelengthIndex, hasSystem, spotSampling, online, recovery],
        () => void analyzeSpot(),
    );
    // The wavelength only matters in monochromatic mode, so weighted results are not discarded when it changes.
    watch(
        [
            revision,
            fieldIndex,
            () => (mtfSpectralMode.value === 'mono' ? wavelengthIndex.value : -1),
            hasSystem,
            mtfAxis,
            mtfMaxFrequency,
            mtfSampling,
            mtfSpectralMode,
            wantMtf,
            online,
            recovery,
        ],
        () => void analyzeMtf(),
    );

    onMounted(async () => {
        if (!inDesktop()) {
            offlineReason.value = tr('workbench.m4');
            return;
        }
        try {
            unlistenWorkspace = await listen<{ revision: number }>('workspace-changed', () => {
                void poll();
            });
        } catch {
            // Event listener optional outside Tauri runtime; polling fallback remains active.
        }
        await poll();
        timer = setInterval(() => void poll(), preferences.preferences.analysis.pollIntervalMs);
    });
    watch(
        () => preferences.preferences.analysis.pollIntervalMs,
        (interval) => {
            if (timer !== undefined) {
                clearInterval(timer);
                timer = setInterval(() => void poll(), interval);
            }
        },
    );
    onBeforeUnmount(() => {
        if (timer !== undefined) clearInterval(timer);
        if (liveSyncTimer !== undefined) clearTimeout(liveSyncTimer);
        unlistenWorkspace?.();
    });

    return {
        online,
        offlineReason,
        system,
        draft,
        revision,
        stale,
        refreshPending,
        busy,
        dirty,
        problems,
        actionError,
        analysisError,
        materials,
        canUndo,
        canRedo,
        activeProjectPath,
        fieldIndex,
        wavelengthIndex,
        wavelength,
        layoutWavelength,
        mtfAxis,
        mtfMaxFrequency,
        spotSampling,
        mtfSampling,
        mtfSpectralMode,
        paraxial,
        layout,
        spot,
        mtf,
        spotRun,
        mtfRun,
        spotPending,
        mtfPending,
        recovery,
        recoveryText,
        recoveryFormat,
        recoveryBusy,
        recoveryPending,
        recoveryError,
        recoveryNotice,
        save,
        applyDraft,
        liveSync,
        optimizationVariables,
        toggleOptimizationVariable,
        draftSurfaceKeys,
        expandedSurfaceDetails,
        savedToDraftSurfaceIndexes,
        draftStructureChanged,
        applySurfaceEdit,
        autofocus,
        undo,
        redo,
        revert,
        discardAndReload,
        mutate,
        retryFromDisk,
        restoreFromText,
        createNewSystem,
        dismissRecoveryNotice,
    };
}
