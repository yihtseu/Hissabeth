import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { openUrl } from '@tauri-apps/plugin-opener';
import { i18n } from '../i18n';
import AboutSection, { type LicenseGroup } from './AboutSection.vue';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
// serde is used under two license texts, so it is listed twice but counted once.
vi.mock('../assets/licenses-rust.json', () => ({
    default: [
        {
            license: 'Apache-2.0',
            text: 'Apache License text',
            packages: [
                { name: 'serde', version: '1.0.0', source: 'https://github.com/serde-rs/serde' },
            ],
        },
        {
            license: 'MIT',
            text: 'MIT License text',
            packages: [
                { name: 'serde', version: '1.0.0', source: 'https://github.com/serde-rs/serde' },
                { name: 'log', version: '0.4.0', source: 'https://github.com/rust-lang/log' },
            ],
        },
    ] satisfies LicenseGroup[],
}));
vi.mock('../assets/licenses-javascript.json', () => ({
    default: [
        {
            license: 'MIT',
            text: '',
            packages: [{ name: 'vue', version: '3.5.0', source: 'https://github.com/vuejs/core' }],
        },
    ] satisfies LicenseGroup[],
}));

// Resolves once the lazily imported component lists have rendered.
async function render(loaded = '2 Rust crates') {
    const wrapper = mount(AboutSection, { attachTo: document.body });
    await vi.waitFor(() => expect(wrapper.text()).toContain(loaded));
    return wrapper;
}
async function expand(wrapper: Awaited<ReturnType<typeof render>>, label: string) {
    const trigger = wrapper.findAll('button').find((b) => b.text().includes(label));
    expect(trigger, label).toBeDefined();
    await trigger!.trigger('click');
    await flushPromises();
}

describe('AboutSection', () => {
    beforeEach(() => {
        i18n.global.locale.value = 'en-US';
        vi.mocked(openUrl).mockReset().mockResolvedValue();
    });
    afterEach(() => {
        document.body.innerHTML = '';
    });

    it('states the version, license, Geopter attribution and fan-work notice', async () => {
        const wrapper = await render();
        const text = wrapper.text();
        expect(text).toContain(`Version ${__APP_VERSION__}`);
        expect(text).toContain('GNU General Public License, version 3 or later');
        expect(text).toContain('Copyright (C) 2021 Hiiragi');
        expect(text).toContain('0edfbf52fcf0e3fc660e5ae33c354fd7a6be80d7');
        expect(text).toContain('not affiliated with, endorsed by, or sponsored by Bluepoch');
        expect(text).toContain('2 Rust crates and 1 JavaScript packages');
    });

    it('shows each license group with its packages and full text', async () => {
        const wrapper = await render();
        await expand(wrapper, 'Rust crates (2)');
        await expand(wrapper, 'MIT');
        expect(wrapper.text()).toContain('MIT License text');
        expect(wrapper.text()).toContain('log');
        expect(wrapper.text()).not.toContain('Apache License text');
        await expand(wrapper, 'JavaScript packages (1)');
        const groups = wrapper.findAll('button').filter((b) => b.text().startsWith('MIT'));
        await groups[groups.length - 1].trigger('click');
        await flushPromises();
        expect(wrapper.text()).toContain('The package ships no license text');
    });

    it('opens links in the system browser and reports failures', async () => {
        const wrapper = await render();
        await wrapper.get('a[href^="https://github.com/yihtseu/Hissabeth"]').trigger('click');
        expect(openUrl).toHaveBeenCalledWith(
            expect.stringMatching(
                /^https:\/\/github\.com\/yihtseu\/Hissabeth(\/tree\/[0-9a-f]{40})?$/,
            ),
        );
        vi.mocked(openUrl).mockRejectedValueOnce(new Error('no browser'));
        await wrapper.get('a[href="https://github.com/heterophyllus/Geopter"]').trigger('click');
        await flushPromises();
        expect(wrapper.get('[role="alert"]').text()).toContain('no browser');
    });

    it('is translated into Chinese', async () => {
        i18n.global.locale.value = 'zh-CN';
        const wrapper = await render('2 个 Rust crate');
        expect(wrapper.text()).toContain('与 Bluepoch 无关联');
    });
});
