<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { Download, Expand, LocateFixed, Maximize2, X, ZoomIn, ZoomOut } from 'lucide-vue-next';
import { tr } from '../i18n';
import type { ChatFigure } from '../stores/chat';

const props = defineProps<{ figure: ChatFigure; tool?: string }>();
const emit = defineEmits<{ locate: [] }>();

const SHOWN = new Set(['image/svg+xml', 'image/png', 'image/jpeg', 'image/webp', 'image/gif']);
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;

// A drawing too large to inline is read back from the copy the bridge stored.
const loaded = ref('');
const loadError = ref('');
watch(
    () => props.figure.file,
    async (file) => {
        loaded.value = '';
        loadError.value = '';
        if (props.figure.data || !file) return;
        try {
            loaded.value = await invoke<string>('chat_figure', { file });
        } catch (error) {
            loadError.value = String(error);
        }
    },
    { immediate: true },
);
const data = computed(() => props.figure.data ?? loaded.value);
const loading = computed(
    () => !props.figure.data && !!props.figure.file && !loaded.value && !loadError.value,
);
// The drawing is shown through <img>, so SVG scripts, handlers and external
// references never run or load; only allow-listed image types and clean
// base64 make it into the data URL.
const source = computed(() => {
    const { mime } = props.figure;
    return data.value && SHOWN.has(mime) && BASE64.test(data.value)
        ? `data:${mime};base64,${data.value}`
        : '';
});
const origin = computed(() => {
    const { revision, saved } = props.figure;
    if (revision === undefined) return '';
    return saved === false
        ? tr('chat.figureDraft', { revision })
        : tr('chat.figureRevision', { revision });
});
const size = computed(() => {
    const kb = props.figure.bytes / 1024;
    return kb < 1024 ? `${kb.toFixed(0)} KB` : `${(kb / 1024).toFixed(1)} MB`;
});

const zoom = ref(1);
const pan = ref<[number, number]>([0, 0]);
const full = ref(false);
const dialog = ref<HTMLElement>();
let drag: { x: number; y: number; pan: [number, number] } | null = null;

function reset(): void {
    zoom.value = 1;
    pan.value = [0, 0];
}
function scaleBy(factor: number): void {
    zoom.value = Math.min(12, Math.max(0.25, zoom.value * factor));
}
function wheel(event: WheelEvent): void {
    scaleBy(event.deltaY < 0 ? 1.15 : 1 / 1.15);
}
function down(event: PointerEvent): void {
    drag = { x: event.clientX, y: event.clientY, pan: [...pan.value] };
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
}
function move(event: PointerEvent): void {
    if (!drag) return;
    pan.value = [drag.pan[0] + event.clientX - drag.x, drag.pan[1] + event.clientY - drag.y];
}
function up(): void {
    drag = null;
}
function download(): void {
    const { mime, filename } = props.figure;
    if (!data.value) return;
    const bytes = Uint8Array.from(atob(data.value), (c) => c.charCodeAt(0));
    const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
    const link = document.createElement('a');
    link.href = url;
    link.download = filename;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
}
function key(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
        event.stopPropagation();
        full.value = false;
    }
}
watch(full, async (open) => {
    reset();
    if (open) {
        await nextTick();
        dialog.value?.focus();
    }
});
onBeforeUnmount(up);
const transform = computed(
    () => `translate(${pan.value[0]}px, ${pan.value[1]}px) scale(${zoom.value})`,
);
</script>

