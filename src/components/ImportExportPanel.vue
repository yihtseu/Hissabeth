<script setup lang="ts">
import { tr } from '../i18n';
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { FolderOpen, SaveAll } from 'lucide-vue-next';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { readTextFile } from '@tauri-apps/plugin-fs';
import { callTool, errorMessage, inDesktop, type ToolResult } from '../api';
import type { ImportFormat, OpticalSystem, ProjectListResult } from '../types';

const props = defineProps<{
    system: OpticalSystem;
    revision: number;
    stale: boolean;
    refreshPending: boolean;
    online: boolean;
    dirty: boolean;
    busy: boolean;
    mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
}>();

const FORMATS: { id: ImportFormat; label: string }[] = [
    {
        id: 'hissabeth',
        get label() {
            return tr('remaining.m0');
        },
    },
    { id: 'geopter', label: 'Geopter' },
    { id: 'zmx', label: 'ZMX (Zemax)' },
];

/** Format requested for the next export and declared for the next import. */
const format = ref<ImportFormat>('hissabeth');
const text = ref('');
const status = ref('');
const error = ref('');
const working = ref(false);
const importing = ref(false);
const creating = ref(false);
const confirming = ref(false);
/** A file read is in flight; cleared by completion or cancelFile. */
const reading = ref(false);

/**
 * Where the current text came from. `textFormat` is the format it was produced or declared under;
 * it is cleared when the dropdown changes, so old text is never relabelled with a newer selection.
 */
const textFormat = ref<ImportFormat | null>(null);
const textName = ref('');
const textFileBase = ref('');

/** Bumped on every text change; async replies apply only while it is unchanged. */
let textGen = 0;
let exportToken = 0;
let fileToken = 0;
let disposed = false;
let openedRevision = 0;

const locked = computed(() => props.busy || importing.value || creating.value);

/** Stored conditions the Geopter format cannot hold; the engine still reports anything else it cannot represent. */
const builtIn = (where: string): string =>
    `${where} is FUSED_SILICA, a built-in material that cannot be stored in a Geopter project. Use the Hissabeth project format.`;
