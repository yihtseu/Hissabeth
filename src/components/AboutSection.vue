<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
    AccordionContent,
    AccordionHeader,
    AccordionItem,
    AccordionRoot,
    AccordionTrigger,
} from 'reka-ui';
import { ChevronRight } from 'lucide-vue-next';
import { tr } from '../i18n';
import { errorMessage } from '../api';

/** One license text and the packages used under it, as `bun run licenses` writes it. */
export type LicenseGroup = {
    license: string;
    text: string;
    packages: { name: string; version: string; source: string }[];
};

const SOURCE = 'https://github.com/yihtseu/Hissabeth';
const GEOPTER = 'https://github.com/heterophyllus/Geopter';
const GEOPTER_COMMIT = '0edfbf52fcf0e3fc660e5ae33c354fd7a6be80d7';
const version = __APP_VERSION__;
const commit = __APP_COMMIT__;
const licenseUrl = `${SOURCE}/blob/${commit || 'main'}/LICENSE.md`;
const sourceUrl = commit ? `${SOURCE}/tree/${commit}` : SOURCE;

const inventory = ref<{ rust: LicenseGroup[]; javascript: LicenseGroup[] }>();
const loadError = ref('');
const openError = ref('');
onMounted(async () => {
    try {
        const [rust, javascript] = await Promise.all([
            import('../assets/licenses-rust.json'),
            import('../assets/licenses-javascript.json'),
        ]);
        inventory.value = { rust: rust.default, javascript: javascript.default };
    } catch (error) {
        loadError.value = errorMessage(error);
    }
});

// A package used under several license texts appears in each group but counts once.
function packageCount(groups: LicenseGroup[]): number {
    return new Set(groups.flatMap((g) => g.packages.map((p) => `${p.name}@${p.version}`))).size;
}
const ecosystems = computed(() => {
    const lists = inventory.value;
    if (!lists) return [];
    return [
        { id: 'rust', label: 'aboutRust', groups: lists.rust, count: packageCount(lists.rust) },
        {
            id: 'javascript',
            label: 'aboutJavascript',
            groups: lists.javascript,
            count: packageCount(lists.javascript),
        },
    ] as const;
});

async function open(url: string): Promise<void> {
    openError.value = '';
    try {
        await openUrl(url);
    } catch (error) {
        openError.value = tr('settings.aboutOpenFailed', { url, message: errorMessage(error) });
    }
}

const link = 'tw:text-accent tw:underline-offset-2 tw:hover:underline';
const trigger =
    'tw:group tw:flex tw:w-full tw:justify-start tw:gap-2 tw:rounded-app tw:border-0 tw:bg-transparent tw:px-3 tw:py-2 tw:text-left tw:font-semibold';
const chevron =
    'tw:size-3.5 tw:shrink-0 tw:text-muted tw:transition-transform tw:group-data-[state=open]:rotate-90';
</script>

