// Production frontend and real optical broker; the SDK channel is an explicit fixture.
import { chromium, expect } from '@playwright/test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { createConnection } from 'node:net';
import { readFile, mkdir, unlink, writeFile } from 'node:fs/promises';
import { resolve, extname } from 'node:path';
import { defaultPreferences } from './ui_preferences';
import { reviewOf } from '../../scripts/chat-figures';
const root = resolve(import.meta.dirname, '../..');
const target = resolve(root, 'src-tauri/target');
const workspace = resolve(target, 'chat-ui-workspace');
await mkdir(workspace, { recursive: true });
await unlink(resolve(workspace, 'endpoint.json')).catch(() => {});
const engine = spawn(resolve(target, 'debug/hissabeth'), ['--engine'], { env: { ...process.env, HISSABETH_DATA_DIR: workspace }, stdio: 'ignore' });
let browser!: import('@playwright/test').Browser;
let id = 0;
async function optical(name: string, args: any = {}) {
    const endpoint = JSON.parse(await readFile(resolve(workspace, 'endpoint.json'), 'utf8'));
    const [host, port] = endpoint.address.split(':');
    return new Promise<any>((done, fail) => {
        const socket = createConnection({ host, port: Number(port) });
        let buffer = '';
        socket.on('error', fail);
        socket.on('data', chunk => {
            buffer += chunk;
            if (!buffer.includes('\n')) return;
            socket.end();
            try {
                const reply = JSON.parse(buffer.split('\n')[0]);
                if (reply.result.isError) throw Error(reply.result.content[0].text);
                done(JSON.parse(reply.result.content[0].text));
            } catch (error) { fail(error); }
        });
        socket.on('connect', () => socket.write(JSON.stringify({ token: endpoint.token, request: { jsonrpc: '2.0', id: ++id, method: 'tools/call', params: { name, arguments: args } } }) + '\n'));
    });
}
const server = createServer(async (request, response) => {
    try {
        const path = new URL(request.url ?? '/', 'http://localhost').pathname;
        const file = resolve(root, 'dist', path === '/' ? 'index.html' : path.slice(1));
        if (!file.startsWith(resolve(root, 'dist') + '/')) throw Error('Invalid path');
        response.setHeader('Content-Type', { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' }[extname(file)] || 'application/octet-stream');
        response.end(await readFile(file));
    } catch { response.writeHead(404).end(); }
});
try {
    for (let count = 0; count < 100; count++) {
        try { await readFile(resolve(workspace, 'endpoint.json')); break; } catch { await new Promise(done => setTimeout(done, 50)); }
    }
    await new Promise<void>(done => server.listen(0, '127.0.0.1', () => done()));
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: { width: 1440, height: 940 } });
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    const preferences = structuredClone(defaultPreferences);
    preferences.locale = 'en-US';
    preferences.workspace.defaultView = 'Layout';
    preferences.workspace.restoreLastView = false;
    await page.exposeFunction('__host', async (command: string, args: any = {}) => {
        if (command === 'get_preferences') return { preferences, revision: 0 };
        if (command === 'plugin:event|listen') return 1;
        if (command === 'plugin:event|unlisten' || command === 'open_settings') return;
        if (command === 'optical_call') return optical(args.name, args.args);
        if (command === 'chat_runtime') return { node: '/node', agents: [{ name: 'codex', installed: true }, { name: 'claude', installed: true }] };
        if (command === 'chat_reset') return;
        // Stands in for the app's chat_figure command (tested in chat.rs).
        if (command === 'chat_figure') return (await readFile(resolve(workspace, 'chat-figures', args.file))).toString('base64');
        throw Error('Unexpected command: ' + command);
    });
    await page.addInitScript(() => {
        let callback = 0;
        window.isTauri = true;
        window.__chatRequests = [];
        window.__TAURI_INTERNALS__ = {
            invoke: async (command: string, args: any) => {
                if (command === 'chat_models') {
                    window.__modelCalls ||= [];
                    window.__modelCalls.push(args.provider);
                    if (window.__failModels) { window.__failModels = false; throw Error('Fixture catalog error'); }
                    if (window.__holdModels) { window.__holdModels = false; await new Promise(resolve => { window.__releaseModels = resolve; }); }
                    return ['a','b'].map(suffix => ({ id: `${args.provider}-fixture-${suffix}`, label: `${args.provider} ${suffix.toUpperCase()}`, description: 'Fixture model' }));
                }
                if (command === 'chat_history') return { conversations: JSON.parse(localStorage.getItem('fixture.history') || '[]'), activeId: null, activeConversationId: null };
                if (command === 'chat_start') {
                    window.__chatRequests.push(args.request);
                    window.__chatChannel = args.channel;
                    if (args.request.prompt === 'fail to start') throw Error('Fixture launch failure');
                    const records = JSON.parse(localStorage.getItem('fixture.history') || '[]');
                    let record = records.find((record: any) => record.id === args.request.conversationId);
                    if (!record) { record = { id: args.request.id, provider: args.request.provider, model: args.request.model, title: args.request.prompt, updatedAt: Date.now(), messages: [], outcome: '', error: '' }; records.push(record); }
                    record.messages.push({ id: args.request.id, kind: 'user', text: args.request.prompt });
                    window.__activeRecord = record.id;
                    window.__activeTurn = args.request.id;
                    localStorage.setItem('fixture.history', JSON.stringify(records));
                    return;
                }
                if (command === 'chat_cancel') { window.__emit({ kind: 'cancelled' }); return; }
                return window.__host(command, args);
            },
            transformCallback: () => ++callback, unregisterCallback: () => {},
            metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        };
        window.__emit = event => {
            const records = JSON.parse(localStorage.getItem('fixture.history') || '[]');
            const record = records.find((record: any) => record.id === window.__activeRecord);
            if (record && ['text','tool'].includes(event.kind)) {
                const id = `${window.__activeTurn}:${event.id}`;
                const entry = record.messages.find((entry: any) => entry.id === id);
                if (entry) Object.assign(entry, event, { id });
                else record.messages.push({ ...event, id });
            } else if (record) { record.outcome = event.kind; record.error = event.kind === 'error' ? event.text : ''; record.updatedAt = Date.now(); }
            localStorage.setItem('fixture.history', JSON.stringify(records));
            window.__chatChannel.onmessage(event);
        };
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    });
    // A saved bench for "locate on bench", and the real drawings and checks the
    // bridge would turn into review cards.
    const revision = (await optical('system_get')).revision;
    await optical('bench_template', { name: 'delay_line', apply: true, revision });
    const savedBench = (await optical('bench_get')).bench;
    const failing = structuredClone(savedBench);
    failing.assertions[0].target += 50;
    const wrap = (result: unknown) => ({ content: [{ type: 'text', text: JSON.stringify(result) }] });
    const labReview = reviewOf(wrap(await optical('bench_lab_check', { bench: failing })), workspace);
    const hardware: Record<string, any>[] = [];
    for (const name of ['michelson', 'periscope_routing']) {
        const bench = (await optical('bench_template', { name })).bench;
        if (name === 'periscope_routing') bench.periscopes = [{ id: 'P1', lower: 'M1', upper: 'M2' }];
        const exported = await optical('bench_export_blueprint', { bench, format: 'layout3d', show: { table: true, holes: true, mounts: true }, stroke: true, background: 'white' });
        const { figure } = reviewOf(wrap(exported), workspace);
        // Hardware drawings exceed the inline limit: kept as a stored file only.
        assert(exported.content.length > 1_400_000, `${name} hardware drawing is ${exported.content.length} bytes`);
        assert.equal(figure!.data, undefined);
        assert.equal(figure!.omitted, undefined);
        assert.equal(figure!.saved, false);
        hardware.push({ ...figure!, filename: `${name}-hardware.svg`, size: Buffer.byteLength(exported.content) });
    }
    await page.goto(`http://127.0.0.1:${(server.address() as import('node:net').AddressInfo).port}`);
    const composer = page.getByRole('textbox', { name: 'Message', exact: true });
    const send = page.getByRole('button', { name: 'Send', exact: true });
    const ensureHistoryOpen = async () => {
        const details = page.locator('details.chat-history');
        if ((await details.getAttribute('open')) === null) {
            await details.locator('> summary').click();
        }
    };
    if (process.argv.includes('--models-only')) {
        const picker = page.getByRole('combobox', { name: 'Model', exact: true });
        const provider = page.getByRole('combobox', { name: 'Agent', exact: true });
        await expect(picker.locator('option[value="codex-fixture-b"]')).toHaveCount(1);
        await picker.selectOption('codex-fixture-b');
        await composer.fill('Selected Codex model'); await send.click();
        await expect(picker).toBeDisabled();
        await page.evaluate(() => window.__emit({kind:'done'}));
        await expect(picker).toBeEnabled();
        assert.equal(await page.evaluate(() => window.__chatRequests.at(-1).model),'codex-fixture-b');
        await composer.fill('Retain model-change draft');
        await picker.selectOption('codex-fixture-a');
        await expect(composer).toHaveValue('Retain model-change draft');
        await expect(page.locator('.chat-history button')).toHaveCount(1);
        await send.click(); await page.evaluate(() => window.__emit({kind:'done'}));
        assert.equal(await page.evaluate(() => window.__chatRequests.at(-1).conversationId),null);
        await ensureHistoryOpen();
        await page.locator('.chat-history button').filter({hasText:'Selected Codex model'}).click();
        await expect(picker).toHaveValue('codex-fixture-b');
        await page.evaluate(() => { window.__holdModels=true; });
        await provider.selectOption('claude');
        await expect.poll(() => page.evaluate(() => !!window.__releaseModels)).toBe(true);
        await provider.selectOption('codex');
        await page.evaluate(() => window.__releaseModels());
        await expect(picker.locator('option[value="codex-fixture-a"]')).toHaveCount(1);
        await expect(picker.locator('option[value="claude-fixture-a"]')).toHaveCount(0);
        await provider.selectOption('claude');
        await expect(picker.locator('option[value="claude-fixture-b"]')).toHaveCount(1);
        await picker.selectOption('claude-fixture-b');
        await composer.fill('Selected Claude model'); await send.click();
        await page.evaluate(() => window.__emit({kind:'done'}));
        assert.equal(await page.evaluate(() => window.__chatRequests.at(-1).model),'claude-fixture-b');
        await picker.selectOption('__custom__');
        await page.getByRole('textbox',{name:'Model ID',exact:true}).fill('private/custom-model');
        await page.getByRole('textbox',{name:'Model ID',exact:true}).blur();
        await composer.fill('Custom model'); await send.click(); await page.evaluate(() => window.__emit({kind:'done'}));
        assert.equal(await page.evaluate(() => window.__chatRequests.at(-1).model),'private/custom-model');
        await page.evaluate(() => { window.__failModels=true; });
        await page.getByRole('button',{name:'Refresh models',exact:true}).click();
        await expect(page.getByRole('alert').filter({hasText:'Fixture catalog error'})).toBeVisible();
        await page.getByRole('button',{name:'Refresh models',exact:true}).click();
        await expect(page.getByRole('alert').filter({hasText:'Fixture catalog error'})).toHaveCount(0);
        await page.reload();
        await ensureHistoryOpen();
        await page.locator('.chat-history button').filter({hasText:'Selected Claude model'}).click();
        await expect(picker).toHaveValue('claude-fixture-b');
        await expect(provider).toHaveValue('claude');
        await page.screenshot({path:resolve(target,'chat-model-picker.png'),fullPage:true});
        await page.setViewportSize({width:480,height:940});
        await expect(page.locator('.assistant-pane')).toHaveClass(/assistant-overlay/);
        await expect(picker).toBeVisible();
        await page.screenshot({path:resolve(target,'chat-model-picker-480.png'),fullPage:true});
        assert.deepEqual(errors,[]);
        console.log('PASS: dynamic model picker, provider isolation with late replies, selected SDK arguments, history restoration, model-change draft retention, custom IDs, refresh failure/retry, active-turn lock and narrow layout');
    } else {
    await composer.fill('Inspect the saved project');
    await expect(send).toBeEnabled();
    await send.click();
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
    await expect(page.getByRole('combobox', { name: 'Agent', exact: true })).toBeDisabled();
    await page.evaluate(() => {
        window.__emit({ kind: 'tool', id: 'tool-1', name: 'system_get', status: 'in_progress', detail: '{}' });
        window.__emit({ kind: 'tool', id: 'tool-1', status: 'completed', detail: 'revision 3' });
        window.__emit({ kind: 'text', id: 'a', text: 'Saved revision' });
        window.__emit({ kind: 'text', id: 'a', text: 'Saved revision 3' });
        window.__emit({ kind: 'done' });
    });
    await expect(page.locator('.chat-entry.text')).toHaveCount(1);
    await expect(page.locator('.chat-entry.tool')).toHaveCount(1);
    await expect(page.locator('.chat-entry.text')).toContainText('Saved revision 3');
    await page.locator('.chat-entry summary').click();
    await expect(page.locator('.chat-entry pre')).toContainText('revision 3');
    await composer.fill('Preserve this draft');
    await page.getByRole('combobox', { name: 'Agent', exact: true }).selectOption('claude');
    await expect(composer).toHaveValue('Preserve this draft');
    await composer.fill('Claude request');
    await send.click();
    await page.getByRole('button', { name: 'Stop', exact: true }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Stopped.' })).toBeVisible();
    await ensureHistoryOpen();
    await page.locator('.chat-history button').filter({hasText:'Inspect the saved project'}).click();
    await expect(composer).toHaveValue('Preserve this draft');
    await expect(page.locator('.chat-entry.text')).toHaveCount(1);
    await page.getByRole('tab', { name: 'Optical layout', exact: true }).click();
    await page.getByRole('button', { name: '3D bench', exact: true }).click();
    await page.getByRole('button', { name: 'Close assistant', exact: true }).click();
    await expect(composer).not.toBeVisible();
    await page.getByRole('button', { name: 'Assistant', exact: true }).click();
    await expect(composer).toHaveValue('Preserve this draft');
    await page.getByRole('button', { name: 'New conversation', exact: true }).click();
    await expect(page.locator('.chat-entry')).toHaveCount(0);
    await expect(page.locator('.chat-history button')).toHaveCount(2);
    await composer.fill('fail to start'); await send.click();
    await expect(page.getByRole('alert').filter({ hasText: 'Fixture launch failure' })).toBeVisible();
    await expect(composer).toHaveValue('fail to start');
    await expect(page.locator('.chat-entry')).toHaveCount(0);
    await composer.fill('stream error'); await send.click();
    await page.evaluate(() => window.__emit({ kind: 'error', text: 'Fixture model unavailable' }));
    await expect(page.getByRole('alert').filter({ hasText: 'Fixture model unavailable' })).toBeVisible();
    await composer.fill('markdown reply'); await send.click();
    await page.evaluate(() => {
        window.__emit({ kind: 'text', id: 'md', text: '# Head' });
        window.__emit({ kind: 'text', id: 'md', text: '# Head\n\n**bold** and `code`\n\n- one\n- two' });
    });
    const mdEntry = page.locator('.chat-entry.text').last();
    await expect(mdEntry.locator('.markstream-vue h1')).toContainText('Head');
    await expect(mdEntry.locator('.markstream-vue strong')).toContainText('bold');
    await expect(mdEntry.locator('.markstream-vue code')).toContainText('code');
    await expect(mdEntry.locator('.markstream-vue li')).toHaveCount(2);
    await page.evaluate(() => window.__emit({ kind: 'done' }));
    // Drawings returned by tools: shown through <img> (SVG scripts and
    // handlers inert), zoomable, full screen, downloadable; oversize ones
    // leave a note.
    await page.getByRole('button', { name: 'New conversation', exact: true }).click();
    await composer.fill('drawing review'); await send.click();
    const drawing = '<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" onload="window.__pwned=1"><script>window.__pwned=1</script><rect width="200" height="100" fill="#3a7"/></svg>';
    await page.evaluate(([svg, checks, hardware]) => {
        window.__emit({ kind: 'tool', id: 'draw', name: 'bench_export_blueprint', status: 'in_progress', detail: '{}' });
        window.__emit({ kind: 'tool', id: 'draw', status: 'completed', detail: '{}', figure: { mime: 'image/svg+xml', filename: 'hissabeth-layout3d.svg', bytes: svg.length, data: btoa(svg) } });
        window.__emit({ kind: 'tool', id: 'big', name: 'artifact_fetch', status: 'completed', detail: '{}', figure: { mime: 'image/png', filename: 'huge.png', bytes: 3 * 1024 * 1024, omitted: 'too_large' } });
        window.__emit({ kind: 'tool', id: 'lab', name: 'bench_lab_check', status: 'completed', detail: '{}', checks });
        (hardware as any[]).forEach((figure, i) => window.__emit({ kind: 'tool', id: 'hw' + i, name: 'bench_export_blueprint', status: 'completed', detail: '{}', figure }));
        window.__emit({ kind: 'done' });
    }, [drawing, labReview.checks, hardware.map(({ size, ...figure }) => figure)] as const);
    // Lab check summary: failed badge, the failing assertion with its residual, and locate.
    const checksCard = page.locator('.chat-checks');
    await expect(checksCard.locator('.chat-checks-badge')).toHaveText('Lab check failed');
    await expect(checksCard).toContainText('1 failed');
    await expect(checksCard).toContainText('Draft on revision');
    await page.locator('.chat-panel').screenshot({ path: resolve(target, 'chat-review.png') });
    const failed = checksCard.locator('li[data-kind="assertion"]').filter({ hasText: 'delay_opl' });
    await expect(failed).toContainText('residual');
    const at = labReview.checks!.items.find(item => item.label === 'delay_opl')!;
    assert(at.element && at.branch, 'the failing assertion names an element and branch');
    await failed.getByRole('button', { name: `Locate ${at.element} on bench` }).click();
    await expect(page.locator('.bench-element-list button[aria-pressed="true"]')).toContainText(at.element);
    // Real hardware drawings: read back from their stored files, viewable and downloadable whole.
    for (const figure of hardware) {
        const big = page.locator(`.chat-figure[data-file="${figure.file}"]`);
        await expect.poll(() => big.locator('img').evaluate((img: HTMLImageElement) => img.naturalWidth), { timeout: 20000 }).toBeGreaterThan(0);
        const download = page.waitForEvent('download');
        await big.getByRole('button', { name: 'Download drawing', exact: true }).click();
        const file = await download;
        assert.equal(file.suggestedFilename(), figure.filename);
        assert.equal((await readFile((await file.path())!)).length, figure.size);
    }
    // A drawing card's own locate opens the bench too.
    await page.locator(`.chat-figure[data-file="${hardware[0]!.file}"]`).getByRole('button', { name: 'Locate on bench', exact: true }).click();
    await expect(page.locator('.bench-element-list')).toBeVisible();
    const card = page.locator('.chat-figure[data-tool="bench_export_blueprint"]:not([data-file])');
    await expect(card.locator('img')).toBeVisible();
    await expect.poll(() => card.locator('img').evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(200);
    await card.locator('.chat-figure-view').hover();
    await page.mouse.wheel(0, -200);
    await expect.poll(() => card.locator('img').getAttribute('style')).toMatch(/scale\((?!1\))/);
    const saved = page.waitForEvent('download');
    await card.getByRole('button', { name: 'Download drawing', exact: true }).first().click();
    assert.equal((await saved).suggestedFilename(), 'hissabeth-layout3d.svg');
    await card.getByRole('button', { name: 'Full screen', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'hissabeth-layout3d.svg' });
    await expect(dialog.locator('img')).toBeVisible();
    await page.screenshot({ path: resolve(target, 'chat-figure-full.png') });
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(page.locator('.chat-figure[data-tool="artifact_fetch"] figcaption')).toContainText('huge.png (3.0 MB) is too large');
    assert.equal(await page.evaluate(() => (window as any).__pwned), undefined);
    await page.reload();
    await expect(page.locator('.chat-history button')).toHaveCount(4);
    await ensureHistoryOpen();
    await page.locator('.chat-history button').filter({hasText:'Inspect the saved project'}).click();
    await expect(page.locator('.chat-entry.text')).toContainText('Saved revision 3');
    await expect(page.getByRole('combobox', {name:'Agent',exact:true})).toHaveValue('codex');
    await ensureHistoryOpen();
    await page.locator('.chat-history button').filter({hasText:'drawing review'}).click();
    await expect(page.locator('.chat-figure[data-tool="bench_export_blueprint"]:not([data-file]) img')).toBeVisible();
    await expect(page.locator('.chat-checks .chat-checks-badge')).toHaveText('Lab check failed');
    for (const figure of hardware)
        await expect.poll(() => page.locator(`.chat-figure[data-file="${figure.file}"] img`).evaluate((img: HTMLImageElement) => img.naturalWidth), { timeout: 20000 }).toBeGreaterThan(0);
    await ensureHistoryOpen();
    await page.locator('.chat-history button').filter({hasText:'Inspect the saved project'}).click();
    const dockBox = (await page.locator('.chat-panel').boundingBox())!;
    const mainBox = (await page.locator('#main').boundingBox())!;
    assert(dockBox.x >= mainBox.x + mainBox.width - 2, 'Assistant must follow the optical workspace');
    assert(Math.abs(dockBox.x + dockBox.width - page.viewportSize()!.width) <= 2, 'Assistant must align with the right window edge');
    for (const width of [1440, 768, 480]) {
        await page.setViewportSize({ width, height: 940 });
        await expect(composer).toBeVisible();
        if(width < 800) { await expect(page.locator('.assistant-pane')).toHaveClass(/assistant-overlay/); await expect.poll(async () => (await page.locator('.chat-panel').boundingBox())!.width).toBeGreaterThanOrEqual(Math.min(440,width)-2); }
        await page.screenshot({ path: resolve(target, `chat-ui-${width}.png`), fullPage: true });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    }
    assert.deepEqual(errors, []);
    await writeFile(resolve(target, 'chat-ui-requests.json'), JSON.stringify(await page.evaluate(() => window.__chatRequests), null, 2));
    console.log('PASS: right conversation dock, history list and reload, provider isolation, streamed items, tool details, drawing cards (inert SVG, zoom, full screen, download, oversize note, stored hardware drawings, lab check summary, locate on bench, reload), close/reopen draft retention, new conversation, cancellation, startup/stream errors, 1440/768/480 layouts');
    }
} finally {
    await browser?.close();
    server.close();
    engine.kill();
}
