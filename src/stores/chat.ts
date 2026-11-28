import { defineStore } from 'pinia';
import { computed, reactive, ref } from 'vue';
import { Channel, invoke } from '@tauri-apps/api/core';
import { errorMessage } from '../api';
import { tr } from '../i18n';
import type { AgentInfo } from '../types';
import type { AgentTarget } from '../preferences';

export type ChatProvider = AgentTarget | 'chatgpt';
export const chatProviderNames: Record<ChatProvider, string> = {
    codex: 'Codex',
    chatgpt: 'ChatGPT',
    claude: 'Claude',
};

interface ChatGPTSession {
    status: 'disconnected' | 'connected' | 'reauth_required';
    sharing: boolean;
    identity?: { name?: string; email?: string };
    error?: { message: string };
}

export interface AgentModel {
    id: string;
    label: string;
    description: string;
}

// Mirrors scripts/chat-figures.ts, which the bridge runs outside the app bundle.
/** A drawing a tool returned: base64 `data` inlined when small, `file` the
 * stored copy read with `chat_figure`; `omitted` when neither was kept. */
export interface ChatFigure {
    mime: string;
    filename: string;
    bytes: number;
    data?: string;
    file?: string;
    omitted?: 'too_large';
    revision?: number;
    saved?: boolean;
}
/** One failed assertion, interrupted branch or lab issue. */
export interface ChatCheckItem {
    kind: 'assertion' | 'branch' | 'issue';
    label: string;
    detail: string;
    severity: 'error' | 'warning';
    element?: string;
    branch?: string;
}
/** The summary of a bench_lab_check result. */
export interface ChatChecks {
    passed: boolean;
    readiness: number;
    /** indeterminate is absent from summaries saved before it was counted. */
    assertions: { pass: number; fail: number; indeterminate?: number; unreachable: number };
    errors: number;
    warnings: number;
    items: ChatCheckItem[];
    more: number;
    revision?: number;
    saved?: boolean;
}
/** Where a chat review card points on the bench. */
export interface BenchTarget {
    element?: string;
    branch?: string;
}
export interface ChatEntry {
    id: string;
    kind: 'user' | 'text' | 'tool';
    text?: string;
    name?: string;
    status?: string;
    detail?: string;
    figure?: ChatFigure;
    checks?: ChatChecks;
    /** Reference images attached to a user message: `data` while the message
     * is sent, `file` the copy the app stored for history. */
    images?: ChatFigure[];
}
/** An image waiting in the composer. */
export interface ChatAttachment {
    id: string;
    mime: string;
    filename: string;
    bytes: number;
    data: string;
}
/** Image types, count and size the app accepts per message (chat.rs). */
export const ATTACHMENT_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif'];
export const ATTACHMENT_COUNT = 6;
export const ATTACHMENT_BYTES = 5 * 1024 * 1024;

