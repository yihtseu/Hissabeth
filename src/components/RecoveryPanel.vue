<script setup lang="ts">
import { tr } from '../i18n';
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
    {
        id: 'hissabeth',
        get label() {
            return tr('remaining.m0');
        },
    },
    { id: 'geopter', label: 'Geopter' },
];

const fileError = ref('');

const canAct = computed(() => props.online && !props.busy);
const canRestore = computed(() => canAct.value && props.text.trim().length > 0);

const sourceInfo = computed(() => {
    const r = props.recovery;
    if (r.source_bytes === null) return tr('recovery.sourceUnreadable');
    // An existing empty file reports its text as '' with 0 bytes; a missing file reports no text.
    if (r.source_bytes === 0)
        return r.source_text === null ? tr('recovery.sourceMissing') : tr('recovery.sourceEmpty');
    return tr('recovery.sourceBytes', { n: r.source_bytes.toLocaleString() });
});
const textHelp = computed(() => {
    const r = props.recovery;
    if (r.source_text !== null) return '';
    if (r.source_bytes === null) return tr('recovery.textHiddenUnreadable');
    if (r.source_bytes === 0) return '';
    return tr('recovery.textHiddenUtf8');
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
    <section class="panel" :aria-label="tr('recovery.title')">
        <fieldset class="recovery-panel" :disabled="busy">
            <legend>{{ tr('recovery.title') }}</legend>
            <div role="alert" class="banner error">{{ error || recovery.reason }}</div>
            <p class="note">
                {{ tr('recovery.savedPath') }} <code>{{ recovery.path }}</code
                ><br />
                {{ sourceInfo }}
                {{ tr('recovery.note') }}
            </p>
            <p v-if="textHelp" class="note">{{ textHelp }}</p>
            <div class="controls">
                <label for="recovery-format"
                    >{{ tr('recovery.format') }}
                    <select
                        id="recovery-format"
                        :value="format"
                        @change="
                            emit(
                                'update:format',
                                ($event.target as HTMLSelectElement).value as ImportFormat,
                            )
                        "
                    >
                        <option v-for="f in FORMATS" :key="f.id" :value="f.id">
                            {{ f.label }}
                        </option>
                    </select>
                </label>
                <label
                    >{{ tr('recovery.openFile') }}
                    <input
                        type="file"
                        accept=".json,.txt,application/json,text/plain"
                        @change="onFile"
                /></label>
            </div>
            <label for="recovery-text" class="block-label">{{ tr('recovery.textLabel') }}</label>
            <textarea
                id="recovery-text"
                :value="text"
                class="paste tall"
                spellcheck="false"
                aria-describedby="recovery-hint recovery-file-error"
                @input="emit('update:text', ($event.target as HTMLTextAreaElement).value)"
                @keydown="onTextKey"
            />
            <span id="recovery-hint" class="hint">{{ tr('recovery.hint') }}</span>
            <div class="controls">
                <button
                    type="button"
                    class="primary"
                    :disabled="!canRestore"
                    @click="emit('restore')"
                >
                    {{ tr('recovery.restore') }}
                </button>
                <button type="button" :disabled="!canAct" @click="emit('retry')">
                    {{ tr('recovery.retry') }}
                </button>
                <button type="button" :disabled="!canAct" @click="emit('create')">
                    {{ tr('recovery.create') }}
                </button>
            </div>
            <p id="recovery-file-error" role="alert" class="json-error">{{ fileError }}</p>
        </fieldset>
    </section>
</template>
