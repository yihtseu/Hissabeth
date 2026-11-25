<script setup lang="ts">
import { computed, reactive, ref, watch, onMounted, onBeforeUnmount } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { TabsRoot, TabsList, TabsTrigger, TabsContent } from 'reka-ui';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
    Settings2,
    Plug,
    Palette,
    PanelsTopLeft,
    SlidersHorizontal,
    Languages,
} from 'lucide-vue-next';
import { usePreferencesStore } from './stores/preferences';
import { useAppearance } from './composables/useAppearance';
import { tr } from './i18n';
import { PREF_LIMITS, type Preferences } from './preferences';
import { clone } from './validation';
import { configureAgents, detectAgents, errorMessage } from './api';
import { TABS, type AgentInfo, type AgentRegistration } from './types';

useAppearance();
const store = usePreferencesStore();
const category = ref(
    new URLSearchParams(location.search).get('section') === 'agents' ? 'agents' : 'general',
);
let stopSection: UnlistenFn | undefined;
let disposed = false;
onMounted(async () => {
    try {
        const stop = await listen<string>('settings-section', ({ payload }) => {
            if (payload === 'agents' || payload === 'general') category.value = payload;
        });
        if (disposed) stop();
        else stopSection = stop;
    } catch (error) {
        store.syncError = errorMessage(error);
    }
});
onBeforeUnmount(() => {
    disposed = true;
    stopSection?.();
});
const categories = [
    ['general', Languages],
    ['appearance', Palette],
    ['workspace', PanelsTopLeft],
    ['display', SlidersHorizontal],
    ['analysis', Settings2],
    ['agents', Plug],
] as const;
const draft = ref<Preferences>(clone(store.preferences));
const baseRevision = ref(store.revision);
const changedElsewhere = ref(false);
const applying = ref(false);
const saved = ref(false);
const sections: Record<string, string> = {
    general: 'General',
    appearance: 'Appearance',
    workspace: 'Workspace',
    display: 'Display',
    analysis: 'Analysis',
    agents: 'Agents',
};
const numberFields = [
    { key: 'fontSize', section: 'appearance', label: 'fontSize' },
    { key: 'navigatorSize', section: 'workspace', label: 'navigatorSize' },
    { key: 'dockSize', section: 'workspace', label: 'dockSize' },
    { key: 'decimals', section: 'display', label: 'decimals' },
    { key: 'gridSize', section: 'analysis', label: 'gridSize' },
    { key: 'hexRings', section: 'analysis', label: 'hexRings' },
    { key: 'reportPrecision', section: 'analysis', label: 'reportPrecision' },
    { key: 'pollIntervalMs', section: 'analysis', label: 'pollInterval' },
] as const;
type NumberKey = (typeof numberFields)[number]['key'];
const raw = reactive({} as Record<NumberKey, string>);
const seedJson = ref('');
function seed(): void {
    draft.value = clone(store.preferences);
    baseRevision.value = store.revision;
    for (const field of numberFields)
        raw[field.key] = String(
            (draft.value[field.section] as unknown as Record<string, number>)[field.key],
        );
    changedElsewhere.value = false;
    saved.value = false;
    seedJson.value = JSON.stringify(store.preferences);
}
seed();
const edited = computed(
    () =>
        JSON.stringify(draft.value) !== JSON.stringify(store.preferences) ||
        numberFields.some(
            (field) =>
                raw[field.key] !==
                String(
                    (store.preferences[field.section] as unknown as Record<string, number>)[
                        field.key
                    ],
                ),
        ),
);
const invalid = computed(() =>
    numberFields.filter((field) => {
        const text = raw[field.key]?.trim();
        const n = Number(text);
        const limit = PREF_LIMITS[field.key];
        return (
            !text ||
            !Number.isFinite(n) ||
            (!['navigatorSize', 'dockSize'].includes(field.key) && !Number.isInteger(n)) ||
            n < limit.min ||
            n > limit.max ||
            (field.key === 'gridSize' && n % 2 !== 1)
        );
    }),
);
watch(
    () => store.revision,
    () => {
        if (applying.value) return;
        // Compare to the form's seed, not the now-updated remote object.
        const local = clone(draft.value);
        for (const field of numberFields)
            (local[field.section] as unknown as Record<string, number>)[field.key] = Number(
                raw[field.key],
            );
        if (JSON.stringify(local) === seedJson.value) seed();
        else changedElsewhere.value = true;
    },
);
function reload(): void {
    seed();
    seedJson.value = JSON.stringify(store.preferences);
}
async function apply(): Promise<void> {
    if (invalid.value.length || applying.value || changedElsewhere.value || !store.loaded) return;
    const next = clone(draft.value);
    for (const field of numberFields)
        (next[field.section] as unknown as Record<string, number>)[field.key] = Number(
            raw[field.key],
        );
    const capturedDraft = JSON.stringify(draft.value);
    const capturedRaw = JSON.stringify(raw);
    applying.value = true;
    saved.value = false;
    try {
        const ok = await store.save(next, baseRevision.value);
        if (ok) {
            if (JSON.stringify(store.preferences) === JSON.stringify(next)) {
                baseRevision.value = store.revision;
                seedJson.value = JSON.stringify(next);
                if (
                    capturedDraft === JSON.stringify(draft.value) &&
                    capturedRaw === JSON.stringify(raw)
                )
                    reload();
                saved.value = true;
            } else changedElsewhere.value = true;
        } else if (store.conflictMessage) changedElsewhere.value = true;
    } finally {
        applying.value = false;
    }
}
function defaults(): void {
    // Defaults are staged in this form until Apply.
    import('./preferences').then(({ defaultPreferences }) => {
        draft.value = defaultPreferences();
        for (const field of numberFields)
            raw[field.key] = String(
                (draft.value[field.section] as unknown as Record<string, number>)[field.key],
            );
        saved.value = false;
    });
}
const agents = ref<AgentInfo[]>([]);
const results = ref<AgentRegistration[]>([]);
const agentBusy = ref(false);
const agentError = ref('');
async function detect(): Promise<void> {
    agentBusy.value = true;
    agentError.value = '';
    try {
        agents.value = await detectAgents();
    } catch (error) {
        agentError.value = errorMessage(error);
    } finally {
        agentBusy.value = false;
    }
}
async function connect(): Promise<void> {
    agentBusy.value = true;
    agentError.value = '';
    results.value = [];
    try {
        results.value = await configureAgents([...draft.value.agents.targets]);
        agents.value = await detectAgents();
    } catch (error) {
        agentError.value = errorMessage(error);
    } finally {
        agentBusy.value = false;
    }
}
void detect();
</script>

