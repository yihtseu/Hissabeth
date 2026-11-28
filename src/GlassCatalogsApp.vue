<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Library } from 'lucide-vue-next';
import { useAppearance } from './composables/useAppearance';
import { i18n, tr } from './i18n';
import { errorMessage } from './api';
import type { CatalogWarning } from './types';

/** Mirrors `glass_catalogs::Status` in src-tauri. */
type Origin = { download: { url: string } } | { file: { name: string } };
type InstalledRecord = { origin: Origin; installed_at: number; glasses: number };
type Vendor = {
    catalog: string;
    name: string;
    terms: string | null;
    page: string;
    download: string | null;
    installed: InstalledRecord | null;
    warnings: CatalogWarning[];
    error: string | null;
};
type Status = { prompt: boolean; vendors: Vendor[]; error: string | null };

useAppearance();
const status = ref<Status>();
const loadError = ref('');
const acknowledged = ref(false);
const busy = reactive<Partial<{ [catalog: string]: 'download' | 'import' | 'remove' }>>({});
const failures = reactive<Partial<{ [catalog: string]: string }>>({});
const notes = reactive<Partial<{ [catalog: string]: string }>>({});
const closeError = ref('');

async function refresh(): Promise<void> {
    try {
        status.value = await invoke<Status>('glass_catalogs');
        loadError.value = '';
    } catch (error) {
        loadError.value = errorMessage(error);
    }
}

let stopChanged: UnlistenFn | undefined;
let disposed = false;
onMounted(async () => {
    await refresh();
    try {
        const stop = await listen('glass-catalogs-changed', () => void refresh());
        if (disposed) stop();
        else stopChanged = stop;
    } catch (error) {
        loadError.value = errorMessage(error);
    }
});
onBeforeUnmount(() => {
    disposed = true;
    stopChanged?.();
});

const anyBusy = computed(() => Object.values(busy).some(Boolean));

async function run(
    vendor: Vendor,
    kind: 'download' | 'import' | 'remove',
    action: () => Promise<{ warnings: CatalogWarning[]; glasses: number } | void>,
): Promise<void> {
    busy[vendor.catalog] = kind;
    failures[vendor.catalog] = '';
    notes[vendor.catalog] = '';
    try {
        const done = await action();
        if (done)
            notes[vendor.catalog] = tr('glass.installedNote', {
                n: done.glasses,
                warnings: done.warnings.length,
            });
    } catch (error) {
        failures[vendor.catalog] = errorMessage(error);
    } finally {
        busy[vendor.catalog] = undefined;
        await refresh();
    }
}

type Installed = { record: InstalledRecord; warnings: CatalogWarning[] };
const summary = (r: Installed) => ({ warnings: r.warnings, glasses: r.record.glasses });

function download(vendor: Vendor): Promise<void> {
    return run(vendor, 'download', async () =>
        summary(await invoke<Installed>('glass_catalog_download', { catalog: vendor.catalog })),
    );
}

/** Vendors a single click can fetch: a direct link and nothing usable installed yet. */
const pending = computed(() =>
    (status.value?.vendors ?? []).filter((v) => v.download && (!v.installed || v.error)),
);
const allProgress = ref<{ name: string; index: number; total: number }>();
const allSummary = ref<{ done: number; failed: number }>();

/** One vendor at a time, so each site sees a single request and progress reads in order. */
async function downloadAll(): Promise<void> {
    const queue = pending.value;
    allSummary.value = undefined;
    const failed = await queue.reduce(async (earlier: Promise<number>, vendor, index) => {
        const count = await earlier;
        allProgress.value = { name: vendor.name, index: index + 1, total: queue.length };
        await download(vendor);
        return count + (failures[vendor.catalog] ? 1 : 0);
    }, Promise.resolve(0));
    allProgress.value = undefined;
    allSummary.value = { done: queue.length - failed, failed };
}

function importFile(vendor: Vendor): Promise<void> {
    return run(vendor, 'import', async () => {
        const path = await openDialog({
            multiple: false,
            directory: false,
            filters: [{ name: tr('glass.importFilter'), extensions: ['agf', 'AGF', 'zip'] }],
        });
        if (typeof path !== 'string') return;
        return summary(
            await invoke<Installed>('glass_catalog_import', { catalog: vendor.catalog, path }),
        );
    });
}

function remove(vendor: Vendor): Promise<void> {
    return run(vendor, 'remove', () =>
        invoke<void>('glass_catalog_remove', { catalog: vendor.catalog }),
    );
}

