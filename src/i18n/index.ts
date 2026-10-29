import { createI18n } from 'vue-i18n';
import english from './en-US';
import chinese from './zh-CN';
export const i18n = createI18n({
    legacy: false,
    locale: 'zh-CN',
    fallbackLocale: false,
    messages: { 'en-US': english, 'zh-CN': chinese },
});
export function tr(key: string, parameters: Record<string, string | number> = {}): string {
    return i18n.global.t(key, parameters);
}
