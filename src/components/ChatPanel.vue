<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { usePreferredDark } from '@vueuse/core';
import {
    ArrowUp,
    ChevronRight,
    MessageSquare,
    Plug,
    Plus,
    RefreshCw,
    Square,
    X,
} from 'lucide-vue-next';
import ChatFigure from './ChatFigure.vue';
import ChatChecks from './ChatChecks.vue';
import MarkdownRender from 'markstream-vue';
import 'markstream-vue/index.css';
import { tr } from '../i18n';
import { usePreferencesStore } from '../stores/preferences';
import { chatProviderNames, useChatStore, type BenchTarget } from '../stores/chat';
import { openUrl } from '@tauri-apps/plugin-opener';
import chatgptBlack from '../assets/chatgpt-button-black-21.svg';
import chatgptWhite from '../assets/chatgpt-button-white-21.svg';

const props = defineProps<{ online: boolean; dirty: boolean; locked: boolean; visible: boolean }>();
const emit = defineEmits<{ settings: []; close: []; locate: [target: BenchTarget] }>();
const chat = useChatStore();
const feed = ref<HTMLElement>();
const composer = ref<HTMLTextAreaElement>();
const follows = ref(true);
const customModel = ref(false);
const modelChoice = computed(() =>
    customModel.value ||
    (chat.model && !chat.modelCatalog.models.some((model) => model.id === chat.model))
        ? '__custom__'
        : chat.model,
);
function chooseModel(event: Event): void {
    const value = (event.target as HTMLSelectElement).value;
    customModel.value = value === '__custom__';
    if (!customModel.value) chat.changeModel(value);
}
watch(
    () => chat.selectedId,
    () => {
        customModel.value = false;
    },
);
watch(
    () => chat.provider,
    () => {
        customModel.value = false;
        if (chat.provider === 'chatgpt') void chat.updateAccount();
        else void chat.loadModels();
    },
);
const entries = computed(() => chat.selected?.messages || []);
const lastEntryId = computed(() => entries.value[entries.value.length - 1]?.id);
const prefersDark = usePreferredDark();
const preferences = usePreferencesStore();
const isDark = computed(() => {
    const theme = preferences.preferences.appearance.theme;
    return theme === 'system' ? prefersDark.value : theme === 'dark';
});
const isChatGPT = computed(() => chat.provider === 'chatgpt');
const accountDisabled = computed(
    () => chat.busy || !!chat.accountAction || chat.modelCatalog.loading,
);
const blocked = computed(
    () => !chat.ready || (!isChatGPT.value && (!props.online || props.dirty || props.locked)),
);
async function openUsage(): Promise<void> {
    try {
        await openUrl('https://chatgpt.com/settings/usage');
    } catch {
        chat.accountError = tr('chat.chatgptUsageFailed');
    }
}
async function send(): Promise<void> {
    if (blocked.value) return;
    await chat.send();
    await nextTick();
    composer.value?.focus();
}
async function sendQuickPrompt(prompt: string): Promise<void> {
    if (blocked.value || chat.busy) return;
    chat.draft = prompt;
    await send();
}
const starterPrompts = computed(() => [
    tr('chat.starterBenchPbs'),
    tr('chat.starterLabAudit'),
    tr('chat.starterOptimize'),
    tr('chat.starterTolerance'),
]);
const outcomeText = computed(() =>
    chat.busy
        ? tr(chat.stopping ? 'chat.stopping' : 'chat.running')
        : chat.selected?.outcome === 'cancelled'
          ? tr(isChatGPT.value ? 'chat.chatgptStopped' : 'chat.cancelled')
          : chat.selected?.outcome === 'done'
            ? tr('chat.done')
            : '',
);
const outcomeTone = computed(() =>
    chat.busy
        ? 'running'
        : chat.selected?.outcome === 'cancelled'
          ? 'cancelled'
          : chat.selected?.outcome === 'done'
            ? 'done'
            : '',
);
function keydown(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        send();
    }
    if (event.key === 'Escape' && chat.busy) {
        event.preventDefault();
        void chat.cancel();
    }
}
function scrolled(): void {
    const node = feed.value;
    if (node) follows.value = node.scrollHeight - node.scrollTop - node.clientHeight < 64;
}
watch(
    () =>
        entries.value.map((entry) => `${entry.id}:${entry.text?.length}:${entry.status}`).join('|'),
    async () => {
        if (!follows.value) return;
        await nextTick();
        feed.value?.scrollTo({ top: feed.value.scrollHeight });
    },
);
watch(
    () => props.visible,
    async (visible) => {
        if (visible) {
            await nextTick();
            composer.value?.focus();
        }
    },
);
watch([() => chat.selectedId, () => chat.provider], async () => {
    follows.value = true;
    await nextTick();
    composer.value?.focus();
    feed.value?.scrollTo({ top: feed.value.scrollHeight });
});
onMounted(async () => {
    await chat.init();
    composer.value?.focus();
});
</script>