const geopterBlock = computed(() => {
    const sys = props.system;
    if (sys.pupil_mapping === 'real_stop') return tr('remaining.m1');
    if (sys.object_distance === null) return tr('remaining.m2');
    if (sys.custom_glasses.length > 0) return tr('remaining.m3');
    if (sys.object_material === 'FUSED_SILICA') return builtIn(tr('sys.objectMaterial'));
    const silica = sys.surfaces.findIndex((x) => x.material === 'FUSED_SILICA');
    if (silica >= 0) return builtIn(tr('ie.geopter.builtInSurface', { n: silica + 1 }));
    if (sys.temperature !== 25 || sys.pressure !== 101325) return tr('remaining.m4');
    const nonRefract = sys.surfaces.findIndex((x) => x.interaction !== 'refract');
    if (nonRefract >= 0)
        return `Geopter supports refracting surfaces only: set surface ${nonRefract + 1} to refract, or use the Hissabeth project format.`;
    const mixed = sys.surfaces.findIndex((x) => x.even.length > 0 && x.odd.length > 0);
    if (mixed >= 0)
        return `Geopter cannot store both even and odd asphere coefficients on one surface: clear one list on surface ${mixed + 1}, or use the Hissabeth project format.`;
    for (const [name, e] of [
        ['object', sys.object_surface],
        ['image', sys.image_surface],
    ] as const)
        if (e.even.length > 0 && e.odd.length > 0)
            return `Geopter cannot store both even and odd asphere coefficients on one surface: clear one list on the ${name} surface, or use the Hissabeth project format.`;
    // Geopter stores exactly ten even or odd coefficients per profile; shorter lists are padded with zeros on export.
    const profiles = [
        {
            get where() {
                return tr('layout.objectDefault');
            },
            e: sys.object_surface,
        },
        ...sys.surfaces.map((x, i) => ({
            get where() {
                return tr('ie.geopter.whereProfileSurface', { n: i + 1 });
            },
            e: x,
        })),
        {
            get where() {
                return tr('layout.imageDefault');
            },
            e: sys.image_surface,
        },
    ];
    for (const { where, e } of profiles)
        if (e.even.length > 10 || e.odd.length > 10)
            return `${where} has more than 10 ${e.even.length > 10 ? tr('ie.geopter.kindEven') : tr('ie.geopter.kindOdd')} asphere coefficients; Geopter stores at most 10. Remove the higher terms, or use the Hissabeth project format.`;
    if (sys.surfaces.some((x) => x.gap_solve.kind !== 'fixed')) return tr('remaining.m5');
    return '';
});
const zmxBlock = computed(() => {
    const sys = props.system;
    if (sys.bench)
        return 'ZMX cannot store the free-space bench. Use the Hissabeth project format.';
    if (sys.pupil.kind === 'image_na')
        return 'ZMX has no image-space NA aperture: switch the pupil to f-number, or use the Hissabeth project format.';
    if (sys.custom_glasses.length > 0)
        return 'ZMX export cannot embed custom glass catalogs. Use the Hissabeth project format.';
    if (sys.object_material === 'FUSED_SILICA')
        return 'ZMX catalogs have no FUSED_SILICA entry for the object material. Use the Hissabeth project format.';
    const dummy = sys.surfaces.findIndex(
        (x) => x.interaction === 'dummy' && (!x.transform || x.transform.return_after),
    );
    if (dummy >= 0)
        return `ZMX has no plain dummy surfaces: remove surface ${dummy + 1}, attach a persistent COORDBRK transform, or use the Hissabeth project format.`;
    const silica = sys.surfaces.findIndex((x) => x.material === 'FUSED_SILICA');
    if (silica >= 0)
        return `ZMX catalogs have no FUSED_SILICA entry for surface ${silica + 1}. Use the Hissabeth project format.`;
    const asymmetric = sys.fields.findIndex(
        (f) => f.vignetting[0] !== f.vignetting[1] || f.vignetting[2] !== f.vignetting[3],
    );
    if (asymmetric >= 0)
        return `ZMX stores only symmetric VCX/VCY vignetting: field ${asymmetric + 1} has directional factors. Use the Hissabeth project format.`;
    if (sys.surfaces.some((x) => x.gap_solve.kind !== 'fixed'))
        return 'ZMX does not store gap solve relationships. Use the Hissabeth project format.';
    return '';
});
const exportBlock = computed(() =>
    !props.online
        ? tr('common.offline')
        : format.value === 'geopter'
          ? geopterBlock.value
          : format.value === 'zmx'
            ? zmxBlock.value
            : '',
);
const stateBlock = computed(() =>
    !props.online
        ? tr('common.offline')
        : props.stale
          ? tr('ie.staleBlock')
          : props.refreshPending
            ? tr('ie.refreshBlock')
            : props.dirty
              ? tr('ie.dirtyBlock')
              : '',
);
const importBlock = computed(
    () => stateBlock.value || (!text.value.trim() ? tr('ie.emptyTextBlock') : ''),
);
const newBlock = computed(() => stateBlock.value);

function safeName(raw: string): string {
    return (
        raw
            .trim()
            .replace(/[^\w.-]+/g, '_')
            .replace(/^_+|_+$/g, '') || 'system'
    );
}
/** Named from the text's own origin and format, never from the current dropdown or model name. */
const downloadName = computed(() => {
    const f = textFormat.value;
    if (!f) return '';
    const base = safeName(
        textFileBase.value ? textFileBase.value.replace(/\.(json|zmx)$/i, '') : textName.value,
    );
    if (f === 'zmx') return base.endsWith('.zmx') ? base : `${base}.zmx`;
    return base.endsWith(`.${f}`) ? `${base}.json` : `${base}.${f}.json`;
});
const downloadNote = computed(() =>
    text.value && textFormat.value === null ? tr('ie.downloadNote') : '',
);

function cancelExport(note = ''): void {
    exportToken++;
    if (working.value && note) status.value = note;
    working.value = false;
}

/** Invalidates any file read and replaces the Reading status, so a silently dropped read never leaves it behind. */
function cancelFile(note = ''): void {
    fileToken++;
    if (reading.value) {
        reading.value = false;
        status.value = note;
    }
}

