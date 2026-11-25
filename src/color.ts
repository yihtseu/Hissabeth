import type { RGBA } from './types';

const byte = (v: number): number => Math.max(0, Math.min(255, Math.round(v * 255)));

/** CSS color from the saved RGB; alpha is stored metadata and does not affect plot visibility. */
export const rgbCss = (c: RGBA): string => `rgb(${byte(c[0])},${byte(c[1])},${byte(c[2])})`;

export const rgbToHex = (c: RGBA): string =>
    `#${[c[0], c[1], c[2]].map((v) => byte(v).toString(16).padStart(2, '0')).join('')}`;

/** New RGBA from a `#rrggbb` color input value; the stored alpha is kept exactly. */
export function withHex(current: RGBA, hex: string): RGBA | null {
    const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
    if (!m) return null;
    if (rgbToHex(current).toLowerCase() === hex.toLowerCase()) return null; // unchanged: keep full stored precision
    return [
        parseInt(m[1]!, 16) / 255,
        parseInt(m[2]!, 16) / 255,
        parseInt(m[3]!, 16) / 255,
        current[3],
    ];
}

/** Visible defaults for new rows, cycled by row number; alpha 1. */
const PALETTE: RGBA[] = [
    [0.78, 0.42, 0.24, 1],
    [0.2, 0.49, 0.62, 1],
    [0.55, 0.44, 0.7, 1],
    [0.3, 0.62, 0.53, 1],
    [0.69, 0.54, 0.18, 1],
    [0.66, 0.31, 0.47, 1],
];
export const paletteColor = (n: number): RGBA =>
    [...PALETTE[((n % PALETTE.length) + PALETTE.length) % PALETTE.length]!] as RGBA;

/** Stand-in when a color is unavailable (for example before a request captured one). */
export const FALLBACK_COLOR: RGBA = [0.2, 0.49, 0.62, 1];
