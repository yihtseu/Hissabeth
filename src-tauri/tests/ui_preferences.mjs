// Shared browser fixture for application preferences. Native persistence is exercised separately.
export const defaultPreferences = {
    locale: 'zh-CN',
    appearance: { theme: 'system', accent: 'blue', density: 'compact', fontSize: 13, reducedMotion: false },
    workspace: { defaultView: 'Layout', restoreLastView: true, showNavigator: true, showPrescriptionDock: true,
        showMetrics: true, showStatusBar: true, navigatorSize: 18, dockSize: 35 },
    display: { decimals: 4, layoutRayArrows: true, plotGrid: true },
    analysis: { samplingKind: 'grid', gridSize: 33, hexRings: 8, reportPrecision: 6, pollIntervalMs: 1500 },
    agents: { targets: [], autoRegister: false },
};
