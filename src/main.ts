import { createApp } from 'vue';
import { createPinia } from 'pinia';
import { i18n } from './i18n';
import { usePreferencesStore } from './stores/preferences';
import './styles.css';
import './tailwind.css';
import 'splitpanes/dist/splitpanes.css';
async function boot(): Promise<void> {
    const pinia = createPinia();
    const preferences = usePreferencesStore(pinia);
    await preferences.init();
    i18n.global.locale.value = preferences.preferences.locale;
    const component =
        new URLSearchParams(location.search).get('window') === 'settings'
            ? (await import('./SettingsApp.vue')).default
            : (await import('./App.vue')).default;
    createApp(component).use(pinia).use(i18n).mount('#app');
}
void boot();
