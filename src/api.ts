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
  FieldCurvature,
  GeometricMtf,
  ImportFormat,
  InspectLaunch,
  Layout,
  LongitudinalAberration,
  MaterialSearchResult,
  MtfAxis,
  OpdFan,
  OpticalSystem,
  Paraxial,
  ParaxialTrace,
  PolychromaticSpot,
  PrescriptionReport,
  PupilSampling,
  RayFan,
  SectionId,
  SolveArgs,
  SolveRecalculateResult,
  SolveResult,
  Spectrum,
  Spherochromatism,
  SpotDiagram,
  SystemSnapshot,
  TraceInspect,
  VignettingUpdateResult,
  Wavefront,
} from './types';

type NoArgs = Record<string, never>;

interface WavelengthArgs {
  wavelength: number;
}
/** Stored field number (index into system.fields). */
interface FieldArgs extends WavelengthArgs {
  field_index: number;
}

interface ToolMap {
  system_get: { args: NoArgs; result: SystemSnapshot };
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
  project_export: { args: { format: ImportFormat }; result: { text: string } };
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
  field_curvature: { args: { samples: number; field_end?: [number, number] }; result: FieldCurvature };
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
}

export type ToolName = keyof ToolMap;
export type ToolArgs<N extends ToolName> = ToolMap[N]['args'];
export type ToolResult<N extends ToolName> = ToolMap[N]['result'];

export const inDesktop = (): boolean => isTauri();

export function callTool<N extends ToolName>(
  name: N,
  args: ToolArgs<N>,
): Promise<ToolResult<N>> {
  return invoke<ToolResult<N>>('optical_call', { name, args });
}

export const detectAgents = (): Promise<AgentInfo[]> => invoke<AgentInfo[]>('detect_agents');
export const configureAgents = (): Promise<AgentRegistration[]> =>
  invoke<AgentRegistration[]>('configure_agents');

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
