export interface ChartSeries {
  label: string;
  color: string;
  dashed?: boolean;
  /** y === null marks an invalid sample: the line breaks there. */
  points: { x: number; y: number | null }[];
}

/** Round, evenly spaced tick values covering [min, max]. */
export function niceTicks(min: number, max: number, count = 5): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [0, 1];
  if (max - min < 1e-300) {
    const pad = Math.abs(min) > 0 ? Math.abs(min) * 0.1 : 1;
    min -= pad;
    max += pad;
  }
  const raw = (max - min) / Math.max(1, count);
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const frac = raw / mag;
  const step = (frac < 1.5 ? 1 : frac < 3 ? 2 : frac < 7 ? 5 : 10) * mag;
  const ticks: number[] = [];
  for (let t = Math.ceil(min / step) * step; t <= max + step * 1e-9; t += step) {
    ticks.push(Math.abs(t) < step * 1e-9 ? 0 : t);
  }
  return ticks;
}

/** Compact numeric label for axes and tables. */
export function fmt(v: number, digits = 3): string {
  if (v === 0) return '0';
  const a = Math.abs(v);
  return a >= 1e5 || a < 1e-3 ? v.toExponential(digits - 1) : String(Number(v.toPrecision(digits)));
}

export function extent(values: Iterable<number | null | undefined>): [number, number] | null {
  let lo = Infinity, hi = -Infinity;
  for (const v of values) {
    if (v === null || v === undefined || !Number.isFinite(v)) continue;
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  return lo <= hi ? [lo, hi] : null;
}
