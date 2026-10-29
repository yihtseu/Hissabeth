<script setup lang="ts">
import { computed } from 'vue';

export interface GridGroup {
    label: string;
    cols: string[];
}
export interface GridRow {
    label: string;
    /** One display string per column across all groups. */
    cells: string[];
    /** Highlights the terminal failed row. */
    failed?: boolean;
    title?: string;
}

const props = defineProps<{
    caption: string;
    first: string;
    groups: GridGroup[];
    rows: GridRow[];
}>();

/** Column indexes where a group begins, for the vertical separators. */
const groupStarts = computed(() => {
    const starts = new Set<number>();
    let k = 0;
    for (const g of props.groups) {
        starts.add(k);
        k += g.cols.length;
    }
    return starts;
});
</script>

<template>
    <div class="table-scroll" role="region" :aria-label="caption" tabindex="0">
        <table class="wide-table ray-table">
            <caption>
                {{
                    caption
                }}
            </caption>
            <thead>
                <tr>
                    <th scope="col" rowspan="2">{{ first }}</th>
                    <th
                        v-for="g in groups"
                        :key="g.label"
                        scope="colgroup"
                        :colspan="g.cols.length"
                        class="group-start"
                    >
                        {{ g.label }}
                    </th>
                </tr>
                <tr>
                    <template v-for="g in groups" :key="g.label">
                        <th
                            v-for="(c, i) in g.cols"
                            :key="c"
                            scope="col"
                            :class="{ 'group-start': i === 0 }"
                        >
                            {{ c }}
                        </th>
                    </template>
                </tr>
            </thead>
            <tbody>
                <tr v-for="(r, i) in rows" :key="i" :class="{ failed: r.failed }" :title="r.title">
                    <th scope="row">{{ r.label }}</th>
                    <td
                        v-for="(c, k) in r.cells"
                        :key="k"
                        :class="{ 'group-start': groupStarts.has(k) }"
                    >
                        {{ c }}
                    </td>
                </tr>
            </tbody>
        </table>
    </div>
</template>