<template>
    <div class="tw:grid tw:max-w-3xl tw:grid-cols-1 tw:gap-6 tw:leading-relaxed">
        <section class="tw:grid tw:grid-cols-1 tw:gap-2">
            <p class="tw:m-0 tw:flex tw:flex-wrap tw:items-baseline tw:gap-x-3 tw:gap-y-1">
                <strong class="tw:text-base">Hissabeth</strong>
                <span>{{ tr('settings.aboutVersion', { version }) }}</span>
                <code
                    v-if="commit"
                    :title="tr('settings.aboutCommit')"
                    class="tw:text-xs tw:text-muted"
                    >{{ commit.slice(0, 12) }}</code
                >
            </p>
            <p class="tw:m-0">{{ tr('settings.aboutLicense') }}</p>
            <p class="tw:m-0 tw:flex tw:flex-wrap tw:gap-4">
                <a :href="sourceUrl" :class="link" @click.prevent="open(sourceUrl)">{{
                    tr('settings.aboutSource')
                }}</a>
                <a :href="licenseUrl" :class="link" @click.prevent="open(licenseUrl)">{{
                    tr('settings.aboutLicenseText')
                }}</a>
            </p>
            <p
                v-if="openError"
                role="alert"
                class="tw:m-0 tw:rounded-app tw:bg-danger-bg tw:px-3 tw:py-2 tw:text-danger"
            >
                {{ openError }}
            </p>
        </section>

        <section class="tw:grid tw:grid-cols-1 tw:gap-2">
            <h3>{{ tr('settings.aboutAttribution') }}</h3>
            <p class="tw:m-0">
                {{ tr('settings.aboutGeopter') }}
                <a :href="GEOPTER" :class="link" @click.prevent="open(GEOPTER)"
                    >heterophyllus/Geopter</a
                >
                <code class="tw:ml-2 tw:break-all tw:text-xs tw:text-muted">{{
                    GEOPTER_COMMIT
                }}</code>
            </p>
        </section>

        <section class="tw:grid tw:grid-cols-1 tw:gap-2">
            <h3>{{ tr('settings.aboutName') }}</h3>
            <p class="tw:m-0">{{ tr('settings.aboutNameRights') }}</p>
            <p class="tw:m-0">{{ tr('settings.aboutNameIcon') }}</p>
        </section>

        <section class="tw:grid tw:grid-cols-1 tw:gap-2">
            <h3>{{ tr('settings.aboutThirdParty') }}</h3>
            <p
                v-if="loadError"
                role="alert"
                class="tw:m-0 tw:rounded-app tw:bg-danger-bg tw:px-3 tw:py-2 tw:text-danger"
            >
                {{ tr('settings.aboutThirdPartyFailed', { message: loadError }) }}
            </p>
            <template v-else-if="inventory">
                <p class="tw:m-0">
                    {{
                        tr('settings.aboutThirdPartySummary', {
                            rust: ecosystems[0].count,
                            javascript: ecosystems[1].count,
                        })
                    }}
                </p>
                <AccordionRoot
                    type="multiple"
                    class="tw:rounded-app tw:border tw:border-border tw:bg-panel"
                >
                    <AccordionItem
                        v-for="ecosystem in ecosystems"
                        :key="ecosystem.id"
                        :value="ecosystem.id"
                        class="tw:border-b tw:border-border tw:last:border-b-0"
                    >
                        <AccordionHeader as="h4" class="tw:m-0">
                            <AccordionTrigger :class="trigger">
                                <ChevronRight :class="chevron" aria-hidden="true" />
                                {{ tr(`settings.${ecosystem.label}`, { count: ecosystem.count }) }}
                            </AccordionTrigger>
                        </AccordionHeader>
                        <AccordionContent class="tw:px-3 tw:pb-3">
                            <AccordionRoot type="multiple" class="tw:grid tw:grid-cols-1 tw:gap-1">
                                <AccordionItem
                                    v-for="(group, index) in ecosystem.groups"
                                    :key="index"
                                    :value="String(index)"
                                    class="tw:rounded-app tw:bg-surface"
                                >
                                    <AccordionHeader as="h5" class="tw:m-0 tw:text-xs">
                                        <AccordionTrigger :class="trigger">
                                            <ChevronRight :class="chevron" aria-hidden="true" />
                                            <span class="tw:shrink-0">{{ group.license }}</span>
                                            <span
                                                class="tw:min-w-0 tw:flex-[2] tw:truncate tw:font-normal tw:text-muted"
                                                >{{
                                                    group.packages.map((p) => p.name).join(', ')
                                                }}</span
                                            >
                                            <span
                                                class="tw:shrink-0 tw:font-normal tw:text-muted"
                                                >{{
                                                    tr('settings.aboutPackages', {
                                                        count: group.packages.length,
                                                    })
                                                }}</span
                                            >
                                        </AccordionTrigger>
                                    </AccordionHeader>
                                    <AccordionContent class="tw:px-3 tw:pb-3">
                                        <div class="tw:grid tw:grid-cols-1 tw:gap-2">
                                            <ul
                                                class="tw:m-0 tw:flex tw:list-none tw:flex-wrap tw:gap-x-3 tw:gap-y-1 tw:p-0 tw:text-xs"
                                            >
                                                <li
                                                    v-for="item in group.packages"
                                                    :key="`${item.name}@${item.version}`"
                                                >
                                                    <a
                                                        :href="item.source"
                                                        :class="link"
                                                        @click.prevent="open(item.source)"
                                                        >{{ item.name }}</a
                                                    >
                                                    <span class="tw:ml-1 tw:text-muted">
                                                        {{ item.version }}</span
                                                    >
                                                </li>
                                            </ul>
                                            <pre
                                                v-if="group.text"
                                                class="tw:m-0 tw:max-h-72 tw:overflow-auto tw:whitespace-pre-wrap tw:rounded-app tw:border tw:border-border tw:bg-panel tw:p-3 tw:text-[11px] tw:leading-snug"
                                                >{{ group.text }}</pre>
                                            <p v-else class="tw:m-0 tw:text-xs tw:text-muted">
                                                {{ tr('settings.aboutNoLicenseText') }}
                                            </p>
                                        </div>
                                    </AccordionContent>
                                </AccordionItem>
                            </AccordionRoot>
                        </AccordionContent>
                    </AccordionItem>
                </AccordionRoot>
            </template>
        </section>
    </div>
</template>
