import type { PupilSampling, Tab } from './types';

/** UI locale; every visible label has an explicit translation in each. */
export type Locale = 'zh-CN' | 'en-US';
export type ThemeName = 'system' | 'light' | 'dark' | 'high-contrast';
export type AccentName = 'blue' | 'teal' | 'amber';
export type DensityName = 'compact' | 'comfortable';
export type AgentTarget = 'codex' | 'claude';
export type SamplingKind = PupilSampling['kind'];

export interface AppearancePreferences {
    theme: ThemeName;
    accent: AccentName;
    density: DensityName;
    fontSize: number;
    reducedMotion: boolean;
}

export interface WorkspacePreferences {
    /** An actual Tab id; labels localize, ids stay stable. */
    defaultView: Tab;
    restoreLastView: boolean;
    showNavigator: boolean;
    showPrescriptionDock: boolean;
    showMetrics: boolean;
    showStatusBar: boolean;
    /** Navigator width as a percentage of the workspace row (10–40). */
    navigatorSize: number;
    /** Prescription dock height as a percentage of the center column (15–60). */
    dockSize: number;
}

export interface DisplayPreferences {
    /** Decimals in read-only numeric displays; never applied to editable model inputs. */
    decimals: number;
    layoutRayArrows: boolean;
    plotGrid: boolean;
}

export interface AnalysisPreferences {
    samplingKind: SamplingKind;
    /** Grid points across the pupil diameter; odd, 3–129. */
    gridSize: number;
    /** Hexapolar rings, 1–64. */
    hexRings: number;
    reportPrecision: number;
    pollIntervalMs: number;
}

export interface AgentPreferences {
    /** Selected agents, each at most once. */
    targets: AgentTarget[];
    autoRegister: boolean;
}

export interface Preferences {
    locale: Locale;
    appearance: AppearancePreferences;
    workspace: WorkspacePreferences;
    display: DisplayPreferences;
    analysis: AnalysisPreferences;
    agents: AgentPreferences;
}

/** Shape returned by get_preferences / set_preferences and included in the preferences-changed event. */
export interface PreferencesSnapshot {
    preferences: Preferences;
    revision: number;
    warning?: string;
}

export type PreferencesRejection =
    | { code: 'revision_conflict'; snapshot: PreferencesSnapshot; message: string }
    | { code: 'validation' | 'storage'; message: string };

export function defaultPreferences(): Preferences {
    return {
        locale: 'zh-CN',
        appearance: {
            theme: 'system',
            accent: 'blue',
            density: 'compact',
            fontSize: 13,
            reducedMotion: false,
        },
        workspace: {
            defaultView: 'Layout',
            restoreLastView: true,
            showNavigator: true,
            showPrescriptionDock: true,
            showMetrics: true,
            showStatusBar: true,
            navigatorSize: 18,
            dockSize: 35,
        },
        display: { decimals: 4, layoutRayArrows: true, plotGrid: true },
        analysis: {
            samplingKind: 'grid',
            gridSize: 33,
            hexRings: 8,
            reportPrecision: 6,
            pollIntervalMs: 1500,
        },
        agents: { targets: [], autoRegister: false },
    };
}

export const PREF_LIMITS = {
    fontSize: { min: 11, max: 18 },
    navigatorSize: { min: 10, max: 40 },
    dockSize: { min: 15, max: 60 },
    decimals: { min: 0, max: 8 },
    gridSize: { min: 3, max: 129 },
    hexRings: { min: 1, max: 64 },
    reportPrecision: { min: 0, max: 15 },
    pollIntervalMs: { min: 500, max: 10000 },
} as const;

export const AGENT_TARGETS: readonly AgentTarget[] = ['codex', 'claude'];

/** Sampling preset derived from the analysis preferences, used to seed new analysis requests. */
export function samplingPreset(a: AnalysisPreferences): PupilSampling {
    return a.samplingKind === 'grid'
        ? { kind: 'grid', count: a.gridSize }
        : { kind: 'hexapolar', rings: a.hexRings };
}

/** Narrow an unknown Tauri error to the structured rejection, when it is one. */
export function asPreferencesRejection(e: unknown): PreferencesRejection | null {
    if (typeof e === 'object' && e !== null && 'code' in e) {
        const c = e as { code?: unknown };
        if (c.code === 'revision_conflict' || c.code === 'validation' || c.code === 'storage')
            return e as PreferencesRejection;
    }
    return null;
}
