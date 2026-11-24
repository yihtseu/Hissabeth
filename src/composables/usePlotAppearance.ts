import { onBeforeUnmount } from 'vue';
import { useMutationObserver } from '@vueuse/core';
export const plotColor = (token: string): string =>
    getComputedStyle(document.documentElement).getPropertyValue(token).trim();
export function usePlotAppearance(redraw: () => void): void {
    let frame = 0;
    useMutationObserver(
        document.documentElement,
        () => {
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(redraw);
        },
        { attributes: true, attributeFilter: ['data-theme', 'data-accent', 'style'] },
    );
    onBeforeUnmount(() => cancelAnimationFrame(frame));
}
