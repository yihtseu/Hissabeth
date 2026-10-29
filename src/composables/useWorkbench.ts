import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';
import { callTool, errorMessage, inDesktop } from '../api';
import type {
  GeometricMtf,
  Layout,
  MtfAxis,
  OpticalSystem,
  Paraxial,
  PupilSampling,
  RunInfo,
  SpotDiagram,
  Spectrum,
  SystemSnapshot,
} from '../types';
import { GEOMETRIC_SAMPLING, SPOT_SAMPLING, type SpectralMode } from '../sampling';
import { FALLBACK_COLOR } from '../color';
import { checkRevision, savedSystemChanged } from '../provenance';
import { canonicalJson, clone, fieldLabel, validateSystem } from '../validation';

const POLL_MS = 1500;
const FALLBACK_MATERIALS = ['AIR'];
const DEFAULT_WAVELENGTH = 587.5618;

export function useWorkbench(wantMtf: Ref<boolean>) {
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

  /** Stored field number and wavelength number (indices into the saved system). */
  const fieldIndex = ref(0);
  const wavelengthIndex = ref(0);
  const mtfAxis = ref<MtfAxis>('y');
  const mtfMaxFrequency = ref(100);
  /** Shared ray sampling; replaced wholesale on user action only, never reset by revisions. */
  const spotSampling = ref<PupilSampling>({ ...SPOT_SAMPLING });
  const mtfSampling = ref<PupilSampling>({ ...GEOMETRIC_SAMPLING });
  const mtfSpectralMode = ref<SpectralMode>('mono');

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
    () => system.value?.wavelengths[system.value.reference_wavelength]?.value ?? DEFAULT_WAVELENGTH,
  );
  const dirty = computed(
    () => !!system.value && !!draft.value && canonicalJson(system.value) !== canonicalJson(draft.value),
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
    if (materialsLoaded) return;
    try {
      materials.value = await callTool('materials_list', {});
      materialsLoaded = true;
    } catch {
      // keep the current list; retried on the next successful poll
    }
  }

  /** Read the engine and replace local state, discarding any draft. */
  async function reload(): Promise<void> {
    adopt(await callTool('system_get', {}));
    online.value = true;
    offlineReason.value = '';
  }

  async function poll(): Promise<void> {
    if (polling || busy.value) return;
    polling = true;
    try {
      const snapshot = await callTool('system_get', {});
      online.value = true;
      offlineReason.value = '';
      void loadMaterials();
      // After a confirmed commit whose refresh failed, adopt even at an unchanged revision number.
      const changed = snapshot.revision !== revision.value || refreshPending.value;
      const draftUnchanged = refreshPending.value && canonicalJson(draft.value) === pendingDraftText;
      if (!system.value || (changed && (!dirty.value || draftUnchanged))) {
        adopt(snapshot);
      } else if (changed) {
        stale.value = true; // an agent edited the system (or our write completed) while a local draft is pending
      }
    } catch (e) {
      online.value = false;
      offlineReason.value = errorMessage(e);
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
  const selectedColor = (sys: OpticalSystem) => sys.wavelengths[wavelengthIndex.value]?.color ?? FALLBACK_COLOR;

  async function analyze(): Promise<void> {
    const token = ++analysisToken;
    const captured = revision.value;
    // Paraxial depends on the wavelength; the layout only on the revision, so it stays while the revision matches.
    paraxial.value = null;
    if (layout.value.revision !== captured) layout.value = { rays: [], field_indices: [], rejected: [], revision: -1 };
    setError('core', '');
    if (!online.value || !system.value) {
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
    const take = <T extends { revision: number }>(r: PromiseSettledResult<T>, label: string): T | null => {
      if (r.status === 'rejected') { errors.push(`${label}: ${errorMessage(r.reason)}`); return null; }
      if (checkRevision(captured, revision.value, r.value.revision) !== 'ok') {
        errors.push(savedSystemChanged(label, captured, r.value.revision));
        return null;
      }
      return r.value;
    };
    paraxial.value = take(p, 'Paraxial');
    const lay = take(l, 'Layout');
    if (lay) layout.value = lay;
    else if (l.status === 'fulfilled') layout.value = { rays: [], field_indices: [], rejected: [], revision: -1 }; // reply from another revision
    setError('core', errors.join(' · '));
  }

  /** Drops the previous spot immediately, then requests the current field/wavelength/sampling. */
  async function analyzeSpot(): Promise<void> {
    const token = ++spotToken;
    spot.value = null;
    spotRun.value = null;
    spotPending.value = false;
    setError('spot', '');
    if (!online.value || !system.value) return;
    const run: RunInfo = { revision: revision.value, field: fieldLabel(system.value, fieldIndex.value), wavelength: wavelength.value, color: selectedColor(system.value), mapping: system.value.pupil_mapping };
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
    if (!online.value || !system.value || !wantMtf.value) return;
    const run: RunInfo = { revision: revision.value, field: fieldLabel(system.value, fieldIndex.value), wavelength: wavelength.value, color: selectedColor(system.value), mapping: system.value.pupil_mapping };
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
    if (!next || busy.value || refreshPending.value || stale.value) return;
    await commit(() => callTool('system_set', { system: clone(next), revision: revision.value }));
  }

  async function autofocus(): Promise<void> {
    if (busy.value || dirty.value || stale.value || refreshPending.value) return;
    // The engine rejects the call if this revision is no longer current, so a stale view cannot apply it.
    await commit(() => callTool('autofocus', { wavelength: wavelength.value, revision: revision.value }));
  }

  /**
   * Run a backend operation that rewrites the saved system (import, catalog import),
   * then adopt the result. Refused while a draft is pending so it cannot be lost silently.
   * Resolves true once the operation is confirmed, even if the refresh afterwards failed.
   */
  async function mutate(op: (revision: number) => Promise<unknown>): Promise<boolean> {
    if (busy.value) return false;
    if (refreshPending.value) {
      actionError.value = 'The last change was saved, but the view has not refreshed yet. Wait for it to reconnect before running another operation.';
      return false;
    }
    if (stale.value) {
      actionError.value = 'The system changed elsewhere. Discard the draft and reload before running this operation.';
      return false;
    }
    if (dirty.value) {
      actionError.value = 'Apply or revert draft changes before running this operation.';
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
  watch([revision, fieldIndex, wavelengthIndex, hasSystem], () => void analyze());
  watch([revision, fieldIndex, wavelengthIndex, hasSystem, spotSampling, online], () => void analyzeSpot());
  // The wavelength only matters in monochromatic mode, so weighted results are not discarded when it changes.
  watch(
    [
      revision, fieldIndex, () => (mtfSpectralMode.value === 'mono' ? wavelengthIndex.value : -1), hasSystem,
      mtfAxis, mtfMaxFrequency, mtfSampling, mtfSpectralMode, wantMtf, online,
    ],
    () => void analyzeMtf(),
  );
  watch(online, () => void analyze());

  onMounted(async () => {
    if (!inDesktop()) {
      offlineReason.value = 'Launch the desktop app to connect to the Rust optical engine.';
      return;
    }
    await poll();
    timer = setInterval(() => void poll(), POLL_MS);
  });
  onBeforeUnmount(() => {
    if (timer !== undefined) clearInterval(timer);
  });

  return {
    online, offlineReason, system, draft, revision, stale, refreshPending, busy, dirty, problems,
    actionError, analysisError, materials, fieldIndex, wavelengthIndex, wavelength, layoutWavelength,
    mtfAxis, mtfMaxFrequency, spotSampling, mtfSampling, mtfSpectralMode, paraxial, layout, spot, mtf,
    spotRun, mtfRun, spotPending, mtfPending,
    save, autofocus, revert, discardAndReload, mutate,
  };
}
