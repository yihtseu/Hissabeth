import { ref, type Ref } from 'vue';

/**
 * Module-level shared surface-selection state.
 *
 * Internal surfaces are numbered 0..N-1 (zero-based indexes into `OpticalSystem.surfaces`);
 * endpoints (object/image) are never part of this state. Every view (SurfaceTable,
 * PrescriptionView, LayoutView, MaterialsPanel) imports this module directly, so all of them
 * observe the same hover/selection without prop drilling.
 */
const hovered = ref<number | null>(null);
const selected = ref<number | null>(null);
/**
 * Known internal surface count. Starts unbounded so early calls stay valid; views that know
 * the current system should report it through `syncSurfaceSelection` so out-of-range indexes
 * are dropped as soon as the underlying data changes.
 */
let knownSurfaceCount = Number.POSITIVE_INFINITY;

const inRange = (index: number): boolean =>
    Number.isInteger(index) && index >= 0 && index < knownSurfaceCount;

/** Currently hovered internal surface index, or null. */
export const hoveredSurfaceIndex: Ref<number | null> = hovered;
/** Currently selected (pinned) internal surface index, or null. */
export const selectedSurfaceIndex: Ref<number | null> = selected;

/** Set or clear the hover highlight; out-of-range indexes are treated as null. */
export function hoverSurface(index: number | null): void {
    const next = index !== null && inRange(index) ? index : null;
    if (hovered.value !== next) hovered.value = next;
}

/** Pin a surface as selected; a stale/out-of-range index clears the selection instead. */
export function selectSurface(index: number): void {
    selected.value = inRange(index) ? index : null;
}

/** Reset the shared selection state (hover and selection). */
export function clearSurfaceSelection(): void {
    hovered.value = null;
    selected.value = null;
}

/**
 * Housekeeping: report how many internal surfaces the current system has. Indexes outside
 * the new range are dropped immediately so no view can render a stale highlight or HUD.
 */
export function syncSurfaceSelection(surfaceCount: number): void {
    knownSurfaceCount = Math.max(0, Math.floor(surfaceCount));
    if (hovered.value !== null && !inRange(hovered.value)) hovered.value = null;
    if (selected.value !== null && !inRange(selected.value)) selected.value = null;
}