<template>
    <figure class="chat-figure" :data-tool="tool" :data-file="figure.file">
        <div v-if="source" class="chat-figure-toolbar">
            <span class="chat-figure-name" :title="figure.filename">{{ figure.filename }}</span>
            <span v-if="origin" class="chat-figure-origin">{{ origin }}</span>
            <button
                v-if="tool"
                type="button"
                class="small"
                :aria-label="tr('chat.figureLocate')"
                :title="tr('chat.figureLocate')"
                @click="emit('locate')"
            >
                <LocateFixed :size="12" />
            </button>
            <button
                type="button"
                class="small"
                :aria-label="tr('chat.figureZoomIn')"
                @click="scaleBy(1.25)"
            >
                <ZoomIn :size="12" />
            </button>
            <button
                type="button"
                class="small"
                :aria-label="tr('chat.figureZoomOut')"
                @click="scaleBy(0.8)"
            >
                <ZoomOut :size="12" />
            </button>
            <button type="button" class="small" :aria-label="tr('chat.figureFit')" @click="reset">
                <Maximize2 :size="12" />
            </button>
            <button
                type="button"
                class="small chat-figure-full"
                :aria-label="tr('chat.figureFullscreen')"
                @click="full = true"
            >
                <Expand :size="12" />
            </button>
            <button
                type="button"
                class="small chat-figure-download"
                :aria-label="tr('chat.figureDownload')"
                @click="download"
            >
                <Download :size="12" />
            </button>
        </div>
        <div
            v-if="source && !full"
            class="chat-figure-view"
            @wheel.prevent="wheel"
            @pointerdown="down"
            @pointermove="move"
            @pointerup="up"
            @pointercancel="up"
            @dblclick="reset"
        >
            <img :src="source" :alt="figure.filename" :style="{ transform }" draggable="false" />
        </div>
        <figcaption v-if="loading" class="chat-figure-omitted">
            {{ tr('chat.figureLoading', { name: figure.filename, size }) }}
        </figcaption>
        <figcaption v-else-if="loadError" class="chat-figure-omitted">
            {{ tr('chat.figureLoadFailed', { name: figure.filename, error: loadError }) }}
        </figcaption>
        <figcaption v-else-if="!source" class="chat-figure-omitted">
            {{ tr('chat.figureOmitted', { name: figure.filename, size }) }}
        </figcaption>
        <div
            v-if="source && full"
            ref="dialog"
            class="chat-figure-dialog"
            role="dialog"
            aria-modal="true"
            :aria-label="figure.filename"
            tabindex="-1"
            @keydown="key"
        >
            <div class="chat-figure-toolbar">
                <span class="chat-figure-name">{{ figure.filename }}</span>
                <button
                    type="button"
                    class="small"
                    :aria-label="tr('chat.figureZoomIn')"
                    @click="scaleBy(1.25)"
                >
                    <ZoomIn :size="12" />
                </button>
                <button
                    type="button"
                    class="small"
                    :aria-label="tr('chat.figureZoomOut')"
                    @click="scaleBy(0.8)"
                >
                    <ZoomOut :size="12" />
                </button>
                <button
                    type="button"
                    class="small"
                    :aria-label="tr('chat.figureFit')"
                    @click="reset"
                >
                    <Maximize2 :size="12" />
                </button>
                <button
                    type="button"
                    class="small"
                    :aria-label="tr('chat.figureDownload')"
                    @click="download"
                >
                    <Download :size="12" />
                </button>
                <button
                    type="button"
                    class="small chat-figure-close"
                    :aria-label="tr('chat.figureClose')"
                    @click="full = false"
                >
                    <X :size="12" />
                </button>
            </div>
            <div
                class="chat-figure-view is-full"
                @wheel.prevent="wheel"
                @pointerdown="down"
                @pointermove="move"
                @pointerup="up"
                @pointercancel="up"
                @dblclick="reset"
            >
                <img
                    :src="source"
                    :alt="figure.filename"
                    :style="{ transform }"
                    draggable="false"
                />
            </div>
        </div>
    </figure>
</template>

<style scoped>
.chat-figure {
    margin: 6px 0 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    overflow: hidden;
}
.chat-figure-toolbar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 6px;
    border-bottom: 1px solid var(--border);
}
.chat-figure-toolbar button {
    flex: none;
}
.chat-figure-name {
    flex: 1 1 40%;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: var(--muted);
}
.chat-figure-origin {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: var(--muted);
}
.chat-figure-view {
    height: 240px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
    touch-action: none;
    background: #fff;
}
.chat-figure-view img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    transform-origin: center;
    user-select: none;
}
.chat-figure-omitted {
    padding: 8px 10px;
    font-size: 12px;
    color: var(--muted);
}
.chat-figure-dialog {
    position: fixed;
    inset: 24px;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    box-shadow: 0 12px 48px rgb(0 0 0 / 35%);
    outline: none;
}
.chat-figure-view.is-full {
    flex: 1;
    height: auto;
}
</style>
