import { watchEffect } from 'vue';
import { usePreferredDark, usePreferredReducedMotion } from '@vueuse/core';
import { usePreferencesStore } from '../stores/preferences';
import { i18n } from '../i18n';
export function useAppearance(): void {
    const store = usePreferencesStore();
    const dark = usePreferredDark();
    const motion = usePreferredReducedMotion();
    watchEffect(() => {
        const { appearance, locale } = store.preferences;
        const root = document.documentElement;
        root.lang = locale;
        i18n.global.locale.value = locale;
        root.dataset.theme =
            appearance.theme === 'system' ? (dark.value ? 'dark' : 'light') : appearance.theme;
        root.dataset.accent = appearance.accent;
        root.dataset.density = appearance.density;
        root.dataset.motion =
            appearance.reducedMotion || motion.value === 'reduce' ? 'reduced' : 'full';
        root.style.setProperty('--ui-font-size', `${appearance.fontSize}px`);
    });
}