<template>
    <section class="chat-panel panel" :aria-label="tr('chat.title')">
        <header class="dock-heading chat-dock-heading">
            <span class="chat-dock-title">
                <MessageSquare :size="13" aria-hidden="true" />
                <span>{{ tr('chat.title') }}</span>
            </span>
            <div class="chat-dock-actions">
                <button
                    type="button"
                    class="chat-dock-btn"
                    :disabled="chat.busy"
                    :aria-label="tr('chat.new')"
                    :title="tr('chat.new')"
                    @click="chat.newConversation()"
                >
                    <Plus :size="14" aria-hidden="true" />
                </button>
                <button
                    type="button"
                    class="chat-dock-btn"
                    :aria-label="tr('chat.connections')"
                    :title="tr('chat.connections')"
                    @click="emit('settings')"
                >
                    <Plug :size="13" aria-hidden="true" />
                </button>
                <button
                    type="button"
                    class="chat-dock-btn"
                    :aria-label="tr('chat.close')"
                    :title="tr('chat.close')"
                    @click="emit('close')"
                >
                    <X :size="14" aria-hidden="true" />
                </button>
            </div>
        </header>

        <div class="chat-config">
            <div class="chat-config-row">
                <div class="chat-field">
                    <label for="chat-provider">{{ tr('chat.provider') }}</label>
                    <select
                        id="chat-provider"
                        v-model="chat.provider"
                        :disabled="chat.busy || !!chat.accountAction"
                        @change="chat.changeProvider()"
                    >
                        <option
                            v-for="(label, provider) in chatProviderNames"
                            :key="provider"
                            :value="provider"
                        >
                            {{ label }}
                        </option>
                    </select>
                </div>
                <div class="chat-field">
                    <label for="chat-model">{{ tr('chat.model') }}</label>
                    <select
                        id="chat-model"
                        :value="modelChoice"
                        :disabled="chat.busy"
                        @change="chooseModel"
                    >
                        <option value="">
                            {{ tr(isChatGPT ? 'chat.chatgptChooseModel' : 'chat.defaultModel') }}
                        </option>
                        <option
                            v-for="choice in chat.modelCatalog.models"
                            :key="choice.id"
                            :value="choice.id"
                            :title="choice.description"
                        >
                            {{ choice.label }}
                        </option>
                        <option value="__custom__">{{ tr('chat.customModel') }}</option>
                    </select>
                </div>
                <button
                    type="button"
                    class="chat-refresh-btn"
                    :disabled="
                        chat.busy ||
                        chat.modelCatalog.loading ||
                        (isChatGPT && !chat.account?.sharing)
                    "
                    :aria-label="tr('chat.refreshModels')"
                    :title="tr('chat.refreshModels')"
                    @click="chat.loadModels(true)"
                >
                    <RefreshCw
                        :size="13"
                        :class="{ 'is-spinning': chat.modelCatalog.loading }"
                        aria-hidden="true"
                    />
                </button>
            </div>

            <div v-if="modelChoice === '__custom__'" class="chat-field chat-custom-model">
                <label for="chat-model-id">{{ tr('chat.modelId') }}</label>
                <input
                    id="chat-model-id"
                    :value="chat.model"
                    :disabled="chat.busy"
                    :placeholder="tr('chat.modelId')"
                    maxlength="256"
                    @change="chat.changeModel(($event.target as HTMLInputElement).value.trim())"
                />
            </div>

            <div v-if="isChatGPT" class="chat-account">
                <div class="chat-account-actions">
                    <button
                        v-if="chat.account?.status !== 'connected'"
                        type="button"
                        class="chatgpt-sign-in"
                        :class="{ dark: isDark }"
                        :disabled="accountDisabled || !chat.runtime?.node"
                        @click="chat.updateAccount('sign-in')"
                    >
                        <img
                            :src="isDark ? chatgptWhite : chatgptBlack"
                            alt=""
                            width="16"
                            height="16"
                        />
                        {{ tr('chat.chatgptSignIn') }}
                    </button>
                    <template v-else>
                        <span class="chat-account-identity">
                            <img
                                :src="isDark ? chatgptWhite : chatgptBlack"
                                alt=""
                                width="16"
                                height="16"
                            />
                            <span>{{
                                chat.account.identity?.email ||
                                chat.account.identity?.name ||
                                'ChatGPT'
                            }}</span>
                        </span>
                        <div class="chat-account-buttons">
                            <button
                                v-if="!chat.account.sharing"
                                type="button"
                                :disabled="accountDisabled"
                                @click="chat.updateAccount('enable-plan')"
                            >
                                {{ tr('chat.chatgptEnablePlan') }}
                            </button>
                            <button v-else type="button" @click="openUsage">
                                {{ tr('chat.chatgptUsage') }}
                            </button>
                            <button
                                type="button"
                                :disabled="accountDisabled"
                                @click="chat.updateAccount('disconnect')"
                            >
                                {{ tr('chat.chatgptDisconnect') }}
                            </button>
                        </div>
                    </template>
                </div>
                <p
                    v-if="chat.accountAction === 'sign-in' || chat.accountAction === 'enable-plan'"
                    class="chat-inline-note"
                    role="status"
                >
                    {{ tr('chat.chatgptWaiting') }}
                </p>
                <p v-else-if="chat.accountError" class="chat-inline-note is-error" role="alert">
                    {{ chat.accountError }}
                </p>
                <p v-else-if="chat.account?.sharing" class="chat-inline-note" role="status">
                    {{ tr('chat.chatgptUsingPlan') }}
                </p>
            </div>

            <p v-if="chat.modelCatalog.loading" class="chat-model-status" role="status">
                {{ tr('chat.loadingModels') }}
            </p>
            <p v-else-if="chat.modelCatalog.error" class="chat-model-status is-error" role="alert">
                {{ tr('chat.modelsFailed') }} {{ chat.modelCatalog.error }}
            </p>
            <p
                v-else-if="chat.modelCatalog.loaded && !chat.modelCatalog.models.length"
                class="chat-model-status"
                role="status"
            >
                {{ tr(isChatGPT ? 'chat.chatgptNoModels' : 'chat.noModels') }}
            </p>
        </div>

        <details class="chat-history" :aria-label="tr('chat.history')">
            <summary class="chat-history-summary">
                <ChevronRight :size="13" class="chat-chevron" aria-hidden="true" />
                <span class="chat-history-label">{{ tr('chat.history') }}</span>
                <span class="chat-history-count">{{ chat.history.length }}</span>
            </summary>
            <div class="chat-history-body">
                <p v-if="!chat.history.length" class="chat-history-empty">
                    {{ tr(chat.loaded ? 'chat.noHistory' : 'chat.loading') }}
                </p>
                <ul v-else>
                    <li v-for="record in chat.history" :key="record.id">
                        <button
                            type="button"
                            :disabled="chat.busy"
                            :aria-pressed="chat.selectedId === record.id"
                            @click="chat.select(record.id)"
                        >
                            <span class="chat-history-title">{{ record.title }}</span>
                            <small class="chat-history-meta">
                                <span>{{ chatProviderNames[record.provider] }}</span>
                                <span aria-hidden="true">·</span>
                                <time>{{ new Date(record.updatedAt).toLocaleString() }}</time>
                            </small>
                        </button>
                    </li>
                </ul>
            </div>
        </details>

        <div
            v-if="
                chat.storageError ||
                (chat.runtime &&
                    (!chat.runtime.node ||
                        (!isChatGPT &&
                            !chat.runtime.agents.find((agent) => agent.name === chat.provider)
                                ?.installed))) ||
                (dirty && !isChatGPT)
            "
            class="chat-banners"
        >
            <div v-if="chat.storageError" class="banner error" role="alert">
                {{ chat.storageError }}
            </div>
            <div
                v-if="
                    chat.runtime &&
                    (!chat.runtime.node ||
                        (!isChatGPT &&
                            !chat.runtime.agents.find((agent) => agent.name === chat.provider)
                                ?.installed))
                "
                class="banner warn"
                role="status"
            >
                <span>{{ tr(isChatGPT ? 'chat.chatgptRuntime' : 'chat.runtime') }}</span>
                <button type="button" @click="chat.detect()">{{ tr('chat.detect') }}</button>
            </div>
            <div v-if="dirty && !isChatGPT" class="banner warn" role="status">
                {{ tr('chat.dirty') }}
            </div>
        </div>

        <div
            ref="feed"
            class="chat-feed"
            role="log"
            :aria-label="tr('chat.transcript')"
            aria-live="polite"
            :aria-busy="chat.busy"
            tabindex="0"
            @scroll="scrolled"
        >
            <div v-if="!entries.length" class="chat-empty-state">
                <p class="empty">
                    {{ tr(isChatGPT ? 'chat.chatgptEmpty' : 'chat.empty') }}
                </p>
                <p class="chat-context">
                    {{ tr(isChatGPT ? 'chat.chatgptContext' : 'chat.context') }}
                </p>
                <div v-if="!isChatGPT" class="starter-chips">
                    <button
                        v-for="(prompt, idx) in starterPrompts"
                        :key="idx"
                        type="button"
                        class="starter-chip"
                        :disabled="blocked || chat.busy"
                        @click="sendQuickPrompt(prompt)"
                    >
                        <span class="starter-chip-text">{{ prompt }}</span>
                        <ChevronRight :size="13" class="starter-chip-arrow" aria-hidden="true" />
                    </button>
                </div>
            </div>

            <article
                v-for="entry in entries"
                :key="entry.id"
                class="chat-entry"
                :class="entry.kind"
            >
                <template v-if="entry.kind !== 'tool'">
                    <div class="chat-entry-head">
                        <span
                            v-if="entry.kind === 'text'"
                            class="chat-role-dot"
                            aria-hidden="true"
                        />
                        <strong class="chat-entry-role">{{
                            entry.kind === 'user'
                                ? tr('chat.you')
                                : chatProviderNames[chat.provider]
                        }}</strong>
                    </div>
                    <MarkdownRender
                        v-if="entry.kind === 'text'"
                        class="chat-text"
                        mode="chat"
                        :content="entry.text || ''"
                        :final="!chat.busy || entry.id !== lastEntryId"
                        :is-dark="isDark"
                    />
                    <div v-else class="chat-text">{{ entry.text }}</div>
                </template>
                <details v-else class="chat-tool">
                    <summary class="chat-tool-summary">
                        <ChevronRight :size="12" class="chat-chevron" aria-hidden="true" />
                        <code class="chat-tool-name">{{ entry.name }}</code>
                        <span
                            class="chat-tool-status"
                            :class="`is-${entry.status || 'in_progress'}`"
                        >
                            {{ tr(`chat.${entry.status || 'in_progress'}`) }}
                        </span>
                    </summary>
                    <pre class="chat-tool-detail">{{ entry.detail }}</pre>
                </details>
                <ChatChecks
                    v-if="entry.kind === 'tool' && entry.checks"
                    :checks="entry.checks"
                    @locate="emit('locate', $event)"
                />
                <ChatFigure
                    v-if="entry.kind === 'tool' && entry.figure"
                    :figure="entry.figure"
                    :tool="entry.name"
                    @locate="emit('locate', {})"
                />
            </article>
        </div>

        <footer class="chat-footer">
            <div v-if="chat.error" class="banner error chat-footer-banner" role="alert">
                {{ chat.error }}
            </div>
            <p
                v-show="outcomeText"
                class="chat-outcome"
                :class="outcomeTone && `is-${outcomeTone}`"
                role="status"
            >
                <span class="chat-outcome-dot" aria-hidden="true" />
                <span>{{ outcomeText }}</span>
            </p>
            <form class="chat-composer" @submit.prevent="send">
                <label class="sr-only" for="agent-message">{{ tr('chat.message') }}</label>
                <div class="composer-box">
                    <textarea
                        id="agent-message"
                        ref="composer"
                        v-model="chat.draft"
                        :placeholder="
                            tr(isChatGPT ? 'chat.chatgptPlaceholder' : 'chat.placeholder')
                        "
                        rows="3"
                        maxlength="16000"
                        @keydown="keydown"
                    />
                    <div class="chat-actions">
                        <small class="chat-keys">{{ tr('chat.keys') }}</small>
                        <button
                            v-if="chat.busy"
                            type="button"
                            class="chat-stop-btn"
                            :disabled="chat.stopping"
                            @click="chat.cancel()"
                        >
                            <Square :size="11" aria-hidden="true" />
                            {{ tr('chat.stop') }}
                        </button>
                        <button
                            v-else
                            type="submit"
                            class="primary chat-send-btn"
                            :disabled="blocked || !chat.draft.trim()"
                        >
                            <ArrowUp :size="13" aria-hidden="true" />
                            {{ tr('chat.send') }}
                        </button>
                    </div>
                </div>
            </form>
        </footer>
    </section>