async function visit(vendor: Vendor, url: string): Promise<void> {
    failures[vendor.catalog] = '';
    try {
        await openUrl(url);
    } catch (error) {
        failures[vendor.catalog] = tr('glass.openFailed', { url, message: errorMessage(error) });
    }
}

async function close(skip: boolean): Promise<void> {
    closeError.value = '';
    try {
        if (skip) await invoke('glass_catalogs_dismiss');
        await getCurrentWindow().close();
    } catch (error) {
        closeError.value = errorMessage(error);
    }
}

function installedDate(seconds: number): string {
    return new Date(seconds * 1000).toLocaleDateString(i18n.global.locale.value, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
    });
}

function source(origin: Origin): string {
    if ('file' in origin) return tr('glass.fromFile', { name: origin.file.name });
    try {
        return tr('glass.fromSite', { host: new URL(origin.download.url).host });
    } catch {
        return tr('glass.fromSite', { host: origin.download.url });
    }
}

const link = 'tw:text-accent tw:underline-offset-2 tw:hover:underline';
</script>

<template>
    <div class="settings-app glass-app">
        <header class="settings-heading">
            <Library :size="18" />
            <h1>{{ tr('glass.title') }}</h1>
            <span>{{ tr('glass.subtitle') }}</span>
        </header>
        <div v-if="loadError" class="banner error" role="alert">
            {{ tr('glass.loadFailed', { message: loadError })
            }}<button @click="refresh">{{ tr('settings.retry') }}</button>
        </div>
        <div v-if="status?.error" class="banner warn" role="alert">
            {{ tr('glass.manifestFailed', { message: status.error }) }}
        </div>
        <main class="settings-content glass-content">
            <section class="glass-notice" aria-labelledby="glass-notice-title">
                <h2 id="glass-notice-title">{{ tr('glass.noticeTitle') }}</h2>
                <p>{{ tr('glass.noticeOwnership') }}</p>
                <p>{{ tr('glass.noticeDirect') }}</p>
                <p>{{ tr('glass.noticeTerms') }}</p>
                <label class="glass-ack">
                    <input v-model="acknowledged" type="checkbox" />
                    <span>{{ tr('glass.acknowledge') }}</span>
                </label>
            </section>

            <section v-if="status" class="glass-all">
                <button
                    class="primary"
                    :disabled="!acknowledged || anyBusy || !pending.length"
                    :title="acknowledged ? '' : tr('glass.acknowledgeFirst')"
                    @click="downloadAll"
                >
                    {{
                        allProgress
                            ? tr('glass.downloadingAll', allProgress)
                            : tr('glass.downloadAll', { n: pending.length })
                    }}
                </button>
                <span v-if="!pending.length && !allSummary" class="glass-hint">{{
                    tr('glass.nothingPending')
                }}</span>
                <span
                    v-if="allSummary"
                    :class="allSummary.failed ? 'glass-error' : 'glass-note'"
                    role="status"
                    >{{
                        allSummary.failed
                            ? tr('glass.allSomeFailed', allSummary)
                            : tr('glass.allDone', { n: allSummary.done })
                    }}</span
                >
            </section>

            <ul v-if="status" class="glass-vendors">
                <li v-for="vendor in status.vendors" :key="vendor.catalog" class="glass-vendor">
                    <div class="glass-vendor-head">
                        <h3>{{ vendor.name }}</h3>
                        <code>{{ vendor.catalog }}</code>
                        <span v-if="vendor.error" class="glass-state bad">{{
                            tr('glass.stateBroken')
                        }}</span>
                        <span v-else-if="vendor.installed" class="glass-state good">{{
                            tr('glass.stateInstalled', {
                                n: vendor.installed.glasses,
                                date: installedDate(vendor.installed.installed_at),
                            })
                        }}</span>
                        <span v-else class="glass-state">{{ tr('glass.stateMissing') }}</span>
                    </div>
                    <p class="glass-meta">
                        <template v-if="vendor.installed"
                            >{{ source(vendor.installed.origin) }} ·
                        </template>
                        <button
                            v-if="vendor.terms"
                            type="button"
                            class="glass-link"
                            :class="link"
                            @click="visit(vendor, vendor.terms)"
                        >
                            {{ tr('glass.terms') }}
                        </button>
                        <span v-else>{{ tr('glass.noTerms') }}</span>
                    </p>
                    <div class="glass-actions">
                        <button
                            v-if="vendor.download"
                            class="primary"
                            :disabled="!acknowledged || anyBusy"
                            :title="acknowledged ? vendor.download : tr('glass.acknowledgeFirst')"
                            @click="download(vendor)"
                        >
                            {{
                                busy[vendor.catalog] === 'download'
                                    ? tr('glass.downloading')
                                    : vendor.installed
                                      ? tr('glass.redownload')
                                      : tr('glass.download')
                            }}
                        </button>
                        <button @click="visit(vendor, vendor.page)">
                            {{ tr('glass.openPage') }}
                        </button>
                        <button :disabled="anyBusy" @click="importFile(vendor)">
                            {{
                                busy[vendor.catalog] === 'import'
                                    ? tr('glass.importing')
                                    : tr('glass.import')
                            }}
                        </button>
                        <button
                            v-if="vendor.installed || vendor.error"
                            :disabled="anyBusy"
                            @click="remove(vendor)"
                        >
                            {{ tr('glass.remove') }}
                        </button>
                    </div>
                    <p v-if="!vendor.download" class="glass-hint">
                        {{ tr('glass.noDirectLink') }}
                    </p>
                    <p v-if="notes[vendor.catalog]" class="glass-note" role="status">
                        {{ notes[vendor.catalog] }}
                    </p>
                    <p v-if="failures[vendor.catalog]" class="glass-error" role="alert">
                        {{ failures[vendor.catalog] }}
                    </p>
                    <p v-if="vendor.error" class="glass-error">{{ vendor.error }}</p>
                    <details v-if="vendor.warnings.length" class="glass-warnings">
                        <summary>
                            {{ tr('glass.warnings', { n: vendor.warnings.length }) }}
                        </summary>
                        <ul>
                            <li v-for="(w, i) in vendor.warnings" :key="i">
                                {{
                                    tr('glass.warningLine', {
                                        line: w.line,
                                        glass: w.glass || '—',
                                        message: w.message,
                                    })
                                }}
                            </li>
                        </ul>
                    </details>
                </li>
            </ul>
        </main>
        <footer class="settings-footer">
            <span v-if="closeError" class="glass-error" role="alert">{{ closeError }}</span>
            <div class="spacer"></div>
            <button v-if="status?.prompt" :disabled="anyBusy" @click="close(true)">
                {{ tr('glass.skip') }}
            </button>
            <button @click="close(false)">{{ tr('common.close') }}</button>
        </footer>
    </div>