// A pending export is only valid for the format, system name and revision it was requested with.
watch(
    format,
    () => {
        cancelExport(tr('ie.cancelExportFormat'));
        cancelFile(tr('ie.cancelFileFormat'));
        textFormat.value = null;
    },
    { flush: 'sync' },
);
watch([() => props.revision, () => props.system.name], () =>
    cancelExport(tr('ie.cancelExportSystem')),
);
onBeforeUnmount(() => {
    disposed = true;
    exportToken++;
    fileToken++;
    dialogToken++;
});

function setText(
    value: string,
    origin: { format: ImportFormat; name: string; file: string },
): void {
    text.value = value;
    textGen++;
    textFormat.value = origin.format;
    textName.value = origin.name;
    textFileBase.value = origin.file;
}

function onTextInput(e: Event): void {
    const value = (e.target as HTMLTextAreaElement).value;
    text.value = value;
    textGen++;
    cancelFile(tr('ie.cancelFileText')); // a read in flight would overwrite this typing
    cancelExport(tr('ie.cancelExportText'));
    textFormat.value = format.value; // typing declares the selected format
    if (!textName.value && !textFileBase.value) textName.value = props.system.name;
}

async function doExport(): Promise<void> {
    if (exportBlock.value || working.value || locked.value) return;
    cancelFile(tr('ie.cancelExportRequest'));
    const token = ++exportToken;
    const gen = textGen;
    const f = format.value;
    const name = props.system.name;
    const rev = props.revision;
    status.value = '';
    error.value = '';
    working.value = true;
    try {
        const r = await callTool('project_export', { format: f });
        if (
            disposed ||
            token !== exportToken ||
            gen !== textGen ||
            f !== format.value ||
            rev !== props.revision ||
            name !== props.system.name
        )
            return;
        if (r.format !== f)
            throw new Error(tr('ie.formatMismatch', { actual: r.format, requested: f }));
        if (r.revision !== rev) {
            // The engine is ahead of the polled view (or behind it); keep the current text.
            status.value = tr('ie.revisionChanged');
            return;
        }
        if (r.name !== name) throw new Error(tr('ie.nameMismatch'));
        setText(r.text, { format: f, name: r.name, file: '' });
        status.value = tr('ie.exported', { format: f, characters: r.text.length.toLocaleString() });
    } catch (e) {
        if (!disposed && token === exportToken) error.value = errorMessage(e);
    } finally {
        if (token === exportToken) working.value = false;
    }
}