</template>

<style scoped>
.chat-panel {
    --chat-pad: 14px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    width: 100%;
    margin: 0;
    border: 0;
    border-left: 1px solid var(--border);
    border-radius: 0;
    box-shadow: none;
    background: var(--panel);
}

/* ---- dock header -------------------------------------------------------- */
.chat-dock-heading {
    padding: 0 8px 0 var(--chat-pad);
    gap: 8px;
}
.chat-dock-title {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--text);
    font-weight: 600;
}
.chat-dock-title svg {
    width: 13px;
    height: 13px;
    color: var(--muted);
    flex: none;
}
.chat-dock-actions {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    flex: none;
}
.chat-dock-btn {
    width: 24px;
    height: 24px;
    min-height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
}
.chat-dock-btn:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
}
.chat-dock-btn svg {
    width: 14px;
    height: 14px;
}

/* ---- agent & model config strip ----------------------------------------- */
.chat-config {
    flex: none;
    padding: 8px var(--chat-pad);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    display: flex;
    flex-direction: column;
    gap: 8px;
}
.chat-config-row {
    display: grid;
    grid-template-columns: minmax(90px, 0.78fr) minmax(0, 1.4fr) auto;
    gap: 8px;
    align-items: end;
}
.chat-field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
}
.chat-field label {
    font-size: 11px;
    font-weight: 500;
    color: var(--muted);
    line-height: 1.2;
}
.chat-field select,
.chat-field input {
    width: 100%;
    min-width: 0;
    height: 26px;
    min-height: 26px;
    padding: 2px 7px;
    font-size: 12px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--panel);
    color: var(--text);
}
.chat-custom-model input {
    font-family: var(--font-mono);
}
.chat-refresh-btn {
    width: 26px;
    height: 26px;
    min-height: 26px;
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--panel);
    color: var(--muted);
    flex: none;
}
.chat-refresh-btn:hover:not(:disabled) {
    color: var(--text);
    background: var(--hover);
}
.chat-refresh-btn svg {
    width: 13px;
    height: 13px;
}
.is-spinning {
    animation: chat-spin 0.9s linear infinite;
}
@keyframes chat-spin {
    to {
        transform: rotate(360deg);
    }
}

