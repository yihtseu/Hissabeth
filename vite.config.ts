/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import tailwindcss from '@tailwindcss/vite';
import process from 'node:process';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
const host = process.env.TAURI_DEV_HOST;
// The About page shows the installer's version and the commit it was built from.
const appVersion: string = JSON.parse(
    readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
).version;
function appCommit(): string {
    try {
        return execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
    } catch {
        return '';
    }
}

// https://vite.dev/config/
export default defineConfig(() => ({
    plugins: [vue(), tailwindcss()],
    define: {
        __APP_VERSION__: JSON.stringify(appVersion),
        __APP_COMMIT__: JSON.stringify(appCommit()),
    },
    test: {
        environment: 'jsdom',
        include: ['src/**/*.test.ts'],
    },
    clearScreen: false,
    server: {
        port: 1420,
        strictPort: true,
        host: host || false,
        hmr: host
            ? {
                  protocol: 'ws',
                  host,
                  port: 1421,
              }
            : undefined,
        watch: {
            ignored: ['**/src-tauri/**'],
        },
    },
}));