function base64(bytes: Uint8Array): string {
    let text = '';
    for (let i = 0; i < bytes.length; i += 0x8000)
        text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(text);
}
export interface Conversation {
    id: string;
    provider: ChatProvider;
    model: string;
    title: string;
    updatedAt: number;
    messages: ChatEntry[];
    outcome: string;
    error: string;
}
interface ChatEvent {
    kind: 'text' | 'tool' | 'done' | 'error' | 'cancelled' | 'storage_error';
    id?: string;
    text?: string;
    name?: string;
    status?: string;
    detail?: string;
    figure?: ChatFigure;
    checks?: ChatChecks;
}
interface HistorySnapshot {
    conversations: Conversation[];
    activeId: string | null;
    activeConversationId: string | null;
}
export const useChatStore = defineStore('chat', () => {
    const provider = ref<ChatProvider>('codex');
    const model = ref('');
    const catalogs = reactive<
        Record<
            ChatProvider,
            { models: AgentModel[]; loading: boolean; loaded: boolean; error: string }
        >
    >({
        codex: { models: [], loading: false, loaded: false, error: '' },
        claude: { models: [], loading: false, loaded: false, error: '' },
        chatgpt: { models: [], loading: false, loaded: false, error: '' },
    });
    const modelCatalog = computed(() => catalogs[provider.value]);
    const conversations = ref<Conversation[]>([]);
    const selectedId = ref('');
    const drafts = reactive<Record<string, string>>({});
    const attachmentDrafts = reactive<Record<string, ChatAttachment[]>>({});
    const error = ref('');
    const storageError = ref('');
    const loaded = ref(false);
    const runtime = ref<{ node: string | null; agents: AgentInfo[] } | null>(null);
    const account = ref<ChatGPTSession | null>(null);
    const accountAction = ref('');
    const accountError = ref('');
    const activeId = ref('');
    const stopping = ref(false);
    const busy = computed(() => !!activeId.value);
    const selected = computed(() =>
        conversations.value.find((record) => record.id === selectedId.value),
    );
    const history = computed(() =>
        conversations.value.toSorted((left, right) => right.updatedAt - left.updatedAt),
    );
    const draftKey = computed(() => selectedId.value || `new:${provider.value}`);
    const draft = computed({
        get: () => drafts[draftKey.value] || '',
        set: (value) => {
            drafts[draftKey.value] = value;
        },
    });
    const attachments = computed(() => attachmentDrafts[draftKey.value] ?? []);
    /** Adds image files to the composer; other files and anything past the
     * count or size limit are refused with a message. */
    async function attach(files: Iterable<File>): Promise<void> {
        const key = draftKey.value;
        const list = [...files];
        if (!list.length) return;
        if (provider.value === 'chatgpt') {
            error.value = tr('chat.attachChatGPT');
            return;
        }
        const room = ATTACHMENT_COUNT - (attachmentDrafts[key]?.length ?? 0);
        const accepted = list
            .filter((file) => ATTACHMENT_TYPES.includes(file.type) && file.size <= ATTACHMENT_BYTES)
            .slice(0, Math.max(0, room));
        const refused = list
            .filter((file) => !accepted.includes(file))
            .map((file) => file.name || file.type);
        const added = await Promise.all(
            accepted.map(async (file) => ({
                id: crypto.randomUUID(),
                mime: file.type,
                filename: file.name || `pasted.${file.type.split('/')[1]}`,
                bytes: file.size,
                data: base64(new Uint8Array(await file.arrayBuffer())),
            })),
        );
        // A second attach may have finished while these files were read.
        const all = [...(attachmentDrafts[key] ?? []), ...added];
        refused.push(...all.slice(ATTACHMENT_COUNT).map((item) => item.filename));
        attachmentDrafts[key] = all.slice(0, ATTACHMENT_COUNT);
        error.value = refused.length
            ? tr('chat.attachRefused', { names: refused.join(', '), count: ATTACHMENT_COUNT })
            : '';
    }
    function detach(id: string): void {
        const key = draftKey.value;
        attachmentDrafts[key] = (attachmentDrafts[key] ?? []).filter((item) => item.id !== id);
    }
    const ready = computed(
        () =>
            loaded.value &&
            !!runtime.value?.node &&
            (provider.value === 'chatgpt'
                ? !!account.value?.sharing && !!model.value.trim() && !accountAction.value
                : !!runtime.value.agents.find((agent) => agent.name === provider.value)?.installed),
    );
    async function detect(): Promise<void> {
        try {
            runtime.value = await invoke('chat_runtime');
        } catch (failure) {
            error.value = errorMessage(failure);
        }
    }
    async function updateAccount(
        action: 'status' | 'sign-in' | 'enable-plan' | 'disconnect' = 'status',
    ): Promise<void> {
        if (accountAction.value) return;
        accountAction.value = action;
        accountError.value = '';
        try {
            account.value = await invoke<ChatGPTSession>('chatgpt_account', { action });
            accountError.value = account.value.error?.message || '';
        } catch (failure) {
            accountError.value = errorMessage(failure);
            if (action !== 'status') {
                account.value = await invoke<ChatGPTSession>('chatgpt_account', {
                    action: 'status',
                }).catch(() => null);
            }
        } finally {
            accountAction.value = '';
        }
        if (action !== 'status' || !account.value?.sharing) {
            Object.assign(catalogs.chatgpt, { models: [], loaded: false, error: '' });
        }
        if (provider.value === 'chatgpt' && account.value?.sharing) void loadModels();
    }
    async function loadModels(force = false): Promise<void> {
        const target = provider.value;
        const catalog = catalogs[target];
        if (target === 'chatgpt' && !account.value?.sharing) return;
        if (catalog.loading || (catalog.loaded && !force)) return;
        catalog.loading = true;
        catalog.error = '';
        try {
            catalog.models = await invoke<AgentModel[]>('chat_models', { provider: target });
            catalog.loaded = true;
        } catch (failure) {
            catalog.error = errorMessage(failure);
        } finally {
            catalog.loading = false;
        }
    }
    function changeModel(value: string): void {
        if (busy.value || value === model.value) return;
        newConversation();
        model.value = value;
    }
    function changeProvider(): void {
        if (busy.value) return;
        newConversation();
        model.value = '';
    }
    function select(id: string): void {
        if (busy.value) return;
        const record = conversations.value.find((candidate) => candidate.id === id);
        if (!record) return;
        selectedId.value = id;
        provider.value = record.provider;
        model.value = record.model;
        error.value = record.error;
    }
    function newConversation(): void {
        if (busy.value) return;
        const text = draft.value;
        selectedId.value = '';
        if (text && !draft.value) draft.value = text;
        error.value = '';
    }
    async function refresh(force = false): Promise<HistorySnapshot | null> {
        try {
            const snapshot = await invoke<HistorySnapshot>('chat_history');
            if (!activeId.value || force) conversations.value = snapshot.conversations;
            loaded.value = true;
            return snapshot;
        } catch (failure) {
            storageError.value = errorMessage(failure);
            return null;
        }
    }
    async function recoverActive(): Promise<void> {
        const snapshot = await refresh(true);
        if (!snapshot) return;
        activeId.value = snapshot.activeId || '';
        if (activeId.value)
            setTimeout(() => {
                void recoverActive();
            }, 1000);
        else {
            stopping.value = false;
            error.value = selected.value?.error || '';
        }
    }
    async function init(): Promise<void> {
        if (loaded.value) return;
        const [, snapshot] = await Promise.all([detect(), refresh()]);
        if (!snapshot) return;
        const record =
            snapshot.conversations.find(
                (candidate) => candidate.id === snapshot.activeConversationId,
            ) || history.value[0];
        if (record) select(record.id);
        if (provider.value === 'chatgpt') void updateAccount();
        else void loadModels();
        if (snapshot.activeId) {
            activeId.value = snapshot.activeId;
            void recoverActive();
        }
    }
    async function send(): Promise<void> {
        const prompt = draft.value.trim();
        const images = provider.value === 'chatgpt' ? [] : attachments.value;
        if ((!prompt && !images.length) || busy.value || !ready.value) return;
        const turnId = crypto.randomUUID();
        const previousId = selectedId.value;
        const previousDraftKey = draftKey.value;
        const existing = selected.value;
        const record: Conversation = existing || {
            id: turnId,
            provider: provider.value,
            model: model.value.trim(),
            title: (prompt.split('\n')[0] || images[0]?.filename || '').slice(0, 80),
            updatedAt: Date.now(),
            messages: [],
            outcome: '',
            error: '',
        };
        if (!existing) conversations.value.push(record);
        // Use the reactive record so streaming updates reach the component immediately.
        selectedId.value = record.id;
        const entries = selected.value!.messages;
        entries.push({
            id: turnId,
            kind: 'user',
            text: prompt,
            ...(images.length
                ? {
                      images: images.map(({ mime, filename, bytes, data }) => ({
                          mime,
                          filename,
                          bytes,
                          data,
                      })),
                  }
                : {}),
        });
        selected.value!.updatedAt = Date.now();
        selected.value!.outcome = '';
        selected.value!.error = '';
        activeId.value = turnId;
        stopping.value = false;
        error.value = '';
        drafts[previousDraftKey] = '';
        attachmentDrafts[previousDraftKey] = [];
        const channel = new Channel<ChatEvent>();
        // Tauri Channel delivers events through this SDK-defined setter.
        // eslint-disable-next-line unicorn/prefer-add-event-listener
        channel.onmessage = (event) => {
            if (activeId.value !== turnId) return;
            if (event.kind === 'storage_error') {
                storageError.value = event.text || 'History write failed';
                return;
            }
            if (event.kind === 'text' || event.kind === 'tool') {
                const key = `${turnId}:${event.id}`;
                const previous = entries.find((item) => item.id === key);
                const update = Object.fromEntries(
                    Object.entries(event).filter(([, value]) => value !== undefined),
                );
                if (previous) Object.assign(previous, update, { id: key });
                else entries.push({ ...event, id: key, kind: event.kind });
            } else {
                if (event.kind === 'error') error.value = event.text || 'Agent request failed';
                selected.value!.outcome = event.kind;
                selected.value!.error = error.value;
                activeId.value = '';
                stopping.value = false;
                void refresh();
                if (record.provider === 'chatgpt' && event.kind === 'error') void updateAccount();
            }
        };
        try {
            await invoke('chat_start', {
                request: {
                    id: turnId,
                    conversationId: existing ? record.id : null,
                    provider: record.provider,
                    model: record.model,
                    prompt,
                    images: images.map(({ mime, filename, data }) => ({ mime, filename, data })),
                },
                channel,
            });
        } catch (failure) {
            error.value = errorMessage(failure);
            if (!drafts[previousDraftKey]) drafts[previousDraftKey] = prompt;
            if (!attachmentDrafts[previousDraftKey]?.length)
                attachmentDrafts[previousDraftKey] = images;
            entries.splice(
                entries.findIndex((entry) => entry.id === turnId),
                1,
            );
            if (!existing)
                conversations.value = conversations.value.filter((item) => item.id !== record.id);
            selectedId.value = previousId;
            activeId.value = '';
            stopping.value = false;
        }
    }
    async function cancel(): Promise<void> {
        if (!activeId.value || stopping.value) return;
        stopping.value = true;
        try {
            await invoke('chat_cancel', { id: activeId.value });
        } catch (failure) {
            error.value = errorMessage(failure);
            stopping.value = false;
        }
    }
    return {
        provider,
        model,
        modelCatalog,
        account,
        accountAction,
        accountError,
        updateAccount,
        loadModels,
        changeModel,
        changeProvider,
        conversations,
        selectedId,
        selected,
        history,
        draft,
        attachments,
        attach,
        detach,
        error,
        storageError,
        loaded,
        runtime,
        activeId,
        stopping,
        busy,
        ready,
        detect,
        init,
        select,
        newConversation,
        send,
        cancel,
    };
});