/* ---- chatgpt account & model status ------------------------------------- */
.chat-account {
    padding-top: 6px;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 4px;
}
.chat-account-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
}
.chat-account-identity {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-size: 12px;
    font-weight: 500;
    overflow-wrap: anywhere;
}
.chat-account-buttons {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
}
.chat-account-buttons button {
    min-height: 24px;
    padding: 2px 8px;
    font-size: 11px;
}
.chatgpt-sign-in {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    padding: 3px 10px;
    font-size: 12px;
    background: #fff;
    color: #0d0d0d;
    border-color: #0d0d0d;
}
.chatgpt-sign-in.dark {
    background: #0d0d0d;
    color: #fff;
    border-color: #fff;
}
.chat-inline-note,
.chat-model-status {
    margin: 0;
    font-size: 11px;
    line-height: 1.45;
    color: var(--muted);
    overflow-wrap: anywhere;
}
.chat-inline-note.is-error,
.chat-model-status.is-error {
    color: var(--danger);
}

/* ---- collapsible history ------------------------------------------------ */
.chat-history {
    flex: none;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
}
.chat-history-summary {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px var(--chat-pad);
    cursor: pointer;
    user-select: none;
    list-style: none;
    font-size: 12px;
    font-weight: 550;
    color: var(--muted);
}
.chat-history-summary::-webkit-details-marker {
    display: none;
}
.chat-history-summary:hover {
    color: var(--text);
    background: var(--surface);
}
.chat-chevron {
    width: 13px;
    height: 13px;
    flex: none;
    color: var(--muted);
    transition: transform 0.14s ease;
}
details[open] > summary .chat-chevron {
    transform: rotate(90deg);
}
.chat-history-label {
    flex: 1;
    min-width: 0;
}
.chat-history-count {
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    padding: 0 6px;
    line-height: 17px;
    border-radius: 99px;
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--muted);
}
.chat-history-body {
    border-top: 1px solid var(--border);
    background: var(--surface);
    padding: 6px;
}
.chat-history-empty {
    margin: 0;
    padding: 6px 6px;
    font-size: 11px;
    color: var(--muted);
}
.chat-history ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 164px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
}
.chat-history li button {
    display: flex;
    width: 100%;
    min-height: 0;
    align-items: flex-start;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    font-weight: 400;
}
.chat-history li button:hover:not(:disabled) {
    background: var(--hover);
    border-color: transparent;
}
.chat-history li button[aria-pressed='true'] {
    background: var(--accent-soft);
    border-color: transparent;
    box-shadow: inset 2px 0 0 var(--accent);
}
.chat-history-title {
    width: 100%;
    font-size: 12px;
    font-weight: 500;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}
