import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
    AgentInfo,
    AgentRegistration,
    ApertureReport,
    Astigmatism,
    CatalogImportResult,
    ChromaticFocus,
    DiffractionMtf,
    DiffractionPsf,
    Distortion,
    EncircledEnergy,
    FiberCoupling,
    FieldCurvature,
    GaussianBeam,
    GeometricMtf,
    HistoryCompareResult,
    HistoryList,
    HuygensPsf,
    ImportFormat,
    InspectLaunch,
    Layout,
    LongitudinalAberration,
    MaterialSearchResult,
    MtfAxis,
    OpdFan,
    OpticalSystem,
    OptimizeOperand,
    OptimizeResult,
    OptimizeVariable,
    Paraxial,
    ParaxialTrace,
    PolychromaticSpot,
    PrescriptionReport,
    ProjectListResult,
    PupilSampling,
    RayFan,
    RecoveryResult,
    RecoverySource,
    SectionId,
    SolveArgs,
    SolveRecalculateResult,
    SolveResult,
    Spectrum,
    Spherochromatism,
    SpotDiagram,
    SurfaceEditOperation,
    SurfaceEditResult,
    SystemSnapshot,
    ToleranceCompensator,
    ToleranceCriterion,
    ToleranceMonteCarloResult,
    ToleranceOperand,
    ToleranceSensitivityResult,
    TraceInspect,
    VignettingUpdateResult,
    Wavefront,
    WorkspaceStatus,
    ZernikeFit,
} from './types';

type NoArgs = Record<string, never>;

interface WavelengthArgs {
    wavelength: number;
}
/** Stored field number (index into system.fields). */
interface FieldArgs extends WavelengthArgs {
    field_index: number;
}

interface BatchFailure {
    index: number;
    command: string;
    error: string;
}

