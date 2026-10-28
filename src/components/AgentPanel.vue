<script setup lang="ts">
import type { AgentInfo, AgentRegistration } from '../types';

defineProps<{ agents: AgentInfo[]; registration: AgentRegistration[] }>();
const emit = defineEmits<{ connect: [] }>();
</script>

<template>
  <section class="panel connections" aria-labelledby="agents-title">
    <h3 id="agents-title">Work from your terminal</h3>
    <p>Keep Hissabeth open, then ask your agent to read the optical system and design a lens.</p>
    <div v-for="a in agents" :key="a.name" class="agent">
      <strong>{{ a.name === 'codex' ? 'Codex' : 'Claude Code' }}</strong>
      <span>{{ a.installed ? 'Detected' : 'Not installed' }}</span>
      <code>{{ a.executable || 'Install the CLI to connect' }}</code>
    </div>
    <button type="button" class="primary" @click="emit('connect')">Register detected agents</button>
    <div role="status">
      <pre v-for="r in registration" :key="r.name">{{ r.name }}: {{ r.success ? 'Registered' : r.message || r.status }}</pre>
    </div>
    <p>Tools operate on this window's system. Changes appear automatically. Prescriptions include fields, pupil specification, finite or infinite conjugates and custom glasses.</p>
  </section>
</template>
