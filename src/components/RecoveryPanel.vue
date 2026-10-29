<script setup lang="ts">
import { computed, ref } from 'vue';
import { errorMessage } from '../api';
import type { ImportFormat, RecoveryWorkspace } from '../types';

const props = defineProps<{
  recovery: RecoveryWorkspace;
  online: boolean;
  busy: boolean;
  error: string;
  text: string;
  format: ImportFormat;
}>();
const emit = defineEmits<{
  'update:text': [value: string];
  'update:format': [value: ImportFormat];
  retry: [];
  restore: [];
  create: [];
}>();

const FORMATS: { id: ImportFormat; label: string }[] = [
  { id: 'hissabeth', label: 'Hissabeth project' },
  { id: 'geopter', label: 'Geopter' },
];

const fileError = ref('');

const canAct = computed(() => props.online && !props.busy);
const canRestore = computed(() => canAct.value && props.text.trim().length > 0);

const sourceInfo = computed(() => {
  const r = props.recovery;
  if (r.source_bytes === null) return 'The file at this path could not be read.';
  // An existing empty file reports its text as '' with 0 bytes; a missing file reports no text.
  if (r.source_bytes === 0)
    return r.source_text === null
      ? 'No file exists at this path.'
      : 'The file at this path is empty (0 bytes).';
  return `The file at this path holds ${r.source_bytes.toLocaleString()} bytes.`;
});
const textHelp = computed(() => {
  const r = props.recovery;
  if (r.source_text !== null) return '';
  if (r.source_bytes === null)
    return 'The original text cannot be shown. Fix the file on disk and use Retry from disk, or paste a replacement below.';
  if (r.source_bytes === 0) return '';
  return 'The original file is not valid UTF-8, so it cannot be shown or edited here. Paste a replacement below, or fix the file on disk and use Retry from disk.';
});

/** Local file read only; the text is sent to the engine on Restore from text, not before. */
async function onFile(e: Event): Promise<void> {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;
  try {
    emit('update:text', await file.text());
    fileError.value = '';
  } catch (err) {
    fileError.value = errorMessage(err);
  }
  input.value = '';
}

function onTextKey(e: KeyboardEvent): void {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter' && canRestore.value) {
    e.preventDefault();
    emit('restore');
  }
}
</script>

<template>
  <section class="panel" aria-label="Workspace recovery">
    <fieldset class="recovery-panel" :disabled="busy">
      <legend>Workspace recovery</legend>
      <div role="alert" class="banner error">{{ error || recovery.reason }}</div>
      <p class="note">
        Saved project path: <code>{{ recovery.path }}</code><br />
        {{ sourceInfo }}
        Editing or choosing a file below changes nothing on disk; the engine validates the full text before any replacement, and Create new system moves the original aside and reports where.
      </p>
      <p v-if="textHelp" class="note">{{ textHelp }}</p>
      <div class="controls">
        <label for="recovery-format">Recovery format
          <select id="recovery-format" :value="format" @change="emit('update:format', ($event.target as HTMLSelectElement).value as ImportFormat)">
            <option v-for="f in FORMATS" :key="f.id" :value="f.id">{{ f.label }}</option>
          </select>
        </label>
        <label>Open file <input type="file" accept=".json,.txt,application/json,text/plain" @change="onFile" /></label>
      </div>
      <label for="recovery-text" class="block-label">Recovery project text</label>
      <textarea
        id="recovery-text"
        :value="text"
        class="paste tall"
        spellcheck="false"
        aria-describedby="recovery-hint recovery-file-error"
        @input="emit('update:text', ($event.target as HTMLTextAreaElement).value)"
        @keydown="onTextKey"
      />
      <span id="recovery-hint" class="hint">Ctrl/Cmd+Enter restores from the text above. The engine reports any semantic error without clearing your text.</span>
      <div class="controls">
        <button type="button" class="primary" :disabled="!canRestore" @click="emit('restore')">Restore from text</button>
        <button type="button" :disabled="!canAct" @click="emit('retry')">Retry from disk</button>
        <button type="button" :disabled="!canAct" @click="emit('create')">Create new system</button>
      </div>
      <p id="recovery-file-error" role="alert" class="json-error">{{ fileError }}</p>
    </fieldset>
  </section>
</template>
