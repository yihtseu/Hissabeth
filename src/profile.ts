/** Common shape of an internal surface and an endpoint profile. */
export interface Profile {
  radius: number;
  conic: number;
  even: number[];
  odd: number[];
  aperture: number;
  clear_aperture: boolean;
  semi_diameter: number;
}

/** Sag matching the backend: conic base + even terms r^(4+2i) + odd terms r^(3+i). Null where the profile is undefined. */
export function sag(p: Profile, r: number): number | null {
  const c = p.radius === 0 ? 0 : 1 / p.radius;
  const q = 1 - (1 + p.conic) * c * c * r * r;
  if (q < 0) return null;
  let z = (c * r * r) / (1 + Math.sqrt(q));
  p.even.forEach((a, k) => (z += a * Math.pow(r, 4 + 2 * k)));
  p.odd.forEach((a, k) => (z += a * Math.pow(r, 3 + k)));
  return Number.isFinite(z) ? z : null;
}

/** Drawn half-height: the derived envelope, or the physical clear radius when it clips and is larger. */
export const drawRadius = (p: Profile): number => Math.max(p.semi_diameter, p.clear_aperture ? p.aperture : 0);

/** Samples (z, y) of the profile over its drawn half-height, with its vertex at `vertexZ`. */
export function profileCurve(p: Profile, vertexZ: number, steps = 40): [number, number][] {
  const out: [number, number][] = [];
  const h = drawRadius(p);
  for (let k = -steps; k <= steps; k++) {
    const y = (h * k) / steps;
    const z = sag(p, Math.abs(y));
    if (z !== null) out.push([vertexZ + z, y]);
  }
  return out;
}