interface ToolMap {
    batch: {
        args: {
            steps: { command: string; args: Record<string, unknown> }[];
            rollback?: boolean;
            revision?: number;
        };
        result: {
            ok: boolean;
            results: (
                | { index: number; command: string; ok: true; result: unknown }
                | (BatchFailure & { ok: false })
            )[];
            failed: BatchFailure | null;
            revision: number;
        };
    };
    bench_example: { args: NoArgs; result: { bench: import('./bench').Bench } };
    bench_get: {
        args: NoArgs;
        result: { bench: import('./bench').Bench | null; revision: number };
    };
    bench_set: {
        args: { bench: import('./bench').BenchInput; revision: number };
        result: {
            revision: number;
            trace?: Omit<import('./bench').BenchTrace, 'revision' | 'saved'>;
        };
    };
    bench_trace: {
        args: { bench?: import('./bench').BenchInput };
        result: import('./bench').BenchTrace;
    };
    bench_assembly: {
        args: { bench?: import('./bench').BenchInput };
        result: import('./bench').BenchAssembly;
    };
    bench_catalog: {
        args: { query?: string; kind?: string };
        result: { items: import('./bench').BenchCatalogItem[] };
    };
    bench_template: {
        args: { name: string; apply?: boolean; revision?: number };
        result: { bench: import('./bench').Bench; applied?: boolean; revision?: number };
    };
    bench_auto_aim: {
        args: {
            bench?: import('./bench').BenchInput;
            snap_holes?: boolean;
            uniform_z?: number;
            apply?: boolean;
            revision?: number;
        };
        result: {
            ok: boolean;
            bench: import('./bench').Bench;
            adjusted_elements: string[];
            warnings: string[];
            conflicts: import('./bench').BenchPort[];
            residuals: import('./bench').BenchAimResidual[];
            periscopes?: import('./bench').BenchPeriscopeReading[];
            applied: boolean;
            revision?: number;
        };
    };
    /** Reads stage, path, assertion and lab diagnostics from a single revisioned calculation. */
    bench_lab_check: {
        args: { bench?: import('./bench').BenchInput; require_pass?: boolean };
        result: import('./bench').BenchLabCheck;
    };
    /** Angular-spectrum field at one branch end or superposed at one input port. */
    bench_field: {
        args: {
            bench?: import('./bench').BenchInput;
            branch?: string;
            port?: import('./bench').BenchPort;
            wavelength?: number;
            realizations?: number;
            arrays?: boolean;
        };
        result: import('./bench').BenchFieldReading;
    };
    bench_alignment_guide: {
        args: { bench?: import('./bench').BenchInput };
        result: import('./bench').BenchAlignmentGuide;
    };
    bench_export_blueprint: {
        args: {
            bench?: import('./bench').BenchInput;
            format: 'svg' | 'dxf' | 'stl' | 'alignment_md';
        };
        result: import('./bench').BenchBlueprintExport;
    };
    /** Workspace state; the only read that also works during recovery. */
    workspace_status: { args: NoArgs; result: WorkspaceStatus };
    /** Recovery only: rereads the file; the revision must be the one the caller last observed. */
    workspace_retry: { args: { revision: number }; result: WorkspaceStatus };
    /** Recovery only: validates the source fully before any disk change. */
    workspace_recover: {
        args: { revision: number; source: RecoverySource };
        result: RecoveryResult;
    };
    system_get: { args: NoArgs; result: SystemSnapshot };
    /** Read-only: transforms the supplied draft system; the saved model, revision and solves are untouched. */
    surface_edit: {
        args: { system: OpticalSystem; operation: SurfaceEditOperation };
        result: SurfaceEditResult;
    };
    system_set: { args: { system: OpticalSystem; revision: number }; result: { revision: number } };
    materials_list: { args: NoArgs; result: string[] };
    material_index: { args: { material: string; wavelength: number }; result: { index: number } };
    material_search: {
        args: { query: string; catalog?: string; offset: number; limit: number };
        result: MaterialSearchResult;
    };
    catalog_import: {
        args: { catalog: string; text: string; revision: number };
        result: CatalogImportResult;
    };
    project_import: {
        args: { format: ImportFormat; text: string; revision: number };
        result: { revision: number };
    };
    /** Read-only; text, revision, name and format all come from one saved snapshot. */
    project_export: {
        args: { format: ImportFormat };
        result: { text: string; revision: number; name: string; format: ImportFormat };
    };
    /** Mutating: replaces the saved system with a clean blank prescription; the revision must be the one the caller last read. */
    project_new: { args: { revision: number }; result: { revision: number } };
    paraxial: { args: WavelengthArgs; result: Paraxial };
    layout: { args: NoArgs; result: Layout };
    /** Read-only: assembles selected saved-prescription sections; an empty sections list yields the header only. */
    prescription_report: {
        args: { sections: SectionId[]; wavelength: number; precision: number };
        result: PrescriptionReport;
    };
    paraxial_trace: { args: { wavelength: number; field_index: number }; result: ParaxialTrace };
    trace_inspect: { args: { wavelength: number; launch: InspectLaunch }; result: TraceInspect };
    spot_diagram: { args: FieldArgs & { sampling: PupilSampling }; result: SpotDiagram };
    geometric_mtf: {
        args: {
            field_index: number;
            sampling: PupilSampling;
            spectrum: Spectrum;
            max_frequency: number;
            axis: MtfAxis;
        };
        result: GeometricMtf;
    };
    polychromatic_spot: {
        args: { field_index: number; sampling: PupilSampling };
        result: PolychromaticSpot;
    };
    /** Mutating: the revision must be the one the caller last read. */
    autofocus: { args: WavelengthArgs & { revision: number }; result: { revision: number } };
    solve: { args: SolveArgs; result: SolveResult };
    /** Read-only coverage of the derived drawing semi-diameters. */
    aperture_report: { args: NoArgs; result: ApertureReport };
    /** Mutating: derives vignetting factors from physical apertures. */
    vignetting_update: { args: { revision: number }; result: VignettingUpdateResult };
    solve_recalculate: { args: { revision: number }; result: SolveRecalculateResult };
    // Analysis tools. Argument names follow the tool schemas in service.rs.
    ray_fan: { args: FieldArgs & { samples: number }; result: RayFan };
    opd_fan: { args: FieldArgs & { samples: number }; result: OpdFan };
    chromatic_focus: {
        args: { lower: number; upper: number; samples: number };
        result: ChromaticFocus;
    };
    longitudinal_aberration: {
        args: WavelengthArgs & { samples: number };
        result: LongitudinalAberration;
    };
    spherochromatism: { args: { samples: number }; result: Spherochromatism };
    field_curvature: {
        args: { samples: number; field_end?: [number, number] };
        result: FieldCurvature;
    };
    astigmatism: { args: FieldArgs; result: Astigmatism };
    distortion: { args: FieldArgs; result: Distortion };
    wavefront: { args: FieldArgs & { grid: number }; result: Wavefront };
    diffraction_psf: { args: FieldArgs & { grid: number }; result: DiffractionPsf };
    diffraction_mtf: {
        args: { field_index: number; spectrum: Spectrum; grid: number; max_frequency?: number };
        result: DiffractionMtf;
    };
    encircled_energy: {
        args: FieldArgs & { sampling: PupilSampling; samples: number };
        result: EncircledEnergy;
    };
    wavefront_zernike: { args: FieldArgs & { grid: number; terms: number }; result: ZernikeFit };
    huygens_psf: {
        args: FieldArgs & { grid: number; size?: number; pitch?: number };
        result: HuygensPsf;
    };
    huygens_mtf: {
        args: { field_index: number; spectrum: Spectrum; grid: number; max_frequency?: number };
        result: DiffractionMtf;
    };
    gaussian_beam: {
        args: WavelengthArgs & { waist: number; waist_z?: number };
        result: GaussianBeam;
    };
    fiber_coupling: {
        args: WavelengthArgs & { waist: number; waist_z?: number; mode_field_diameter: number };
        result: FiberCoupling;
    };
    design_summary: { args: NoArgs; result: Record<string, unknown> };
    optimize: {
        args: {
            variables: OptimizeVariable[];
            operands: OptimizeOperand[];
            max_iterations?: number;
            initial_damping?: number;
            tolerance?: number;
            apply?: boolean;
            revision?: number;
        };
        result: OptimizeResult;
    };
    tolerance_sensitivity: {
        args: {
            tolerances: ToleranceOperand[];
            compensator?: ToleranceCompensator;
            criterion: ToleranceCriterion;
        };
        result: ToleranceSensitivityResult;
    };
    tolerance_monte_carlo: {
        args: {
            tolerances: ToleranceOperand[];
            compensator?: ToleranceCompensator;
            criterion: ToleranceCriterion;
            trials?: number;
            distribution?: 'normal' | 'uniform';
            seed?: number;
            pass_threshold?: number | null;
            lower_is_better?: boolean | null;
        };
        result: ToleranceMonteCarloResult;
    };
    history_list: { args: NoArgs; result: HistoryList };
    history_undo: {
        args: { revision: number };
        result: { revision: number; cursor: number; can_undo: boolean; can_redo: boolean };
    };
    history_redo: {
        args: { revision: number };
        result: { revision: number; cursor: number; can_undo: boolean; can_redo: boolean };
    };
    history_restore: { args: { index: number; revision: number }; result: { revision: number } };
    history_compare: {
        args: { from_index?: number; to_index?: number };
        result: HistoryCompareResult;
    };
    project_list: { args: NoArgs; result: ProjectListResult };
    project_open: {
        args: { path: string; revision: number };
        result: { revision: number; path: string; linked: boolean };
    };
    project_save_as: {
        args: { path: string; revision: number };
        result: { revision: number; path: string };
    };
}

export type ToolName = keyof ToolMap;
export type ToolArgs<N extends ToolName> = ToolMap[N]['args'];
export type ToolResult<N extends ToolName> = ToolMap[N]['result'];

export const inDesktop = (): boolean => isTauri();

export function callTool<N extends ToolName>(name: N, args: ToolArgs<N>): Promise<ToolResult<N>> {
    return invoke<ToolResult<N>>('optical_call', { name, args });
}

export const detectAgents = (): Promise<AgentInfo[]> => invoke<AgentInfo[]>('detect_agents');
export const configureAgents = (agents: ('codex' | 'claude')[]): Promise<AgentRegistration[]> =>
    invoke<AgentRegistration[]>('configure_agents', { agents });

export function errorMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
}
