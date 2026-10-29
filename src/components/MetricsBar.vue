<script setup lang="ts">
import { tr } from '../i18n';
import { usePreferencesStore } from '../stores/preferences';
const preferences = usePreferencesStore();
import type { Paraxial, SpotDiagram } from '../types';

defineProps<{ paraxial: Paraxial; spot: SpotDiagram | null }>();

const fixed = (v: number | null, _digits: number): string =>
    v === null ? tr('common.dash') : v.toFixed(preferences.preferences.display.decimals);
</script>

<template>
    <dl class="metrics" :aria-label="tr('metrics.label')">
        <div>
            <dt :title="tr('metrics.eflTip', { basis: paraxial.matrix_basis })">
                {{ tr('metrics.efl') }}
            </dt>
            <dd>
                {{ fixed(paraxial.efl, 3) }} <small>{{ tr('metrics.eflUnit') }}</small>
            </dd>
        </div>
        <div>
            <dt :title="tr('metrics.bflTip')">
                {{ tr('metrics.bfl') }}
            </dt>
            <dd>
                {{ fixed(paraxial.bfl, 3) }}
                <small
                    >{{ tr('fragments.mmZImageLight') }}
                    {{ paraxial.image_direction < 0 ? '← −z' : '→ +z' }}</small
                >
            </dd>
        </div>
        <div>
            <dt :title="tr('metrics.wfnTip')">{{ tr('metrics.wfn') }}</dt>
            <dd>ƒ / {{ fixed(paraxial.f_number, 2) }}</dd>
        </div>
        <div>
            <dt>{{ tr('metrics.imageNa') }}</dt>
            <dd>{{ fixed(paraxial.image_na, 3) }}</dd>
        </div>
        <div>
            <dt>
                {{ tr('metrics.objectNa') }}
                <small>{{ tr('fragments.n') }} {{ fixed(paraxial.object_index, 4) }})</small>
            </dt>
            <dd>{{ fixed(paraxial.object_na, 3) }}</dd>
        </div>
        <div>
            <dt>{{ tr('metrics.ffd') }}</dt>
            <dd>{{ fixed(paraxial.front_focal_distance, 3) }} <small>mm</small></dd>
        </div>
        <div>
            <dt>{{ tr('metrics.rmsSpot') }}</dt>
            <dd>
                {{ spot ? (spot.rms_radius * 1000).toFixed(2) : tr('common.dash') }}
                <small>µm</small>
            </dd>
        </div>
    </dl>
</template>
