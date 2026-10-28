import type { PupilMapping, PupilSampling, Spectrum } from './types';

export const GRID_RANGE = { min: 3, max: 129 } as const;
export const RING_RANGE = { min: 1, max: 64 } as const;

export const SPOT_SAMPLING: PupilSampling = { kind: 'grid', count: 25 };
export const GEOMETRIC_SAMPLING: PupilSampling = { kind: 'grid', count: 33 };

export type SpectralMode = 'mono' | 'poly';

export function describeSampling(s: PupilSampling): string {
  return s.kind === 'grid' ? `grid ${s.count} points across the pupil diameter` : `hexapolar, ${s.rings} ring${s.rings === 1 ? '' : 's'}`;
}

export function describeSpectrum(s: Spectrum): string {
  return s.kind === 'monochromatic' ? `${s.wavelength} nm` : 'weighted wavelengths';
}

export const describeMapping = (m: PupilMapping): string =>
  m === 'real_stop' ? 'real stop aiming' : 'reference entrance pupil';

export const percent = (v: number): string => `${(v * 100).toFixed(1)}%`;
