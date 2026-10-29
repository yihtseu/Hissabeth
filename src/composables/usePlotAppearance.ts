import { onMounted, onBeforeUnmount } from 'vue';
export const plotColor = (token: string): string =>
    getComputedStyle(document.documentElement).getPropertyValue(token).trim();
export function usePlotAppearance(redraw: () => void): void {
    let observer: MutationObserver | undefined;
    let frame = 0;
    onMounted(() => {
        observer = new MutationObserver(() => {
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(redraw);
        });
        observer.observe(document.documentElement, {
            attributes: true,
            attributeFilter: ['data-theme', 'data-accent', 'style'],
        });
    });
    onBeforeUnmount(() => {
        observer?.disconnect();
        cancelAnimationFrame(frame);
    });
}
