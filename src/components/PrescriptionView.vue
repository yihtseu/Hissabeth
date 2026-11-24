<script setup lang="ts">
import { computed } from 'vue';
import { tr } from '../i18n';
import type { OpticalSystem } from '../types';
const props = defineProps<{ system: OpticalSystem; compact?: boolean; chatOpen?: boolean }>();
const rows = computed(() => {
    let z = 0;
    let material = props.system.object_material;
    return props.system.surfaces.map((surface, i) => {
        const vertex = z;
        z += surface.thickness;
        if (surface.interaction === 'refract') material = surface.material;
        return {
            label: surface.label,
            interaction: surface.interaction,
            radius: surface.radius,
            thickness: surface.thickness,
            clear_aperture: surface.clear_aperture,
            aperture: surface.aperture,
            conic: surface.conic,
            vertex,
            material,
            stop: i === props.system.stop_index,
        };
    });
});
const number = (value: number) => Number(value.toPrecision(10)).toString();
</script>
<template>
    <section class="prescription-view">
        <p v-if="!compact" class="optical-context">
            {{ tr('optical.currentPrescription')
            }}<template v-if="!chatOpen"> · {{ tr('optical.chatEdits') }}</template>
        </p>
        <div class="table-scroll" tabindex="0" :aria-label="tr('tabs.Prescription')">
            <table>
                <thead>
                    <tr>
                        <th>#</th>
                        <th>{{ tr('bench.label') }}</th>
                        <th>{{ tr('optical.interaction') }}</th>
                        <th>{{ tr('optical.radius') }} (mm)</th>
                        <th>{{ tr('optical.gap') }} (mm)</th>
                        <th>{{ tr('optical.material') }}</th>
                        <th>{{ tr('bench.aperture') }} (mm)</th>
                        <th>{{ tr('optical.vertex') }} (mm)</th>
                        <th>{{ tr('optical.conic') }}</th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="(s, i) in rows" :key="i">
                        <th scope="row">{{ i + 1 }}{{ s.stop ? ' ◉' : '' }}</th>
                        <td>{{ s.label || '—' }}</td>
                        <td>{{ tr('optical.' + s.interaction) }}</td>
                        <td>{{ number(s.radius) }}</td>
                        <td>{{ number(s.thickness) }}</td>
                        <td>{{ s.material }}</td>
                        <td>{{ s.clear_aperture ? number(s.aperture) : '—' }}</td>
                        <td>{{ number(s.vertex) }}</td>
                        <td>{{ number(s.conic) }}</td>
                    </tr>
                </tbody>
            </table>
        </div>
        <template v-if="!compact">
            <dl class="optical-properties prescription-spec">
                <dt>{{ tr('optical.object') }}</dt>
                <dd>
                    {{ system.object_label }} ·
                    {{ system.object_distance === null ? '∞' : system.object_distance + ' mm' }} ·
                    {{ system.object_material }}
                </dd>
                <dt>{{ tr('optical.image') }}</dt>
                <dd>{{ system.image_label }}</dd>
                <dt>{{ tr('optical.pupil') }}</dt>
                <dd>
                    {{ system.pupil.kind }}: {{ system.pupil.value }} · {{ system.pupil_mapping }}
                </dd>
                <dt>{{ tr('optical.environment') }}</dt>
                <dd>{{ system.temperature }} °C · {{ system.pressure }} Pa</dd>
                <template v-if="system.configurations?.length">
                    <dt>{{ tr('cfg.title') }}</dt>
                    <dd>
                        {{
                            tr('cfg.activeLine', {
                                name:
                                    system.configurations[system.active_configuration ?? 0]?.name ||
                                    '—',
                                i: (system.active_configuration ?? 0) + 1,
                                n: system.configurations.length,
                            })
                        }}
                    </dd>
                </template>
                <dt>{{ tr('optical.wavelength') }}</dt>
                <dd>
                    {{
                        system.wavelengths
                            .map(
                                (w, i) =>
                                    `${w.value} nm (${w.weight})${i === system.reference_wavelength ? ' ◉' : ''}`,
                            )
                            .join(' / ')
                    }}
                </dd>
                <dt>{{ tr('optical.fields') }}</dt>
                <dd>
                    {{ system.field_type }}:
                    {{ system.fields.map((f) => `(${f.x}, ${f.y}; ${f.weight})`).join(' / ') }}
                </dd>
            </dl>
            <p v-if="system.note">{{ system.note }}</p>
            <p>{{ tr('optical.fullReport') }}</p>
        </template>
    </section>
</template>
