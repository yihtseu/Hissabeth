<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { tr } from '../i18n';
import type { Layout, OpticalSystem } from '../types';
import LayoutView from './LayoutView.vue';
import BenchView from './BenchView.vue';
const props = defineProps<{
    system: OpticalSystem;
    layout: Layout;
    wavelength: number;
    revision: number;
    online: boolean;
    dirty: boolean;
    busy: boolean;
    stale: boolean;
    refreshPending: boolean;
    mutate: (op: (revision: number) => Promise<unknown>) => Promise<boolean>;
    chatOpen?: boolean;
}>();
const emit = defineEmits<{ chat: []; scope: [section: boolean] }>();
const view = ref<'section' | '2d' | '3d'>(props.system.bench ? '2d' : 'section');
watch(view, (value) => emit('scope', value === 'section'), { flush: 'post' });
// Register the surrounding panes before changing which dock is visible.
onMounted(() => void nextTick(() => emit('scope', view.value === 'section')));
const selected = ref('');
const group = computed(() =>
    props.system.bench?.elements.find((e) => e.id === selected.value && e.kind === 'lens_group'),
);
</script>
<template>
    <section class="optical-workspace">
        <div class="optical-viewbar" role="group" :aria-label="tr('optical.views')">
            <button :aria-pressed="view === 'section'" @click="view = 'section'">
                {{ tr('optical.section') }}
            </button>
            <button :aria-pressed="view === '2d'" @click="view = '2d'">
                {{ tr('bench.plan') }}
            </button>
            <button :aria-pressed="view === '3d'" @click="view = '3d'">
                {{ tr('bench.spatial') }}
            </button>
            <span class="spacer"></span
            ><span class="optical-scope">{{
                view === 'section' ? tr('optical.groupScope') : tr('optical.benchScope')
            }}</span>
        </div>
        <p v-if="view === 'section' && group" class="optical-context">
            {{ group.id }} · {{ group.label }} · {{ tr('optical.currentPrescription') }}
        </p>
        <LayoutView
            v-show="view === 'section'"
            :system="system"
            :layout="layout"
            :wavelength="wavelength"
        />
        <BenchView
            v-show="view !== 'section'"
            v-model:selected="selected"
            :system="system"
            :revision="revision"
            :online="online"
            :dirty="dirty"
            :busy="busy"
            :stale="stale"
            :refresh-pending="refreshPending"
            :mutate="mutate"
            :mode="view === '3d' ? '3d' : '2d'"
            :chat-open="chatOpen"
            @inspect-group="view = 'section'"
            @chat="emit('chat')"
        />
    </section>
</template>
