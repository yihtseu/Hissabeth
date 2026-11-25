<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { tr } from '../i18n';
import type { Layout, OpticalSystem, Paraxial } from '../types';
import LayoutView from './LayoutView.vue';
import BenchView from './BenchView.vue';
const props = defineProps<{
    system: OpticalSystem;
    layout: Layout;
    wavelength: number;
    revision: number;
    online: boolean;
    /** Kept for template compatibility; pending drafts are chained in automatically. */
    dirty?: boolean;
    busy: boolean;
    stale: boolean;
    refreshPending: boolean;
    /** Chains a valid pending draft, runs the operation with the current revision, then reloads. */
    mutate: (op: (revision: number, system: OpticalSystem) => Promise<unknown>) => Promise<boolean>;
    /** First-order data of the referenced prescription, shown in the lens group summary. */
    paraxial?: Paraxial | null;
    chatOpen?: boolean;
}>();
const emit = defineEmits<{ chat: []; scope: [section: boolean] }>();
/** section = sequential path, 2d/3d = laser bench; the app toolbar binds this to switch modes. */
const modeModel = defineModel<'section' | '2d' | '3d'>('mode');
const mode = computed<'section' | '2d' | '3d'>({
    get: () => modeModel.value ?? (props.system.bench ? '2d' : 'section'),
    set: (value) => (modeModel.value = value),
});
/** Bench draft state, reported upward so the app can show one unified dirty indicator. */
const benchDirty = defineModel<boolean>('benchDirty', { default: false });
watch(mode, (value) => emit('scope', value === 'section'), { flush: 'post' });
// Register the surrounding panes before changing which dock is visible.
onMounted(() => void nextTick(() => emit('scope', mode.value === 'section')));
const benchView = ref<InstanceType<typeof BenchView>>();
const selected = ref('');
const group = computed(() =>
    props.system.bench?.elements.find((e) => e.id === selected.value && e.kind === 'lens_group'),
);
/** The app toolbar's unified Apply/Revert reaches the bench draft through these. */
defineExpose({
    saveBench: () => benchView.value?.saveBench(),
    discardBench: () => benchView.value?.discardBench(),
});
</script>
<template>
    <section class="optical-workspace">
        <div class="optical-viewbar" role="group" :aria-label="tr('optical.views')">
            <button :aria-pressed="mode === 'section'" @click="mode = 'section'">
                {{ tr('optical.section') }}
            </button>
            <button :aria-pressed="mode === '2d'" @click="mode = '2d'">
                {{ tr('bench.plan') }}
            </button>
            <button :aria-pressed="mode === '3d'" @click="mode = '3d'">
                {{ tr('bench.spatial') }}
            </button>
            <span class="spacer"></span
            ><span class="optical-scope">{{
                mode === 'section' ? tr('optical.groupScope') : tr('optical.benchScope')
            }}</span>
        </div>
        <p v-if="mode === 'section' && group" class="optical-context">
            {{ group.id }} · {{ group.label }} · {{ tr('optical.currentPrescription') }}
        </p>
        <LayoutView
            v-show="mode === 'section'"
            :system="system"
            :layout="layout"
            :wavelength="wavelength"
        />
        <BenchView
            v-show="mode !== 'section'"
            ref="benchView"
            v-model:selected="selected"
            v-model:bench-dirty="benchDirty"
            :system="system"
            :paraxial="paraxial"
            :revision="revision"
            :online="online"
            :dirty="dirty"
            :busy="busy"
            :stale="stale"
            :refresh-pending="refreshPending"
            :mutate="mutate"
            :mode="mode === '3d' ? '3d' : '2d'"
            :chat-open="chatOpen"
            @inspect-group="mode = 'section'"
            @chat="emit('chat')"
        />
    </section>
</template>