.chat-history-meta {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
}

/* ---- banners ------------------------------------------------------------ */
.chat-banners {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px var(--chat-pad) 0;
}
.chat-panel .banner {
    margin: 0;
    padding: 7px 10px;
    font-size: 12px;
    line-height: 1.45;
    gap: 8px;
    justify-content: space-between;
}
.chat-panel .banner button {
    min-height: 24px;
    padding: 2px 8px;
    font-size: 11px;
}

/* ---- transcript feed ---------------------------------------------------- */
.chat-feed {
    flex: 1;
    min-height: 70px;
    overflow-y: auto;
    padding: 12px var(--chat-pad);
    display: flex;
    flex-direction: column;
    gap: 12px;
    overscroll-behavior: contain;
}

/* ---- empty state -------------------------------------------------------- */
.chat-empty-state {
    padding: 4px 0;
}
.chat-empty-state .empty {
    padding: 0;
    margin: 0 0 4px;
    font-size: 13px;
    font-weight: 600;
    line-height: 1.45;
    color: var(--text);
}
.chat-empty-state .chat-context {
    margin: 0 0 14px;
    font-size: 12px;
    line-height: 1.5;
    color: var(--muted);
}
.starter-chips {
    display: flex;
    flex-direction: column;
    gap: 6px;
}
.starter-chip {
    width: 100%;
    min-height: 0;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
    text-align: left;
    padding: 9px 11px;
    font-size: 12px;
    line-height: 1.45;
    font-weight: 450;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
}
.starter-chip-text {
    flex: 1;
    min-width: 0;
}
.starter-chip-arrow {
    width: 13px;
    height: 13px;
    flex: none;
    margin-top: 2px;
    color: var(--muted);
    opacity: 0.6;
    transition:
        transform 0.12s ease,
        opacity 0.12s ease,
        color 0.12s ease;
}
.starter-chip:hover:not(:disabled) {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
}
.starter-chip:hover:not(:disabled) .starter-chip-arrow {
    color: var(--accent);
    opacity: 1;
    transform: translateX(2px);
}