<template>
    <div class="settings-app">
        <header class="settings-heading">
            <Settings2 :size="18" />
            <h1>{{ tr('settings.title') }}</h1>
            <span>{{ tr('settings.subtitle') }}</span>
        </header>
        <div v-if="!store.loaded" class="banner error" role="alert">
            <div>
                {{ tr('settings.loadFailedTitle') }}
                <p>{{ tr('settings.loadFailedBody') }}</p>
                <code>{{ store.loadError }}</code>
            </div>
            <button @click="store.retry()">{{ tr('settings.retry') }}</button>
        </div>
        <div v-if="store.warning || store.syncError" class="banner warn" role="alert">
            {{ tr('settings.warning', { message: store.warning || store.syncError })
            }}<button @click="store.retry()">{{ tr('settings.retry') }}</button>
        </div>
        <div v-if="changedElsewhere" class="banner warn" role="alert">
            {{ tr('settings.conflict', { message: store.conflictMessage })
            }}<button @click="reload">{{ tr('settings.retry') }}</button>
        </div>
        <div v-if="store.saveError" class="banner error" role="alert">
            {{ tr('settings.saveFailed', { message: store.saveError }) }}
        </div>
        <TabsRoot v-model="category" class="settings-tabs" orientation="vertical">
            <TabsList :aria-label="tr('settings.title')" class="settings-categories">
                <TabsTrigger v-for="[id, icon] in categories" :key="id" :value="id"
                    ><component :is="icon" :size="16" />{{
                        tr(`settings.section${sections[id]}`)
                    }}</TabsTrigger
                >
            </TabsList>
            <div class="settings-content">
                <TabsContent
                    v-for="[id] in categories"
                    :key="id"
                    :value="id"
                    class="settings-section"
                >
                    <h2>{{ tr(`settings.section${sections[id]}`) }}</h2>
                    <fieldset :disabled="!store.loaded" class="settings-fields">
                        <template v-if="id === 'general'">
                            <label
                                >{{ tr('settings.language')
                                }}<select
                                    :aria-label="tr('settings.language')"
                                    v-model="draft.locale"
                                >
                                    <option value="zh-CN">简体中文</option>
                                    <option value="en-US">English (US)</option>
                                </select></label
                            >
                            <label
                                >{{ tr('settings.defaultView')
                                }}<select
                                    :aria-label="tr('settings.defaultView')"
                                    v-model="draft.workspace.defaultView"
                                >
                                    <option
                                        v-for="tab in TABS.filter(
                                            (view) => view !== 'Agent connections',
                                        )"
                                        :key="tab"
                                        :value="tab"
                                    >
                                        {{ tr(`tabs.${tab}`) }}
                                    </option>
                                </select></label
                            >
                            <label class="check"
                                ><input
                                    v-model="draft.workspace.restoreLastView"
                                    type="checkbox"
                                />{{ tr('settings.restoreLastView') }}</label
                            >
                        </template>
                        <template v-if="id === 'appearance'">
                            <label
                                >{{ tr('settings.theme')
                                }}<select
                                    :aria-label="tr('settings.theme')"
                                    v-model="draft.appearance.theme"
                                >
                                    <option
                                        v-for="theme in [
                                            'system',
                                            'light',
                                            'dark',
                                            'high-contrast',
                                        ]"
                                        :key="theme"
                                        :value="theme"
                                    >
                                        {{
                                            tr(
                                                `settings.theme${theme === 'high-contrast' ? 'HighContrast' : theme.charAt(0).toUpperCase() + theme.slice(1)}`,
                                            )
                                        }}
                                    </option>
                                </select></label
                            >
                            <label
                                >{{ tr('settings.accent')
                                }}<select
                                    :aria-label="tr('settings.accent')"
                                    v-model="draft.appearance.accent"
                                >
                                    <option
                                        v-for="accent in ['blue', 'teal', 'amber']"
                                        :key="accent"
                                        :value="accent"
                                    >
                                        {{
                                            tr(
                                                `settings.accent${accent.charAt(0).toUpperCase() + accent.slice(1)}`,
                                            )
                                        }}
                                    </option>
                                </select></label
                            >
                            <label
                                >{{ tr('settings.density')
                                }}<select
                                    :aria-label="tr('settings.density')"
                                    v-model="draft.appearance.density"
                                >
                                    <option value="compact">
                                        {{ tr('settings.densityCompact') }}
                                    </option>
                                    <option value="comfortable">
                                        {{ tr('settings.densityComfortable') }}
                                    </option>
                                </select></label
                            >
                            <label class="check"
                                ><input
                                    v-model="draft.appearance.reducedMotion"
                                    type="checkbox"
                                />{{ tr('settings.reducedMotion') }}</label
                            >
                        </template>
                        <template v-if="id === 'workspace'">
                            <label
                                v-for="[key, label] in [
                                    ['showNavigator', 'showNavigator'],
                                    ['showPrescriptionDock', 'showDock'],
                                    ['showMetrics', 'showMetrics'],
                                    ['showStatusBar', 'showStatusBar'],
                                ] as const"
                                :key="key"
                                class="check"
                                ><input v-model="draft.workspace[key]" type="checkbox" />{{
                                    tr(`settings.${label}`)
                                }}</label
                            >
                        </template>
                        <template v-if="id === 'display'">
                            <p class="note">{{ tr('settings.decimalsHelp') }}</p>
                            <label class="check"
                                ><input v-model="draft.display.layoutRayArrows" type="checkbox" />{{
                                    tr('settings.layoutRayArrows')
                                }}</label
                            >
                            <label class="check"
                                ><input v-model="draft.display.plotGrid" type="checkbox" />{{
                                    tr('settings.plotGrid')
                                }}</label
                            >
                        </template>
                        <template v-if="id === 'analysis'">
                            <label
                                >{{ tr('settings.samplingKind')
                                }}<select
                                    :aria-label="tr('settings.samplingKind')"
                                    v-model="draft.analysis.samplingKind"
                                >
                                    <option value="grid">{{ tr('sampling.grid') }}</option>
                                    <option value="hexapolar">
                                        {{ tr('sampling.hexapolar') }}
                                    </option>
                                </select></label
                            >
                        </template>
                        <label
                            v-for="field in numberFields.filter((f) => f.section === id)"
                            :key="field.key"
                            >{{ tr(`settings.${field.label}`, PREF_LIMITS[field.key])
                            }}<input
                                :value="raw[field.key]"
                                type="number"
                                :min="PREF_LIMITS[field.key].min"
                                :max="PREF_LIMITS[field.key].max"
                                :step="
                                    field.key === 'gridSize'
                                        ? 2
                                        : ['navigatorSize', 'dockSize'].includes(field.key)
                                          ? 0.1
                                          : 1
                                "
                                :aria-invalid="invalid.some((f) => f.key === field.key)"
                                @input="
                                    raw[field.key] = ($event.target as HTMLInputElement).value;
                                    saved = false;
                                "
                        /></label>
                        <template v-if="id === 'agents'">
                            <fieldset>
                                <legend>{{ tr('settings.agentTargets') }}</legend>
                                <label
                                    v-for="agent in ['codex', 'claude'] as const"
                                    :key="agent"
                                    class="check"
                                    ><input
                                        v-model="draft.agents.targets"
                                        type="checkbox"
                                        :value="agent"
                                    />{{ agent === 'codex' ? 'Codex' : 'Claude Code' }}</label
                                >
                            </fieldset>
                            <label class="check"
                                ><input v-model="draft.agents.autoRegister" type="checkbox" />{{
                                    tr('settings.autoRegister')
                                }}</label
                            >
                            <div class="controls">
                                <button :disabled="agentBusy" @click="detect">
                                    {{ tr('settings.detect') }}</button
                                ><button
                                    :disabled="agentBusy || !draft.agents.targets.length"
                                    @click="connect"
                                >
                                    {{ tr('agents.connectSelected') }}
                                </button>
                            </div>
                            <p v-for="agent in agents" :key="agent.name" class="note">
                                {{
                                    agent.installed
                                        ? tr('settings.detected', {
                                              name: agent.name,
                                              path: agent.executable || '',
                                          })
                                        : tr('settings.notDetected', { name: agent.name })
                                }}
                            </p>
                            <p v-if="agentError" class="banner error" role="alert">
                                {{ agentError }}
                            </p>
                            <p
                                v-for="(result, index) in results"
                                :key="index"
                                :class="result.success ? 'ok banner' : 'error banner'"
                                role="status"
                            >
                                {{ result.name }}: {{ result.message || result.status }}
                            </p>
                        </template>
                    </fieldset>
                </TabsContent>
            </div>
        </TabsRoot>
        <footer class="settings-footer">
            <button @click="defaults">{{ tr('settings.restoreDefaults') }}</button
            ><span v-if="saved && !edited" role="status">{{ tr('settings.saved') }}</span>
            <div class="spacer"></div>
            <button @click="reload">{{ tr('settings.cancel') }}</button
            ><button @click="getCurrentWindow().close()">{{ tr('common.close') }}</button
            ><button
                class="primary"
                :disabled="
                    !store.loaded ||
                    (!edited && !store.warning) ||
                    applying ||
                    !!invalid.length ||
                    changedElsewhere
                "
                @click="apply"
            >
                {{ tr(applying ? 'settings.saving' : 'settings.apply') }}
            </button>
        </footer>
        <ul v-if="invalid.length && edited" class="settings-invalid" role="status">
            <li v-for="field in invalid" :key="field.key">
                {{ tr(`settings.validation.${field.label}`, PREF_LIMITS[field.key]) }}
            </li>
        </ul>
    </div>
</template>
