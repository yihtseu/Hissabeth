<script setup lang="ts">
import { computed } from 'vue';
import { CircleCheck, CircleX, LocateFixed } from 'lucide-vue-next';
import { tr } from '../i18n';
import type { BenchTarget, ChatChecks } from '../stores/chat';

const props = defineProps<{ checks: ChatChecks }>();
const emit = defineEmits<{ locate: [target: BenchTarget] }>();

const target = (item: { element?: string; branch?: string }): BenchTarget => ({
    ...(item.element ? { element: item.element } : {}),
    ...(item.branch ? { branch: item.branch } : {}),
});
/** The card-level button goes to the first failure that names a place. */
const first = computed(() =>
    target(props.checks.items.find((item) => item.element || item.branch) ?? {}),
);
const origin = computed(() => {
    const { revision, saved } = props.checks;
    if (revision === undefined) return '';
    return saved === false
        ? tr('chat.figureDraft', { revision })
        : tr('chat.figureRevision', { revision });
});
</script>

<template>
    <section class="chat-checks" :class="checks.passed ? 'is-pass' : 'is-fail'">
        <header class="chat-checks-head">
            <span class="chat-checks-badge" role="status">
                <CircleCheck v-if="checks.passed" :size="13" aria-hidden="true" />
                <CircleX v-else :size="13" aria-hidden="true" />
                {{ tr(checks.passed ? 'chat.checksPassed' : 'chat.checksFailed') }}
            </span>
            <span class="chat-checks-counts">{{
                tr('chat.checksSummary', {
                    pass: checks.assertions.pass,
                    fail: checks.assertions.fail,
                    indeterminate: checks.assertions.indeterminate ?? 0,
                    unreachable: checks.assertions.unreachable,
                    errors: checks.errors,
                    warnings: checks.warnings,
                    readiness: checks.readiness,
                })
            }}</span>
            <span v-if="origin" class="chat-checks-origin">{{ origin }}</span>
            <button type="button" class="small chat-checks-locate" @click="emit('locate', first)">
                <LocateFixed :size="12" /> {{ tr('chat.figureLocate') }}
            </button>
        </header>
        <ul v-if="checks.items.length" class="chat-checks-items">
            <li
                v-for="(item, index) in checks.items"
                :key="index"
                :class="`is-${item.severity}`"
                :data-kind="item.kind"
            >
                <span class="chat-checks-label">{{ item.label }}</span>
                <span class="chat-checks-detail">{{ item.detail }}</span>
                <button
                    v-if="item.element || item.branch"
                    type="button"
                    class="small"
                    :aria-label="
                        tr('chat.checksLocate', { name: item.element ?? item.branch ?? '' })
                    "
                    :title="tr('chat.checksLocate', { name: item.element ?? item.branch ?? '' })"
                    @click="emit('locate', target(item))"
                >
                    <LocateFixed :size="12" />
                </button>
            </li>
        </ul>
        <p v-if="checks.more" class="chat-checks-more">
            {{ tr('chat.checksMore', { count: checks.more }) }}
        </p>
        <ul
            v-if="checks.readings?.length"
            class="chat-checks-readings"
            :aria-label="tr('chat.checksReadings')"
        >
            <li
                v-for="(reading, index) in checks.readings"
                :key="index"
                :class="reading.ok ? 'is-ok' : 'is-off'"
                :data-kind="reading.kind"
            >
                <CircleCheck v-if="reading.ok" :size="11" aria-hidden="true" />
                <CircleX v-else :size="11" aria-hidden="true" />
                <span class="chat-checks-label"
                    >{{ tr(`chat.checksReading.${reading.kind}`) }} {{ reading.label }}</span
                >
                <span class="chat-checks-detail">{{ reading.detail }}</span>
                <button
                    v-if="reading.element || reading.branch"
                    type="button"
                    class="small"
                    :aria-label="
                        tr('chat.checksLocate', {
                            name: reading.element ?? reading.branch ?? '',
                        })
                    "
                    :title="
                        tr('chat.checksLocate', {
                            name: reading.element ?? reading.branch ?? '',
                        })
                    "
                    @click="emit('locate', target(reading))"
                >
                    <LocateFixed :size="12" />
                </button>
            </li>
        </ul>
        <p v-if="checks.readingsMore" class="chat-checks-more">
            {{ tr('chat.checksReadingsMore', { count: checks.readingsMore }) }}
        </p>
    </section>
</template>

<style scoped>
.chat-checks {
    margin: 6px 0 0;
    border: 1px solid var(--border);
    border-left: 3px solid var(--danger);
    border-radius: var(--radius-sm);
    background: var(--surface);
    font-size: 12px;
}
.chat-checks.is-pass {
    border-left-color: var(--positive);
}
.chat-checks-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 5px 8px;
}
.chat-checks-badge {
    flex: 1;
    white-space: nowrap;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 600;
    color: var(--danger);
}
.is-pass .chat-checks-badge {
    color: var(--positive);
}
.chat-checks-counts,
.chat-checks-origin {
    color: var(--muted);
}
.chat-checks-origin {
    font-size: 11px;
}
.chat-checks-counts {
    order: 3;
    flex-basis: 100%;
}
.chat-checks-locate {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    white-space: nowrap;
}
.chat-checks-items {
    list-style: none;
    margin: 0;
    padding: 0 8px 6px;
    display: grid;
    gap: 3px;
}
.chat-checks-items li {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: baseline;
    gap: 6px;
    padding-left: 8px;
    border-left: 2px solid var(--danger);
}
.chat-checks-items li.is-warning {
    border-left-color: var(--warning);
}
.chat-checks-label {
    font-family: var(--font-mono);
    font-weight: 600;
}
.chat-checks-detail {
    color: var(--muted);
    overflow-wrap: anywhere;
}
.chat-checks-readings {
    list-style: none;
    margin: 0;
    padding: 4px 8px 6px;
    border-top: 1px solid var(--border);
    display: grid;
    gap: 3px;
}
.chat-checks-readings li {
    display: grid;
    grid-template-columns: auto auto 1fr auto;
    align-items: baseline;
    gap: 6px;
}
.chat-checks-readings li.is-ok > svg {
    color: var(--positive);
}
.chat-checks-readings li.is-off > svg {
    color: var(--danger);
}
.chat-checks-more {
    margin: 0;
    padding: 0 8px 6px;
    color: var(--muted);
}
</style>