/* ---- chat entries ------------------------------------------------------- */
.chat-entry {
    min-width: 0;
}
.chat-entry-head {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
}
.chat-role-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    flex: none;
}
.chat-entry-role {
    font-size: 11px;
    font-weight: 600;
    color: var(--muted);
    line-height: 1.2;
}
.chat-entry.user {
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 2px solid var(--accent);
    border-radius: var(--radius);
    padding: 9px 12px;
}
.chat-entry.text {
    padding: 2px 0;
}
.chat-text {
    overflow-wrap: anywhere;
    font-size: var(--ui-font-size);
    line-height: 1.6;
    color: var(--text);
    user-select: text;
}
div.chat-text {
    white-space: pre-wrap;
}

/* ---- normalized markstream-vue typography ------------------------------- */
.chat-text.markstream-vue {
    font-size: var(--ui-font-size);
    line-height: 1.6;
    background: transparent;
    color: var(--text);
}
.chat-text.markstream-vue :deep(> :first-child) {
    margin-top: 0;
}
.chat-text.markstream-vue :deep(> :last-child) {
    margin-bottom: 0;
}
.chat-text.markstream-vue :deep(p) {
    margin: 0 0 8px;
    font-size: inherit;
    line-height: 1.6;
}
.chat-text.markstream-vue :deep(h1) {
    font-size: 15px;
    font-weight: 650;
    line-height: 1.35;
    margin: 12px 0 6px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--border);
}
.chat-text.markstream-vue :deep(h2) {
    font-size: 14px;
    font-weight: 650;
    line-height: 1.35;
    margin: 10px 0 5px;
}
.chat-text.markstream-vue :deep(h3),
.chat-text.markstream-vue :deep(h4) {
    font-size: 13px;
    font-weight: 600;
    line-height: 1.4;
    margin: 8px 0 4px;
}
.chat-text.markstream-vue :deep(ul),
.chat-text.markstream-vue :deep(ol) {
    margin: 4px 0 8px;
    padding-left: 18px;
}
.chat-text.markstream-vue :deep(li) {
    margin: 2px 0;
    line-height: 1.55;
}
.chat-text.markstream-vue :deep(code) {
    font-family: var(--font-mono);
    font-size: 12px;
    padding: 1px 4px;
    border-radius: 3px;
    background: var(--surface);
    border: 1px solid var(--border);
}
.chat-text.markstream-vue :deep(pre) {
    margin: 8px 0;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--surface);
    border: 1px solid var(--border);
    font-size: 12px;
    line-height: 1.45;
    overflow-x: auto;
}
.chat-text.markstream-vue :deep(pre code) {
    padding: 0;
    border: 0;
    background: transparent;
}
.chat-text.markstream-vue :deep(blockquote) {
    margin: 8px 0;
    padding: 2px 0 2px 10px;
    border-left: 2px solid var(--border-strong);
    color: var(--muted);
}
.chat-text.markstream-vue :deep(hr) {
    border: 0;
    border-top: 1px solid var(--border);
    margin: 10px 0;
}
.chat-text.markstream-vue :deep(table) {
    width: 100%;
    font-size: 12px;
    margin: 8px 0;
    border-collapse: collapse;
}
.chat-text.markstream-vue :deep(th),
.chat-text.markstream-vue :deep(td) {
    padding: 4px 8px;
    border: 1px solid var(--border);
}