function download(): void {
    const name = downloadName.value;
    if (!text.value || !name) return;
    const url = URL.createObjectURL(new Blob([text.value], { type: 'application/json' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
    status.value = tr('ie.savedFile', { name: name });
    error.value = '';
}

async function onFile(e: Event): Promise<void> {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = ''; // the same file can be chosen again; the File object stays valid
    if (!file || locked.value) return;
    cancelFile(); // overlapping picks: the latest wins
    const token = ++fileToken;
    const gen = textGen;
    const f = format.value;
    cancelExport(tr('ie.cancelExportFile'));
    reading.value = true;
    status.value = tr('ie.reading', { name: file.name });
    error.value = '';
    try {
        const bytes = await file.arrayBuffer();
        if (disposed || token !== fileToken) return;
        let content: string;
        try {
            // Strict UTF-8 that keeps a leading BOM as U+FEFF instead of dropping or replacing anything.
            content = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
        } catch {
            error.value = tr('ie.notUtf8', { name: file.name });
            status.value = '';
            return;
        }
        if (gen !== textGen || f !== format.value) {
            status.value = tr('ie.fileStale', { name: file.name });
            return;
        }
        setText(content, { format: f, name: '', file: file.name });
        status.value = tr('ie.fileLoaded', { name: file.name });
    } catch (err) {
        if (!disposed && token === fileToken) {
            error.value = tr('agents.connected', { name: file.name, status: errorMessage(err) });
            status.value = '';
        }
    } finally {
        if (token === fileToken) reading.value = false;
    }
}

async function doImport(): Promise<void> {
    if (importBlock.value || locked.value) return;
    const body = text.value;
    const f = format.value;
    status.value = '';
    error.value = '';
    cancelFile();
    cancelExport();
    importing.value = true;
    try {
        const ok = await props.mutate((revision) =>
            callTool('project_import', { format: f, text: body, revision }),
        );
        if (disposed) return;
        if (ok) status.value = tr('ie.imported', { format: f });
        else error.value = tr('ie.importFailed');
    } finally {
        importing.value = false;
    }
}

// ---- new system ---------------------------------------------------------------

const newButton = ref<HTMLButtonElement | null>(null);
const cancelButton = ref<HTMLButtonElement | null>(null);

function openNew(): void {
    if (newBlock.value || locked.value) return;
    openedRevision = props.revision;
    status.value = '';
    error.value = '';
    confirming.value = true;
    void nextTick(() => cancelButton.value?.focus());
}
function cancelNew(note = ''): void {
    confirming.value = false;
    if (note) status.value = note;
    void nextTick(() => newButton.value?.focus());
}
// A confirmation never outlives the state it was opened in.
watch([newBlock, () => props.revision, () => props.system], () => {
    if (confirming.value && !creating.value) cancelNew(tr('ie.cancelNewWorkspace'));
});

async function createBlank(): Promise<void> {
    if (!confirming.value || creating.value) return;
    if (newBlock.value || locked.value || props.revision !== openedRevision) {
        cancelNew(tr('ie.cancelNewWorkspace'));
        return;
    }
    creating.value = true;
    status.value = '';
    error.value = '';
    cancelFile();
    cancelExport();
    try {
        const ok = await props.mutate((revision) => callTool('project_new', { revision }));
        if (disposed) return;
        confirming.value = false;
        if (ok) status.value = tr('ie.created');
        else error.value = tr('ie.createFailed');
    } finally {
        creating.value = false;
    }
}
// ---- workspace project files (multi-project open / save-as / recent) -------
const projects = ref<ProjectListResult | null>(null);
const projectPath = ref('');
const projectStatus = ref('');
const projectError = ref('');
const projectBusy = ref(false);
const projectPicking = ref(false);
const projectBlocked = computed(
    () => !!stateBlock.value || locked.value || projectBusy.value || projectPicking.value,
);

const desktop = inDesktop();
let dialogToken = 0;
let projectListToken = 0;

function isZmxPath(path: string): boolean {
    return /\.zmx$/i.test(path);
}

function fileName(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
}

async function refreshProjects(): Promise<void> {
    if (!props.online) return;
    const token = ++projectListToken;
    try {
        const list = await callTool('project_list', {});
        if (disposed || token !== projectListToken) return;
        projects.value = list;
        if (!projectPath.value && list.active_file) projectPath.value = list.active_file;
    } catch {
        // non-fatal
    }
}

async function runProjectPath<N extends 'project_open' | 'project_save_as'>(
    name: N,
    path: string,
    expected = props.revision,
): Promise<ToolResult<N> | null> {
    const trimmed = path.trim();
    if (!trimmed || disposed || projectBlocked.value) return null;
    projectError.value = '';
    projectStatus.value = '';
    if (name === 'project_save_as' && isZmxPath(trimmed)) {
        projectError.value = tr('proj.saveZmx');
        return null;
    }
    projectBusy.value = true;
    const result: { value: ToolResult<N> | null } = { value: null };
    try {
        const ok = await props.mutate(async (revision) => {
            if (disposed || revision !== expected) throw new Error(tr('proj.pickerStale'));
            result.value = await callTool(name, { path: trimmed, revision: expected });
        });
        if (!ok || disposed || !result.value) return null;
        projectPath.value = result.value.path;
        projectStatus.value = tr(name === 'project_open' ? 'proj.opened' : 'proj.saved', {
            path: result.value.path,
        });
        void refreshProjects();
        return result.value;
    } catch (e) {
        projectError.value = errorMessage(e);
        return null;
    } finally {
        projectBusy.value = false;
    }
}

async function browseProject(toSave: boolean): Promise<void> {
    if (!desktop || projectBlocked.value) return;
    const token = ++dialogToken;
    const revision = props.revision;
    const fileRequest = fileToken;
    const gen = textGen;
    const selectedFormat = format.value;
    const current = () => !disposed && token === dialogToken;
    const previewCurrent = () =>
        current() &&
        !reading.value &&
        !working.value &&
        fileRequest === fileToken &&
        gen === textGen &&
        selectedFormat === format.value;

    const typedPath = projectPath.value.trim();
    projectPicking.value = true;
    projectError.value = '';
    let path: string | null;
    try {
        path = toSave
            ? await saveDialog({
                  defaultPath:
                      typedPath && !isZmxPath(typedPath)
                          ? typedPath
                          : `${safeName(props.system.name)}.hissabeth.json`,
                  filters: [{ name: tr('proj.saveFilter'), extensions: ['json'] }],
              })
            : await openDialog({
                  filters: [{ name: tr('proj.openFilter'), extensions: ['json', 'zmx'] }],
              });
    } catch (e) {
        if (current()) projectError.value = tr('proj.pickerFailed', { error: errorMessage(e) });
        return;
    } finally {
        if (current()) projectPicking.value = false;
    }
    if (!current() || !path) return;
    if (revision !== props.revision || projectBlocked.value) {
        projectError.value = tr('proj.pickerStale');
        return;
    }
    if (toSave) {
        await runProjectPath('project_save_as', path, revision);
        return;
    }
    const opened = await runProjectPath('project_open', path, revision);
    if (!opened || !previewCurrent() || opened.revision !== props.revision) return;
    try {
        const content = await readTextFile(path);
        if (!previewCurrent() || opened.revision !== props.revision || stateBlock.value) return;
        const projectFormat = opened.linked ? 'hissabeth' : 'zmx';
        format.value = projectFormat;
        setText(content, {
            format: projectFormat,
            name: props.system.name,
            file: fileName(path),
        });
        status.value = tr('ie.fileLoaded', { name: fileName(path) });
        error.value = '';
    } catch {
        // the text preview is optional; the project itself is already open
    }
}

watch(
    () => [props.online, props.revision],
    () => {
        void refreshProjects();
    },
    { immediate: true },
);

defineExpose({ openNew });
</script>

<template>
    <section
        class="panel"
        aria-labelledby="ie-title"
        :aria-busy="working || reading || importing || creating || projectBusy || projectPicking"
    >
        <div class="panel-head">
            <h3 id="ie-title">{{ tr('ie.title') }}</h3>
            <span>{{ tr('ie.subtitle') }}</span>
        </div>
        <div class="subpanel">
            <h4>{{ tr('proj.title') }}</h4>
            <p class="note">
                {{ tr('proj.active') }}:
                <code>{{ projects?.active_file || projects?.workspace_file || '—' }}</code>
            </p>
            <div v-if="desktop" class="controls">
                <button type="button" :disabled="projectBlocked" @click="browseProject(false)">
                    <FolderOpen :size="14" /> {{ tr('proj.open') }}…
                </button>
                <button
                    type="button"
                    class="primary"
                    :disabled="projectBlocked"
                    @click="browseProject(true)"
                >
                    <SaveAll :size="14" /> {{ tr('proj.saveAs') }}…
                </button>
            </div>
            <div class="controls">
                <input
                    v-model="projectPath"
                    type="text"
                    class="project-path-input"
                    :placeholder="tr('proj.placeholder')"
                    :aria-label="tr('proj.placeholder')"
                    :disabled="locked || projectBusy || projectPicking"
                    @keyup.enter="runProjectPath('project_open', projectPath)"
                />
                <button
                    type="button"
                    :disabled="projectBlocked || !projectPath.trim()"
                    @click="runProjectPath('project_open', projectPath)"
                >
                    <FolderOpen :size="14" /> {{ tr('proj.open') }}
                </button>
                <button
                    type="button"
                    :class="{ primary: !desktop }"
                    :disabled="projectBlocked || !projectPath.trim()"
                    @click="runProjectPath('project_save_as', projectPath)"
                >
                    <SaveAll :size="14" /> {{ tr('proj.saveAs') }}
                </button>
            </div>
            <div v-if="projects?.saved_projects.length" class="recent-list">
                <span class="note">{{ tr('proj.savedDir') }}:</span>
                <button
                    v-for="p in projects.saved_projects"
                    :key="p"
                    type="button"
                    class="small"
                    :disabled="projectBlocked"
                    @click="runProjectPath('project_open', p)"
                >
                    {{ fileName(p) }}
                </button>
            </div>
            <div v-if="projects?.recent.length" class="recent-list">
                <span class="note">{{ tr('proj.recent') }}:</span>
                <button
                    v-for="p in projects.recent"
                    :key="p"
                    type="button"
                    class="small"
                    :title="p"
                    :disabled="projectBlocked"
                    @click="runProjectPath('project_open', p)"
                >
                    {{ fileName(p) }}
                </button>
            </div>
            <p v-if="projectStatus" role="status" class="note">{{ projectStatus }}</p>
            <p v-if="projectError" role="alert" class="json-error">{{ projectError }}</p>
        </div>
        <div class="analysis-controls">
            <div class="controls">
                <label
                    >{{ tr('ie.format') }}
                    <select v-model="format" :disabled="locked">
                        <option v-for="f in FORMATS" :key="f.id" :value="f.id">
                            {{ f.label }}
                        </option>
                    </select>
                </label>
                <label
                    >{{ tr('ie.openFile') }}
                    <input
                        type="file"
                        accept=".json,.txt,.zmx,application/json,text/plain"
                        :disabled="locked"
                        @change="onFile"
                /></label>
                <button
                    ref="newButton"
                    type="button"
                    :disabled="!!newBlock || locked || confirming"
                    @click="openNew"
                >
                    {{ tr('ie.newSystem') }}
                </button>
            </div>
            <p v-if="newBlock" class="note">{{ tr('fragments.newSystem') }} {{ newBlock }}</p>
        </div>
        <div v-if="confirming" class="subpanel" role="group" aria-labelledby="ie-new-title">
            <h4 id="ie-new-title">{{ tr('ie.confirmTitle') }}</h4>
            <p class="note">
                {{ tr('ie.confirmBody') }}
            </p>
            <p class="note">
                {{ tr('ie.confirmNote') }}
            </p>
            <div class="controls">
                <button
                    type="button"
                    class="primary"
                    :disabled="creating || locked || !!newBlock"
                    @click="createBlank"
                >
                    {{ tr('ie.confirmCreate') }}
                </button>
                <button ref="cancelButton" type="button" :disabled="creating" @click="cancelNew()">
                    {{ tr('ie.confirmCancel') }}
                </button>
            </div>
        </div>
        <div class="subpanel">
            <label for="ie-text" class="block-label">{{ tr('ie.projectText') }}</label>
            <textarea
                id="ie-text"
                :value="text"
                class="paste tall"
                spellcheck="false"
                :readonly="importing || creating"
                aria-describedby="ie-status ie-error"
                @input="onTextInput"
            />
            <div class="controls">
                <button
                    type="button"
                    :disabled="!!exportBlock || working || locked"
                    @click="doExport"
                >
                    {{ tr('ie.export') }}
                </button>
                <button type="button" :disabled="!text || !textFormat" @click="download">
                    {{ tr('ie.download') }}
                </button>
                <button
                    type="button"
                    class="primary"
                    :disabled="!!importBlock || locked"
                    @click="doImport"
                >
                    {{ tr('ie.import') }}
                </button>
            </div>
            <p v-if="format === 'geopter'" class="note">
                {{
                    tr(
                        'fragments.geopterFilesKeepTheObjectMediumEndpointLabelsEndpointProfilesAndAperturesAndFieldAndW',
                    )
                }}
            </p>
            <p v-if="format === 'zmx'" class="note">
                {{ tr('ie.zmxNote') }}
            </p>
            <p
                v-if="(format === 'geopter' || format === 'zmx') && online && exportBlock"
                class="note"
            >
                {{ exportBlock }}
            </p>
            <p v-if="importBlock" class="note">{{ importBlock }}</p>
            <p v-if="downloadNote" class="note">{{ downloadNote }}</p>
            <p v-if="textFormat && text" class="note">
                {{ tr('fragments.downloadWillBeNamed') }} {{ downloadName }}.
            </p>
            <p id="ie-status" role="status" class="note">
                {{
                    working
                        ? tr('ie.statusExporting')
                        : importing
                          ? tr('ie.statusImporting')
                          : creating
                            ? tr('ie.statusCreating')
                            : status
                }}
            </p>
            <p id="ie-error" role="alert" class="json-error">{{ error }}</p>
        </div>
    </section>
</template>
