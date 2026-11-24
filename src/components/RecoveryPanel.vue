<script setup lang="ts">
import { tr } from '../i18n';
import type { RecoveryWorkspace } from '../types';
defineProps<{ recovery: RecoveryWorkspace; chatOpen?: boolean }>();
const emit = defineEmits<{ chat: [] }>();
</script>
<template>
    <section class="panel recovery-panel" :aria-label="tr('recovery.title')">
        <h3>{{ tr('recovery.title') }}</h3>
        <p class="banner error" role="alert">{{ recovery.reason }}</p>
        <p>
            {{ tr('recovery.savedPath') }} <code>{{ recovery.path }}</code>
        </p>
        <p>{{ tr('optical.recoveryHelp') }}</p>
        <button v-if="!chatOpen" @click="emit('chat')">{{ tr('optical.openChat') }}</button>
    </section>
</template>
