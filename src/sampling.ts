import { tr } from './i18n';
import type { PupilMapping, PupilSampling, Spectrum } from './types';

export const GRID_RANGE = { min: 3, max: 129 } as const;
export const RING_RANGE = { min: 1, max: 64 } as const;

export const SPOT_SAMPLING: PupilSampling = { kind: 'grid', count: 25 };
export const GEOMETRIC_SAMPLING: PupilSampling = { kind: 'grid', count: 33 };

export type SpectralMode = 'mono' | 'poly';

export function describeSampling(s: PupilSampling): string {
    return s.kind === 'grid'
        ? tr('descriptions.samplingGrid', { count: s.count })
        : tr('descriptions.samplingHex', { count: s.rings });
}

export function describeSpectrum(s: Spectrum): string {
    return s.kind === 'monochromatic' ? `${s.wavelength} nm` : tr('descriptions.weighted');
}

export const describeMapping = (m: PupilMapping): string =>
    m === 'real_stop' ? tr('descriptions.realStop') : tr('descriptions.entrancePupil');

export const percent = (v: number): string => `${(v * 100).toFixed(1)}%`;
