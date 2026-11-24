// Fixture globals the UI tests inject into pages (see each test's addInitScript block).
/* eslint-disable @typescript-eslint/no-explicit-any */
export {};

declare global {
    var isTauri: boolean;
    var __heldProjectFiles: any[];
    var __holdProjectFiles: string[];
    interface Window {
        isTauri: boolean;
        __TAURI_INTERNALS__: any;
        __TAURI_EVENT_PLUGIN_INTERNALS__: any;
        __TAURI__: any;
        __bridge: (command: string, args?: any) => Promise<any>;
        __host: (command: string, args?: any) => Promise<any>;
        __testInvoke: (command: string, args?: any) => Promise<any>;
        __testTauriCallback: (handler: number, event: unknown) => void;
        __opticalInvoke: (command: string, args?: any) => Promise<any>;
        __emit: (event: any) => void;
        __chatChannel: any;
        __chatRequests: any[];
        __modelCalls: string[];
        __failModels: boolean;
        __holdModels: boolean;
        __releaseModels: (value?: unknown) => void;
        __history: any;
        __activeRecord: string;
        __activeTurn: string;
    }
}
