import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { errorMessage, inDesktop } from '../api';
import {
    asPreferencesRejection,
    defaultPreferences,
    type Preferences,
    type PreferencesSnapshot,
    type WorkspacePreferences,
} from '../preferences';
import { clone } from '../validation';

export const usePreferencesStore = defineStore('preferences', () => {
    const preferences = ref<Preferences>(defaultPreferences());
    const revision = ref(0);
    const status = ref<'loading' | 'ready' | 'error'>('loading');
    const loadError = ref('');
    const syncError = ref('');
    const warning = ref('');
    const saveError = ref('');
    const conflictMessage = ref('');
    const pending = ref(0);
    const saving = computed(() => pending.value > 0);
    const loaded = computed(() => status.value === 'ready');
    let unlisten: UnlistenFn | undefined;
    let initializing: Promise<void> | undefined;
    let queue: Promise<unknown> = Promise.resolve();
    let early: PreferencesSnapshot | undefined;
    function adopt(snapshot: PreferencesSnapshot): void {
        if (snapshot.revision < revision.value) return;
        preferences.value = clone(snapshot.preferences);
        revision.value = snapshot.revision;
        warning.value = snapshot.warning ?? '';
    }
    async function load(): Promise<void> {
        if (!inDesktop()) {
            status.value = 'error';
            loadError.value = 'desktop_required';
            return;
        }
        if (!unlisten) {
            try {
                unlisten = await listen<PreferencesSnapshot>(
                    'preferences-changed',
                    ({ payload }) => {
                        if (!loaded.value) {
                            if (!early || payload.revision > early.revision) early = payload;
                        } else adopt(payload);
                    },
                );
                syncError.value = '';
            } catch (error) {
                syncError.value = errorMessage(error);
            }
        }
        try {
            adopt(await invoke<PreferencesSnapshot>('get_preferences'));
            if (early) adopt(early);
            early = undefined;
            status.value = 'ready';
            loadError.value = '';
            saveError.value = '';
            conflictMessage.value = '';
        } catch (error) {
            status.value = 'error';
            loadError.value = errorMessage(error);
        }
    }
    function init(): Promise<void> {
        initializing ??= load();
        return initializing;
    }
    async function retry(): Promise<void> {
        await load();
    }
    async function write(next: Preferences, expectedRevision: number): Promise<boolean> {
        saveError.value = '';
        conflictMessage.value = '';
        try {
            adopt(
                await invoke<PreferencesSnapshot>('set_preferences', {
                    preferences: next,
                    expectedRevision,
                }),
            );
            return true;
        } catch (error) {
            const rejection = asPreferencesRejection(error);
            if (rejection?.code === 'revision_conflict') {
                adopt(rejection.snapshot);
                conflictMessage.value = rejection.message;
            } else saveError.value = rejection?.message ?? errorMessage(error);
            return false;
        }
    }
    function enqueue(operation: () => Promise<boolean>): Promise<boolean> {
        if (!loaded.value) return Promise.resolve(false);
        pending.value++;
        const result = queue.then(operation).finally(() => {
            pending.value--;
        });
        queue = result.catch(() => undefined);
        return result;
    }
    // A settings draft records the revision it was edited from, even while another save is queued.
    function save(next: Preferences, expectedRevision = revision.value): Promise<boolean> {
        const captured = clone(next);
        return enqueue(() => write(captured, expectedRevision));
    }
    // Workspace gestures change named fields against the latest snapshot when their turn arrives.
    function patchWorkspace(fields: Partial<WorkspacePreferences>): Promise<boolean> {
        const captured = clone(fields);
        return enqueue(() =>
            write(
                {
                    ...clone(preferences.value),
                    workspace: { ...preferences.value.workspace, ...captured },
                },
                revision.value,
            ),
        );
    }
    function dispose(): void {
        unlisten?.();
        unlisten = undefined;
    }
    return {
        preferences,
        revision,
        status,
        loaded,
        saving,
        loadError,
        syncError,
        warning,
        saveError,
        conflictMessage,
        init,
        retry,
        save,
        patchWorkspace,
        dispose,
    };
});
