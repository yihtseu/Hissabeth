<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { usePreferredDark } from '@vueuse/core';
import MarkdownRender from 'markstream-vue';
import 'markstream-vue/index.css';
import { tr } from '../i18n';
import { usePreferencesStore } from '../stores/preferences';
import { chatProviderNames, useChatStore } from '../stores/chat';
import { openUrl } from '@tauri-apps/plugin-opener';
import chatgptBlack from '../assets/chatgpt-button-black-21.svg';
import chatgptWhite from '../assets/chatgpt-button-white-21.svg';

const props = defineProps<{ online: boolean; dirty: boolean; locked: boolean; visible: boolean }>();
const emit = defineEmits<{ settings: []; close: [] }>();
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
        <div class="dock-heading">
            {{ tr('chat.title')
            }}<button :aria-label="tr('chat.close')" @click="emit('close')">×</button>
        </div>
        <div class="panel-head chat-head">
            <label
                >{{ tr('chat.provider') }}
                <select
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
            </label>
            <label
                >{{ tr('chat.model') }}
                <select :value="modelChoice" :disabled="chat.busy" @change="chooseModel">
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
            </label>
            <label v-if="modelChoice === '__custom__'"
                >{{ tr('chat.modelId') }}
                <input
                    :value="chat.model"
                    :disabled="chat.busy"
                    :placeholder="tr('chat.modelId')"
                    maxlength="256"
                    @change="chat.changeModel(($event.target as HTMLInputElement).value.trim())"
                />
            </label>
            <button
                :disabled="
                    chat.busy || chat.modelCatalog.loading || (isChatGPT && !chat.account?.sharing)
                "
                @click="chat.loadModels(true)"
            >
                {{ tr('chat.refreshModels') }}
            </button>
            <button :disabled="chat.busy" @click="chat.newConversation()">
                {{ tr('chat.new') }}
            </button>
            <button @click="emit('settings')">{{ tr('chat.connections') }}</button>
        </div>
        <div v-if="isChatGPT" class="chat-account">
            <div class="chat-account-actions">
                <button
                    v-if="chat.account?.status !== 'connected'"
                    class="chatgpt-sign-in"
                    :class="{ dark: isDark }"
                    :disabled="accountDisabled || !chat.runtime?.node"
                    @click="chat.updateAccount('sign-in')"
                >
                    <img
                        :src="isDark ? chatgptWhite : chatgptBlack"
                        alt=""
                        width="21"
                        height="21"
                    />
                    {{ tr('chat.chatgptSignIn') }}
                </button>
                <template v-else>
                    <span class="chat-account-identity">
                        <img
                            :src="isDark ? chatgptWhite : chatgptBlack"
                            alt=""
                            width="21"
                            height="21"
                        />
                        {{
                            chat.account.identity?.email || chat.account.identity?.name || 'ChatGPT'
                        }}
                    </span>
                    <button
                        v-if="!chat.account.sharing"
                        :disabled="accountDisabled"
                        @click="chat.updateAccount('enable-plan')"
                    >
                        {{ tr('chat.chatgptEnablePlan') }}
                    </button>
                    <button v-else @click="openUsage">{{ tr('chat.chatgptUsage') }}</button>
                    <button :disabled="accountDisabled" @click="chat.updateAccount('disconnect')">
                        {{ tr('chat.chatgptDisconnect') }}
                    </button>
                </template>
            </div>
            <p
                v-if="chat.accountAction === 'sign-in' || chat.accountAction === 'enable-plan'"
                role="status"
            >
                {{ tr('chat.chatgptWaiting') }}
            </p>
            <p v-else-if="chat.accountError" role="alert">{{ chat.accountError }}</p>
            <p v-else-if="chat.account?.sharing" role="status">{{ tr('chat.chatgptUsingPlan') }}</p>
        </div>
        <p v-if="chat.modelCatalog.loading" class="chat-model-status" role="status">
            {{ tr('chat.loadingModels') }}
        </p>
        <p v-else-if="chat.modelCatalog.error" class="chat-model-status" role="alert">
            {{ tr('chat.modelsFailed') }} {{ chat.modelCatalog.error }}
        </p>
        <p
            v-else-if="chat.modelCatalog.loaded && !chat.modelCatalog.models.length"
            class="chat-model-status"
            role="status"
        >
            {{ tr(isChatGPT ? 'chat.chatgptNoModels' : 'chat.noModels') }}
        </p>
        <details class="chat-history" :aria-label="tr('chat.history')">
            <summary>{{ tr('chat.history') }} ({{ chat.history.length }})</summary>
            <p v-if="!chat.history.length" class="chat-history-empty">
                {{ tr(chat.loaded ? 'chat.noHistory' : 'chat.loading') }}
            </p>
            <ul v-else>
                <li v-for="record in chat.history" :key="record.id">
                    <button
                        :disabled="chat.busy"
                        :aria-pressed="chat.selectedId === record.id"
                        @click="chat.select(record.id)"
                    >
                        <span>{{ record.title }}</span
                        ><small
                            >{{ chatProviderNames[record.provider] }} ·
                            {{ new Date(record.updatedAt).toLocaleString() }}</small
                        >
                    </button>
                </li>
            </ul>
        </details>
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
            {{ tr(isChatGPT ? 'chat.chatgptRuntime' : 'chat.runtime')
            }}<button @click="chat.detect()">{{ tr('chat.detect') }}</button>
        </div>
        <div v-if="dirty && !isChatGPT" class="banner warn" role="status">
            {{ tr('chat.dirty') }}
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
                        {{ prompt }}
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
                    <strong>{{
                        entry.kind === 'user' ? tr('chat.you') : chatProviderNames[chat.provider]
                    }}</strong>
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
                <details v-else>
                    <summary>
                        {{ entry.name }}
                        <span>{{ tr(`chat.${entry.status || 'in_progress'}`) }}</span>
                    </summary>
                    <pre>{{ entry.detail }}</pre>
                </details>
            </article>
        </div>
        <div v-if="chat.error" class="banner error" role="alert">
            {{ chat.error }}
        </div>
        <p class="chat-outcome" role="status">
            {{
                chat.busy
                    ? tr(chat.stopping ? 'chat.stopping' : 'chat.running')
                    : chat.selected?.outcome === 'cancelled'
                      ? tr(isChatGPT ? 'chat.chatgptStopped' : 'chat.cancelled')
                      : chat.selected?.outcome === 'done'
                        ? tr('chat.done')
                        : ''
            }}
        </p>
        <form class="chat-composer" @submit.prevent="send">
            <label class="sr-only" for="agent-message">{{ tr('chat.message') }}</label>
            <textarea
                id="agent-message"
                ref="composer"
                v-model="chat.draft"
                :placeholder="tr(isChatGPT ? 'chat.chatgptPlaceholder' : 'chat.placeholder')"
                rows="3"
                maxlength="16000"
                @keydown="keydown"
            />
            <div class="chat-actions">
                <small>{{ tr('chat.keys') }}</small>
                <button
                    v-if="chat.busy"
                    type="button"
                    :disabled="chat.stopping"
                    @click="chat.cancel()"
                >
                    {{ tr('chat.stop') }}
                </button>
                <button
                    v-else
                    type="submit"
                    class="primary"
                    :disabled="blocked || !chat.draft.trim()"
                >
                    {{ tr('chat.send') }}
                </button>
            </div>
        </form>
    </section>