/* ---- tool activity card ------------------------------------------------- */
.chat-tool {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
}
.chat-tool-summary {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    cursor: pointer;
    user-select: none;
    list-style: none;
    font-size: 12px;
}
.chat-tool-summary::-webkit-details-marker {
    display: none;
}
.chat-tool-summary:hover {
    background: var(--hover);
}
.chat-tool-name {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--text);
    overflow-wrap: anywhere;
}
.chat-tool-status {
    flex: none;
    font-size: 11px;
    font-weight: 500;
    padding: 1px 6px;
    border-radius: 99px;
    background: var(--panel);
    border: 1px solid var(--border);
    color: var(--muted);
}
.chat-tool-status.is-completed {
    color: var(--positive);
}
.chat-tool-status.is-in_progress {
    color: var(--warning);
}
.chat-tool-status.is-failed {
    color: var(--danger);
}
.chat-tool-detail {
    margin: 0;
    padding: 8px 10px;
    border-top: 1px solid var(--border);
    background: var(--panel);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 200px;
    overflow: auto;
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--muted);
}

/* ---- footer & unified composer ------------------------------------------ */
.chat-footer {
    flex: none;
    border-top: 1px solid var(--border);
    background: var(--panel);
    padding: 8px var(--chat-pad) 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
}
.chat-outcome {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    line-height: 1.3;
    color: var(--muted);
}
.chat-outcome-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
}
.chat-outcome.is-running .chat-outcome-dot {
    background: var(--warning);
}
.chat-outcome.is-done .chat-outcome-dot {
    background: var(--positive);
}
.chat-outcome.is-cancelled .chat-outcome-dot {
    background: var(--muted);
}
.composer-box {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    transition:
        border-color 0.12s ease,
        box-shadow 0.12s ease,
        background-color 0.12s ease;
}
.composer-box:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
    background: var(--panel);
}
.composer-box textarea {
    display: block;
    width: 100%;
    resize: none;
    max-height: 180px;
    min-height: 58px;
    background: transparent;
    color: var(--text);
    border: 0;
    border-radius: var(--radius) var(--radius) 0 0;
    padding: 9px 11px 4px;
    line-height: 1.5;
    outline: none;
}
.composer-box textarea:focus-visible {
    outline: none;
}
.chat-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: space-between;
    padding: 4px 8px 7px 11px;
}
.chat-keys {
    font-size: 11px;
    color: var(--muted);
    line-height: 1.3;
    min-width: 0;
}
.chat-send-btn,
.chat-stop-btn {
    flex: none;
    min-height: 26px;
    padding: 3px 10px;
    font-size: 12px;
    border-radius: var(--radius-sm);
}
.chat-send-btn svg,
.chat-stop-btn svg {
    width: 12px;
    height: 12px;
}
.chat-stop-btn {
    color: var(--danger);
    border-color: var(--danger);
    background: var(--danger-bg);
}
.chat-stop-btn:hover:not(:disabled) {
    background: var(--danger);
    color: #fff;
    border-color: var(--danger);
}

@media (max-width: 420px) {
    .chat-config-row {
        grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto;
    }
}
</style>
