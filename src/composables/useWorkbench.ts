import { tr } from '../i18n';
import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';
import { callTool, errorMessage, inDesktop } from '../api';
import type {
    GeometricMtf,
    ImportFormat,
    Layout,
    MtfAxis,
    OpticalSystem,
    Paraxial,
    PupilSampling,
    RecoveryWorkspace,
    RunInfo,
    SpotDiagram,
    Spectrum,
    SystemSnapshot,
} from '../types';
import { type SpectralMode } from '../sampling';
import { usePreferencesStore } from '../stores/preferences';
import { samplingPreset } from '../preferences';
import { FALLBACK_COLOR } from '../color';
import { checkRevision, savedSystemChanged } from '../provenance';
import { canonicalJson, clone, fieldLabel, validateSystem } from '../validation';

const FALLBACK_MATERIALS = ['AIR'];
const DEFAULT_WAVELENGTH = 587.5618;

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
    let polling = false;
    let materialsLoaded = false;
    /** Serialized draft captured when a request starts; the poll may adopt over the draft only while it still matches. */
    let pendingDraftText: string | null = null;
    let analysisToken = 0;
    let spotToken = 0;
    let mtfToken = 0;

    function adopt(snapshot: SystemSnapshot): void {
        if (snapshot.revision !== revision.value) materialsLoaded = false; // custom glasses may have changed
        const previous = system.value;
        const selectedValue = previous?.wavelengths[wavelengthIndex.value]?.value;
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

    async function analyze(): Promise<void> {
        const token = ++analysisToken;
        const captured = revision.value;
        // Paraxial depends on the wavelength; the layout only on the revision, so it stays while the revision matches.
        paraxial.value = null;
        if (layout.value.revision !== captured)
            layout.value = { rays: [], field_indices: [], rejected: [], revision: -1 };
        setError('core', '');
        if (!online.value || !system.value || recovery.value) {
            layout.value = { rays: [], field_indices: [], rejected: [], revision: -1 };
            return;
        }
        const w = wavelength.value;
        // Settled independently so one failing analysis (e.g. afocal paraxial) does not hide the rest.
        const [p, l] = await Promise.allSettled([
            callTool('paraxial', { wavelength: w }),
            callTool('layout', {}),
        ]);
        if (token !== analysisToken || revision.value !== captured) return;
        const errors: string[] = [];
        const take = <T extends { revision: number }>(
            r: PromiseSettledResult<T>,
            label: string,
        ): T | null => {
            if (r.status === 'rejected') {
                errors.push(`${label}: ${errorMessage(r.reason)}`);
                return null;
            }
            if (checkRevision(captured, revision.value, r.value.revision) !== 'ok') {
                errors.push(savedSystemChanged(label, captured, r.value.revision));
                return null;
            }
            return r.value;
        };
        paraxial.value = take(p, 'Paraxial');
        const lay = take(l, 'Layout');
        if (lay) layout.value = lay;
        else if (l.status === 'fulfilled')
            layout.value = { rays: [], field_indices: [], rejected: [], revision: -1 }; // reply from another revision
        setError('core', errors.join(' · '));
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
    async function commit(op: () => Promise<unknown>): Promise<boolean> {
        const draftAtRequest = canonicalJson(draft.value);
        busy.value = true;
        try {
            await op();
        } catch (e) {
            actionError.value = errorMessage(e); // not confirmed: the write may or may not have been applied
            busy.value = false;
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
        } finally {
            busy.value = false;
        }
        return true;
    }

    async function save(next: OpticalSystem | null = draft.value): Promise<void> {
        if (!next || busy.value || refreshPending.value || stale.value || recovery.value) return;
        await commit(() =>
            callTool('system_set', { system: clone(next), revision: revision.value }),
        );
    }

    async function autofocus(): Promise<void> {
        if (busy.value || dirty.value || stale.value || refreshPending.value || recovery.value)
            return;
        // The engine rejects the call if this revision is no longer current, so a stale view cannot apply it.
        await commit(() =>
            callTool('autofocus', { wavelength: wavelength.value, revision: revision.value }),
        );
    }

    /**
     * Run a backend operation that rewrites the saved system (import, catalog import),
     * then adopt the result. Refused while a draft is pending so it cannot be lost silently.
     * Resolves true once the operation is confirmed, even if the refresh afterwards failed.
     */
    async function mutate(op: (revision: number) => Promise<unknown>): Promise<boolean> {
        if (busy.value || recovery.value) return false;
        if (refreshPending.value) {
            actionError.value = tr('workbench.m1');
            return false;
        }
        if (stale.value) {
            actionError.value = tr('workbench.m2');
            return false;
        }
        if (dirty.value) {
            actionError.value = tr('workbench.m3');
            return false;
        }
        return commit(() => op(revision.value));
    }

    function revert(): void {
        if (system.value) draft.value = clone(system.value);
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
    watch([revision, fieldIndex, wavelengthIndex, hasSystem, recovery], () => void analyze());
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
    watch(online, () => void analyze());

    onMounted(async () => {
        if (!inDesktop()) {
            offlineReason.value = tr('workbench.m4');
            return;
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
        autofocus,
        revert,
        discardAndReload,
        mutate,
        retryFromDisk,
        restoreFromText,
        createNewSystem,
        dismissRecoveryNotice,
    };
}