</template>

<style scoped>
.chat-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    width: 100%;
    margin: 0;
    border: 0;
    border-left: 1px solid var(--border);
    border-radius: 0;
}
.chat-history {
    border-bottom: 1px solid var(--border);
    padding: 10px 12px;
    flex: none;
}
.chat-history summary {
    cursor: pointer;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    padding: 4px 0;
    user-select: none;
}
.chat-history ul {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    max-height: 150px;
    overflow: auto;
}
.chat-history li + li {
    margin-top: 3px;
}
.chat-history button {
    display: flex;
    width: 100%;
    align-items: flex-start;
    flex-direction: column;
    text-align: left;
    padding: 5px 7px;
    border: 1px solid transparent;
}
.chat-history button[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--hover);
}
.chat-history button span {
    width: 100%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}
.chat-history small,
.chat-history-empty {
    color: var(--muted);
    font-size: 11px;
    margin: 0;
}
.chat-head {
    justify-content: flex-start;
    padding: 10px 14px;
}
.chat-head label {
    display: flex;
    align-items: center;
    gap: 6px;
}
.chat-head select {
    max-width: 200px;
    min-width: 0;
}
.chat-account {
    padding: 4px 14px 10px;
    flex: none;
}
.chat-account p {
    margin: 6px 0 0;
    overflow-wrap: anywhere;
}
.chat-account-actions,
.chat-account-identity,
.chatgpt-sign-in {
    display: flex;
    align-items: center;
    gap: 8px;
}
.chat-account-actions {
    flex-wrap: wrap;
}
.chat-account-identity {
    overflow-wrap: anywhere;
    min-width: 0;
}
.chatgpt-sign-in {
    background: #fff;
    color: #0d0d0d;
    border-color: #0d0d0d;
}
.chatgpt-sign-in.dark {
    background: #0d0d0d;
    color: #fff;
    border-color: #fff;
}
.chat-model-status {
    margin: 6px 14px;
    color: var(--muted);
    overflow-wrap: anywhere;
}
.chat-head input {
    width: 130px;
    min-width: 0;
}
.chat-context {
    margin: 10px 14px;
    color: var(--muted);
}
.chat-panel .banner {
    margin: 0 14px 8px;
}
.chat-feed {
    flex: 1;
    overflow: auto;
    min-height: 70px;
    padding: 0 14px;
    overscroll-behavior: contain;
}
.chat-entry {
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
}
.chat-entry.user {
    background: var(--surface);
    padding: 10px 12px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    margin: 8px 0;
}
.chat-empty-state {
    padding: 12px 0;
}
.chat-empty-state .empty {
    padding: 6px 0 4px;
    color: var(--text);
    font-weight: 500;
}
.chat-empty-state .chat-context {
    margin: 0 0 12px;
    font-size: 12px;
}
.starter-chips {
    display: flex;
    flex-direction: column;
    gap: 6px;
}
.starter-chip {
    text-align: left;
    justify-content: flex-start;
    padding: 8px 10px;
    font-size: 12px;
    line-height: 1.45;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
    font-weight: 450;
}
.starter-chip:hover:not(:disabled) {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent);
}
.chat-text {
    overflow-wrap: anywhere;
    line-height: 1.65;
    margin-top: 5px;
    user-select: text;
}
div.chat-text {
    white-space: pre-wrap;
}
.chat-text.markstream-vue {
    font-size: inherit;
    background: transparent;
}
.chat-entry summary {
    cursor: pointer;
    overflow-wrap: anywhere;
}
.chat-entry summary span {
    color: var(--muted);
    margin-left: 8px;
}
.chat-entry pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 220px;
    overflow: auto;
    font-size: 12px;
}
.chat-composer {
    border-top: 1px solid var(--border);
    padding: 10px 14px;
}
.chat-composer textarea {
    display: block;
    width: 100%;
    resize: vertical;
    max-height: 180px;
    min-height: 70px;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    padding: 8px;
}
.chat-actions {
    display: flex;
    gap: 10px;
    align-items: center;
    justify-content: space-between;
    margin-top: 8px;
}
.chat-actions small,
.chat-outcome {
    color: var(--muted);
}
.chat-outcome {
    min-height: 18px;
    margin: 6px 14px;
}
@media (max-width: 700px) {
    .chat-head input {
        width: 120px;
    }
}
</style>
