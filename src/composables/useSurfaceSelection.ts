import { ref, type Ref } from 'vue';

/**
 * Module-level shared surface-selection state.
 *
 * Shared indexes identify internal surfaces in the active draft. Views of the saved system
 * translate between saved and draft indexes while insertions or removals are pending.
 * Object and image endpoints are outside this state.
 */
const hovered = ref<number | null>(null);
const selected = ref<number | null>(null);
/**
 * The workbench reports the current draft surface count through `syncSurfaceSelection`.
 * It starts unbounded until the workbench loads a system.
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
