<script setup lang="ts">
import { tr } from '../i18n';
import type { AgentInfo, AgentRegistration } from '../types';

defineProps<{ agents: AgentInfo[]; registration: AgentRegistration[] }>();
const emit = defineEmits<{ connect: [] }>();
</script>

<template>
    <section class="panel connections" aria-labelledby="agents-title">
        <h3 id="agents-title">{{ tr('agents.title') }}</h3>
        <p>
            {{
                tr(
                    'fragments.keepHissabethOpenThenAskYourAgentToReadTheOpticalSystemAndDesignALens',
                )
            }}
        </p>
        <div v-for="a in agents" :key="a.name" class="agent">
            <strong>{{ a.name === 'codex' ? tr('agents.codex') : tr('agents.claude') }}</strong>
            <span>{{ a.installed ? tr('agents.detected') : tr('agents.notInstalled') }}</span>
            <code>{{ a.executable || tr('agents.installCli') }}</code>
        </div>
        <button type="button" class="primary" @click="emit('connect')">
            {{ tr('fragments.registerDetectedAgents') }}
        </button>
        <div role="status">
            <pre v-for="r in registration" :key="r.name"
                >{{ r.name }}: {{
                    r.success ? tr('agents.registered') : r.message || r.status
                }}</pre>
        </div>
        <p>
            {{ tr('agents.outro') }}
        </p>
    </section>
</template>
