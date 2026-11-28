/// <reference types="vite/client" />

declare module '*.vue' {
    import type { DefineComponent } from 'vue';
    const component: DefineComponent<{}, {}, any>;
    export default component;
}

/** Application version from tauri.conf.json, injected by Vite. */
declare const __APP_VERSION__: string;
/** Source commit the build was made from, or an empty string outside a git checkout. */
declare const __APP_COMMIT__: string;
