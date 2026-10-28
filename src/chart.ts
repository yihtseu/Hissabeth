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

/** Approximate display colour of a wavelength in nm; UV/IR are shown as dim violet/red. Darkened for white backgrounds. */
export function wavelengthColor(nm: number): string {
  let r = 0, g = 0, b = 0;
  if (nm < 440) { r = Math.max(0, (440 - nm) / 60); b = 1; }
  else if (nm < 490) { g = (nm - 440) / 50; b = 1; }
  else if (nm < 510) { g = 1; b = (510 - nm) / 20; }
  else if (nm < 580) { r = (nm - 510) / 70; g = 1; }
  else if (nm < 645) { r = 1; g = (645 - nm) / 65; }
  else { r = 1; }
  const edge = nm < 380 ? 0.5 : nm < 420 ? 0.5 + (0.5 * (nm - 380)) / 40 : nm > 780 ? 0.5 : nm > 700 ? 0.5 + (0.5 * (780 - nm)) / 80 : 1;
  const c = (v: number): number => Math.round(255 * Math.pow(v * edge, 0.8) * 0.85);
  return `rgb(${c(r)},${c(g)},${c(b)})`;
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