</template>

<style scoped>
.glass-content {
    display: grid;
    align-content: start;
    gap: 16px;
}
.glass-notice {
    max-width: 720px;
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--panel);
    font-size: 12px;
    line-height: 1.6;
}
.glass-notice h2 {
    font-size: 14px;
    margin: 0 0 6px;
}
.glass-notice p {
    margin: 0 0 6px;
}
.glass-ack {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-top: 10px;
    font-weight: 600;
}
.glass-ack input {
    min-height: 0;
    margin: 3px 0 0;
    width: 14px;
    height: 14px;
    flex: none;
    accent-color: var(--accent);
}
.glass-all {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    max-width: 720px;
    font-size: 12px;
}
.glass-all .glass-hint,
.glass-all .glass-note,
.glass-all .glass-error {
    margin: 0;
}
.glass-vendors {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 10px;
    max-width: 720px;
}
.glass-vendor {
    padding: 10px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
}
.glass-vendor-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex-wrap: wrap;
}
.glass-vendor-head h3 {
    font-size: 13px;
    margin: 0;
}
.glass-vendor-head code {
    font-size: 11px;
    color: var(--muted);
}
.glass-state {
    margin-left: auto;
    color: var(--muted);
}
.glass-state.good {
    color: var(--positive);
}
.glass-state.bad,
.glass-error {
    color: var(--danger);
}
.glass-meta,
.glass-hint {
    margin: 4px 0 8px;
    color: var(--muted);
}
.glass-link {
    border: 0;
    background: transparent;
    padding: 0;
    min-height: 0;
    font: inherit;
}
.glass-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
}
.glass-note,
.glass-error {
    margin: 6px 0 0;
    overflow-wrap: anywhere;
}
.glass-warnings {
    margin-top: 6px;
    color: var(--muted);
}
.glass-warnings ul {
    margin: 4px 0 0;
    padding-left: 18px;
    max-height: 140px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 11px;
}
</style>
