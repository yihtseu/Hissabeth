<script setup lang="ts">
import { tr } from '../i18n';

/** The three analysis views share one sub-navigation; the labels are the existing tab route keys. */
export type AnalysisView = 'Spot diagram' | 'MTF' | 'Analysis';
const props = defineProps<{ active: AnalysisView }>();
const emit = defineEmits<{ navigate: [view: AnalysisView] }>();

const VIEWS: AnalysisView[] = ['Spot diagram', 'MTF', 'Analysis'];

function onKey(e: KeyboardEvent, index: number): void {
    const step =
        e.key === 'ArrowRight' || e.key === 'ArrowDown'
            ? 1
            : e.key === 'ArrowLeft' || e.key === 'ArrowUp'
              ? -1
              : 0;
    const target =
        e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? VIEWS.length - 1
              : step
                ? (index + step + VIEWS.length) % VIEWS.length
                : -1;
    if (target < 0) return;
    e.preventDefault();
    emit('navigate', VIEWS[target]!);
}
</script>

<template>
    <div class="analysis-nav" role="tablist" :aria-label="tr('app.workbenchViews')">
        <button
            v-for="(view, i) in VIEWS"
            :key="view"
            type="button"
            role="tab"
            class="subnav-pill"
            :aria-selected="active === view"
            :tabindex="active === view ? 0 : -1"
            @click="emit('navigate', view)"
            @keydown="onKey($event, i)"
        >
            {{ tr(`tabs.${view}`) }}
        </button>
    </div>
</template>

<style scoped>
.analysis-nav {
    display: inline-flex;
    gap: 2px;
    padding: 3px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 999px;
    width: max-content;
    max-width: 100%;
    overflow-x: auto;
}
.subnav-pill {
    flex: none;
    border: 1px solid transparent;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
    padding: 3px 14px;
    min-height: 24px;
    white-space: nowrap;
}
.subnav-pill:hover {
    color: var(--text);
    background: var(--hover);
}
.subnav-pill[aria-selected='true'] {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
}
@media (max-width: 480px) {
    .analysis-nav {
        width: 100%;
        justify-content: flex-start;
    }
    .subnav-pill {
        flex: 1 0 auto;
        padding: 3px 10px;
    }
}
</style>
