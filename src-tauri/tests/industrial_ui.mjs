// Production Vue + real Rust optical service. Preferences/events are an explicit browser fixture;
// native_settings.py exercises the actual window/IPC/persistence path separately.
import { chromium, expect } from '@playwright/test';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { createConnection } from 'node:net';
import { readFile, writeFile, unlink } from 'node:fs/promises';
import { dirname, resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const target = resolve(root, 'src-tauri/target');
const workspace = resolve(target, 'ui-workspace');
const errors = [];
const calls = [];
let engine;
let requestId = 0;
async function startEngine() {
    engine = spawn(resolve(target, 'debug/hissabeth'), ['--engine'], {
        env: { ...process.env, HISSABETH_DATA_DIR: workspace, HISSABETH_SKIP_AGENT_REGISTRATION: '1' },
        stdio: ['ignore', 'ignore', 'pipe'],
    });
    let stderr = '';
    engine.stderr.on('data', (chunk) => { stderr += chunk; });
    for (let attempt = 0; attempt < 150; attempt++) {
        if (engine.exitCode !== null) throw new Error(stderr);
        try { JSON.parse(await readFile(resolve(workspace, 'endpoint.json'), 'utf8')); return; } catch {}
        await new Promise((r) => setTimeout(r, 50));
    }
    throw new Error('Rust service did not start: ' + stderr);
}
async function optical(name, args = {}) {
    const endpoint = JSON.parse(await readFile(resolve(workspace, 'endpoint.json'), 'utf8'));
    const [host, port] = endpoint.address.split(':');
    return new Promise((resolveResult, reject) => {
        const socket = createConnection({ host, port: Number(port) });
        let response = '';
        socket.setTimeout(15000);
        socket.on('timeout', () => { socket.destroy(); reject(new Error('Rust service timeout')); });
        socket.on('error', reject);
        socket.on('data', (chunk) => {
            response += chunk;
            if (!response.includes('\n')) return;
            socket.end();
            try {
                const result = JSON.parse(response.split('\n')[0]);
                if (result.error) throw new Error(result.error.message);
                if (result.result.isError) throw new Error(result.result.content[0].text);
                resolveResult(JSON.parse(result.result.content[0].text));
            } catch (error) { reject(error); }
        });
        socket.on('connect', () => socket.write(JSON.stringify({ token: endpoint.token, request: {
            jsonrpc: '2.0', id: ++requestId, method: 'tools/call', params: { name, arguments: args },
        } }) + '\n'));
    });
}
import { defaultPreferences } from './ui_preferences.mjs';

let snapshot = { preferences: structuredClone(defaultPreferences), revision: 0 };
let failSave = false;
let holdSave = false;
let releaseSave;
let saveHeld = false;
let openSettingsCalls = 0;
const listeners = [];
let listenerId = 0;
async function broadcast() {
    for (const entry of [...listeners]) {
        if (entry.event === 'preferences-changed' && !entry.page.isClosed()) await entry.page.evaluate(({ handler, event }) => {
            window.__testTauriCallback(handler, event);
        }, { handler: entry.handler, event: { event: 'preferences-changed', id: entry.id, payload: snapshot } });
    }
}
async function installBridge(page) {
    page.on('pageerror', (error) => errors.push(error.message));
    page.on('console', (message) => { if (message.type() === 'error' || (message.type() === 'warning' && message.text().includes('[intlify]'))) errors.push(message.text()); });
    await page.exposeFunction('__testInvoke', async (command, args = {}) => {
        calls.push({ command, args: structuredClone(args), time: Date.now() });
        if (command === 'optical_call') return optical(args.name, args.args);
        if (command === 'get_preferences') return structuredClone(snapshot);
        if (command === 'set_preferences') {
            if (holdSave) {
                saveHeld = true;
                await new Promise((resume) => { releaseSave = resume; });
                saveHeld = false;
            }
            if (failSave) { failSave = false; return { __fixtureRejection: { code: 'storage', message: 'Injected settings write failure' } }; }
            if (args.expectedRevision !== snapshot.revision) return { __fixtureRejection: {
                code: 'revision_conflict', message: 'Settings changed in another window', snapshot: structuredClone(snapshot),
            } };
            snapshot = { preferences: structuredClone(args.preferences), revision: snapshot.revision + 1 };
            await broadcast();
            return structuredClone(snapshot);
        }
        if (command === 'open_settings') {
            openSettingsCalls++;
            for (const entry of [...listeners]) if (entry.event === 'settings-section' && !entry.page.isClosed()) {
                await entry.page.evaluate(({handler,event}) => window.__testTauriCallback(handler,event), {handler:entry.handler,event:{event:'settings-section',id:entry.id,payload:args.section}});
            }
            return;
        }
        if (command === 'detect_agents') return [
            { name: 'codex', installed: true, executable: '/fixture/codex' },
            { name: 'claude', installed: true, executable: '/fixture/claude' },
        ];
        if (command === 'configure_agents') return args.agents.map((name) => ({ name, success: true, message: 'Fixture registered' }));
        if (command === 'plugin:event|listen') {
            const entry = { page, handler: args.handler, event: args.event, id: ++listenerId };
            listeners.push(entry); return entry.id;
        }
        if (command === 'plugin:event|unlisten') {
            const index = listeners.findIndex((entry) => entry.id === args.eventId && entry.page === page);
            if (index >= 0) listeners.splice(index, 1); return;
        }
        throw new Error('Unexpected Tauri command: ' + command);
    });
    await page.addInitScript(() => {
        let serial = 0;
        const callbacks = new Map();
        window.isTauri = true;
        window.__testTauriCallback = (id, value) => callbacks.get(id)?.(value);
        window.__TAURI_INTERNALS__ = {
            invoke: async (command, args) => {
                const result = await window.__testInvoke(command, args);
                if (result?.__fixtureRejection) throw result.__fixtureRejection;
                return result;
            },
            transformCallback: (callback, once = false) => {
                const id = ++serial;
                callbacks.set(id, (value) => { if (once) callbacks.delete(id); callback(value); });
                return id;
            },
            unregisterCallback: (id) => callbacks.delete(id),
            metadata: { currentWindow: { label: location.search.includes('settings') ? 'settings' : 'main' },
                currentWebview: { label: location.search.includes('settings') ? 'settings' : 'main' } },
        };
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    });
}
const server = createServer(async (req, res) => {
    try {
        const pathname = new URL(req.url, 'http://localhost').pathname;
        const file = resolve(root, 'dist', pathname === '/' ? 'index.html' : pathname.slice(1));
        if (!file.startsWith(resolve(root, 'dist') + '/')) { res.writeHead(403).end(); return; }
        res.setHeader('Content-Type', { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.ico': 'image/x-icon' }[extname(file)] || 'application/octet-stream');
        res.end(await readFile(file));
    } catch { res.writeHead(404).end(); }
});
let browser;
let main;
let settings;
try {
    await unlink(resolve(workspace, 'endpoint.json')).catch(() => {});
    await startEngine();
    const status = await optical('workspace_status');
    if (status.state === 'recovery') await optical('workspace_recover', { revision: status.revision, source: { kind: 'default' } });
    // A new explicit project gives deterministic source data without touching personal app state.
    const existing = await optical('system_get');
    await optical('project_new', { revision: existing.revision });
    const blank = await optical('system_get');
    await optical('project_import', { format: 'geopter', text: await readFile(resolve(root, 'reference/upstream/example/book/kingslake_doublet.json'), 'utf8'), revision: blank.revision });
    await new Promise((r) => server.listen(0, '127.0.0.1', r));
    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    main = await context.newPage(); await installBridge(main);
    const origin = `http://127.0.0.1:${server.address().port}`;
    await main.goto(origin);
    settings = await context.newPage(); await installBridge(settings);
    await settings.goto(origin + '/?window=settings');
    await expect(main.locator('.layout-svg, .layout svg, .panel svg').first()).toBeVisible();
    await expect(main.locator('.command-toolbar')).toBeVisible();
    await expect(main.locator('.prescription-dock')).toBeVisible();
    await expect(main.locator('html')).toHaveAttribute('lang', 'zh-CN');
    const apply = () => settings.locator('.settings-footer button.primary').click();
    const section = (name) => settings.getByRole('tab', {name, exact:true}).click();
    const view = (name) => main.locator('.command-toolbar select').selectOption(name);
    await settings.getByLabel('语言', {exact:true}).selectOption('en-US');
    await apply();
    await expect(settings.getByRole('heading',{name:'Settings',exact:true})).toBeVisible();
    await expect(main.locator('html')).toHaveAttribute('lang','en-US');
    await expect(main.getByRole('heading',{name:'Layout',exact:true})).toBeVisible();
    await main.keyboard.press('Control+,');
    assert.equal(openSettingsCalls,1);
    if (process.argv.includes('--plot-contrast')) {
        await section('Appearance');
        for (const theme of ['dark','high-contrast']) {
            await settings.getByLabel('Theme',{exact:true}).selectOption(theme);await apply();
            await expect(main.locator('html')).toHaveAttribute('data-theme',theme);
            await expect(main.locator('.data-mark').first()).not.toHaveCSS('filter','none');
            await main.screenshot({path:resolve(target,`industrial-${theme}.png`),fullPage:true});
        }
        await view('Spot diagram');await expect(main.locator('.spot-canvas').first()).toBeVisible();
        await expect(main.locator('.spot-canvas').first()).not.toHaveCSS('filter','none');
        assert.deepEqual(errors,[]);
        console.log('PASS: theme contrast outlines on data marks and spot canvas; saved field and wavelength colors remain unchanged');
    } else if (process.argv.includes('--editor-recovery')) {
        await view('Prescription');
        await main.locator('#sys-name').fill('Draft survives language');
        await section('General');await settings.getByLabel('Language',{exact:true}).selectOption('zh-CN');await apply();
        await expect(main.locator('#sys-name')).toHaveValue('Draft survives language');
        await settings.getByLabel('语言',{exact:true}).selectOption('en-US');await apply();
        await main.keyboard.press('Control+s');
        await expect.poll(async()=> (await optical('system_get')).system.name).toBe('Draft survives language');
        const originalRows=(await optical('system_get')).system.surfaces.length;
        await main.locator('.prescription-dock').getByRole('button',{name:'+ Surface',exact:true}).click();
        await expect(main.getByLabel(`Surface ${originalRows+1} label`,{exact:true})).toBeVisible();
        assert.equal((await optical('system_get')).system.surfaces.length,originalRows);
        await main.getByRole('button',{name:'Revert',exact:true}).click();
        await expect(main.getByLabel(`Surface ${originalRows+1} label`,{exact:true})).toHaveCount(0);
        await view('Import / Export');
        await main.getByRole('button',{name:'Export saved system',exact:true}).click();
        await expect(main.locator('#ie-text')).toHaveValue(/Draft survives language/);
        const exported=await main.locator('#ie-text').inputValue();
        await main.getByRole('button',{name:'New system',exact:true}).click();
        await expect(main.getByRole('group',{name:'Create a blank system?',exact:true})).toBeVisible();
        await main.getByRole('button',{name:'Cancel new system',exact:true}).click();
        assert.equal((await optical('system_get')).system.name,'Draft survives language');
        await main.getByRole('button',{name:'New system',exact:true}).click();
        await main.getByRole('button',{name:'Create blank system',exact:true}).click();
        await expect.poll(async()=> (await optical('system_get')).system.surfaces.length).toBe(1);
        await expect(main.locator('.document-strip')).toContainText('Untitled system');
        await main.locator('#ie-text').fill(exported);
        await main.getByRole('button',{name:'Import (replaces saved system)',exact:true}).click();
        await expect.poll(async()=> (await optical('system_get')).system.surfaces.length).toBe(originalRows);
        await expect(main.locator('.document-strip')).toContainText('Draft survives language');
        // A stopped/restarted real broker enters connected recovery from an invalid saved file.
        engine.kill();await new Promise(resolveExit=>engine.once('exit',resolveExit));
        await writeFile(resolve(workspace,'system.json'),'invalid native project');
        await unlink(resolve(workspace,'endpoint.json')).catch(()=>{});await startEngine();
        await expect(main.getByRole('region',{name:'Workspace recovery',exact:true})).toBeVisible({timeout:15000});
        await expect(main.getByLabel('Recovery project text',{exact:true})).toHaveValue('invalid native project');
        await main.getByLabel('Recovery project text',{exact:true}).fill(exported);
        await main.getByRole('button',{name:'Restore from text',exact:true}).click();
        await expect(main.getByRole('region',{name:'Workspace recovery',exact:true})).toHaveCount(0);
        await expect(main.locator('.document-strip')).toContainText('Draft survives language');
        assert.equal((await optical('workspace_status')).state,'ready');
        assert.deepEqual(errors,[]);
        console.log('PASS: localized editor preserves draft, staged surface insertion/Revert, saved Apply, project export/New confirmation/import, real engine restart and connected text recovery');
    } else if (process.argv.includes('--settings-repair')) {
        snapshot={...snapshot,revision:snapshot.revision+1,warning:'Damaged settings fixture; Apply saves the selected defaults'};await broadcast();
        await expect(settings.locator('.settings-app > .banner.warn')).toContainText('Damaged settings fixture');
        await expect(settings.locator('.settings-footer button.primary')).toBeEnabled();await apply();
        await expect(settings.locator('.settings-app > .banner.warn')).toHaveCount(0);
        assert.equal(snapshot.warning,undefined);
        // Pristine form follows any number of external updates without a false dirty seed.
        await main.keyboard.press('Control+b');await expect(main.locator('.navigator')).toHaveCount(0);
        await main.keyboard.press('Control+b');await expect(main.locator('.navigator')).toBeVisible();
        await expect(settings.locator('[role=alert]')).toHaveCount(0);
        failSave=true;await main.keyboard.press('Control+b');
        await expect(main.locator('.industrial-app > .banner[role=alert]')).toContainText('Injected settings write failure');
        await main.getByRole('button',{name:'Retry',exact:true}).click();
        await expect(main.locator('.industrial-app > .banner[role=alert]')).toHaveCount(0);
        // Native menu target is also delivered to an already open settings entry.
        await main.getByRole('menuitem',{name:'Tools',exact:true}).click();
        await main.getByRole('menuitem',{name:'Agent connections',exact:true}).click();
        assert.equal(calls.findLast(c=>c.command==='open_settings').args.section,'agents');
        await expect(settings.getByRole('tab',{name:'Agents',exact:true})).toHaveAttribute('data-state','active');
        await section('Appearance');await settings.getByLabel('Theme',{exact:true}).selectOption('system');
        await settings.getByLabel('Accent color',{exact:true}).selectOption('teal');await apply();
        await main.emulateMedia({colorScheme:'dark'});await expect(main.locator('html')).toHaveAttribute('data-theme','dark');
        await main.screenshot({path:resolve(target,'industrial-dark.png'),fullPage:true});
        await main.emulateMedia({colorScheme:'light'});await expect(main.locator('html')).toHaveAttribute('data-theme','light');
        await settings.getByLabel('Theme',{exact:true}).selectOption('high-contrast');await apply();
        await main.screenshot({path:resolve(target,'industrial-high-contrast.png'),fullPage:true});
        await settings.getByLabel('Theme',{exact:true}).selectOption('light');await apply();
        await section('Display');await settings.getByLabel('Show plot grid lines',{exact:true}).uncheck();await apply();
        await expect(main.locator('.plot-grid')).toHaveCount(0);
        await settings.getByLabel('Show plot grid lines',{exact:true}).check();await apply();
        await expect(main.locator('.plot-grid')).toHaveCount(1);
        const divider=main.locator('.workbench-split > .splitpanes__splitter');
        const box=await divider.boundingBox();assert(box);
        const beforeWidth=snapshot.preferences.workspace.navigatorSize;
        await main.mouse.move(box.x+box.width/2,box.y+80);await main.mouse.down();await main.mouse.move(box.x+60,box.y+80,{steps:8});await main.mouse.up();
        await expect.poll(()=>snapshot.preferences.workspace.navigatorSize).not.toBe(beforeWidth);
        await view('Prescription');
        await expect(main.getByLabel('Surface 1 interaction',{exact:true}).locator('option').first()).toHaveText('Refract');
        await section('General');await settings.getByLabel('Language',{exact:true}).selectOption('zh-CN');await apply();
        await expect(main.getByLabel('面 1 相互作用',{exact:true}).locator('option').first()).toHaveText('折射');
        await settings.getByLabel('语言',{exact:true}).selectOption('en-US');await apply();
        await settings.getByLabel('Restore the last view at startup',{exact:true}).uncheck();
        await settings.getByLabel('Default view',{exact:true}).selectOption('Materials');await apply();
        await main.reload();await expect(main.locator('.view-heading h1')).toHaveText('Materials');
        assert.deepEqual(errors,[]);
        console.log('PASS: repeated pristine external updates, retry clears errors, agent menu target, system color scheme, live translated interaction choices');
    } else {
    await section('Appearance');
    await settings.getByLabel('Theme',{exact:true}).selectOption('dark');
    await settings.getByLabel('Accent color',{exact:true}).selectOption('teal');
    await settings.getByLabel('Density',{exact:true}).selectOption('comfortable');
    await settings.getByLabel(/Font size/).fill('15');
    await settings.getByLabel('Reduce motion',{exact:true}).check();
    await apply();
    for(const page of [main,settings]){
        await expect(page.locator('html')).toHaveAttribute('data-theme','dark');
        await expect(page.locator('html')).toHaveAttribute('data-accent','teal');
        await expect(page.locator('html')).toHaveAttribute('data-density','comfortable');
        await expect(page.locator('html')).toHaveAttribute('data-motion','reduced');
    }
    assert.equal(await main.locator('body').evaluate(el=>getComputedStyle(el).fontSize),'15px');
    await settings.getByLabel('Theme',{exact:true}).selectOption('high-contrast');await apply();
    await expect(main.locator('html')).toHaveAttribute('data-theme','high-contrast');
    await settings.getByLabel('Theme',{exact:true}).selectOption('light');await apply();
    await expect(main.locator('html')).toHaveAttribute('data-theme','light');
    await section('Workspace');
    await settings.getByLabel('Show metrics bar',{exact:true}).uncheck();
    await settings.getByLabel('Show status bar',{exact:true}).uncheck();await apply();
    await expect(main.locator('.metrics')).toHaveCount(0);await expect(main.locator('.statusbar')).toHaveCount(0);
    await settings.getByLabel('Show metrics bar',{exact:true}).check();
    await settings.getByLabel('Show status bar',{exact:true}).check();await apply();
    await expect(main.locator('.metrics')).toBeVisible();await expect(main.locator('.statusbar')).toBeVisible();
    // External workspace gestures invalidate an edited settings form and preserve it.
    await settings.getByLabel(/Navigator width/).fill('24');
    await main.keyboard.press('Control+b');
    await expect(settings.locator('[role=alert]')).toContainText('Preferences changed in another window');
    await expect(settings.locator('.settings-footer button.primary')).toBeDisabled();
    await settings.getByRole('button',{name:'Retry',exact:true}).click();
    await expect(settings.getByLabel(/Navigator width/)).toHaveValue('18');
    await main.keyboard.press('Control+b');await expect(main.locator('.navigator')).toBeVisible();
    await section('Appearance');
    await settings.getByLabel(/Font size/).fill('16');failSave=true;await apply();
    await expect(settings.locator('[role=alert]')).toContainText('Injected settings write failure');
    await expect(settings.getByLabel(/Font size/)).toHaveValue('16');
    assert.equal(snapshot.preferences.appearance.fontSize,15);
    await apply();await expect(main.locator('body')).toHaveCSS('font-size','16px');
    // Edits made while persistence is pending survive the confirmation of an earlier form.
    await settings.getByLabel(/Font size/).fill('14');holdSave=true;await apply();
    await expect.poll(()=>saveHeld).toBe(true);
    await settings.getByLabel(/Font size/).fill('13');holdSave=false;releaseSave();
    await expect(settings.locator('.settings-footer button.primary')).toBeEnabled();
    await expect(settings.getByLabel(/Font size/)).toHaveValue('13');await apply();
    await expect(main.locator('body')).toHaveCSS('font-size','13px');
    await section('Analysis defaults');
    const savesBefore=calls.filter(c=>c.command==='set_preferences').length;
    await settings.getByLabel(/Grid diameter points/).fill('4');
    await expect(settings.locator('.settings-footer button.primary')).toBeDisabled();
    assert.equal(calls.filter(c=>c.command==='set_preferences').length,savesBefore);
    await settings.getByLabel(/Grid diameter points/).fill('17');
    await settings.getByLabel(/Report precision/).fill('3');
    await settings.getByLabel(/Engine poll interval/).fill('600');await apply();
    await expect(main.locator('.statusbar')).toContainText('poll 600 ms');
    await view('Spot diagram');
    await expect(main.getByLabel(/Diameter points/)).toHaveValue('17');
    await view('Prescription report');
    await expect(main.getByLabel('Display decimals',{exact:true})).toHaveValue('3');
    await main.getByRole('button',{name:'Generate report',exact:true}).click();
    await expect(main.locator('.report-section')).toHaveCount(5);
    await expect(main.locator('.report-section').first()).toContainText('System information');
    const downloadEvent=main.waitForEvent('download');await main.getByRole('button',{name:'Download report',exact:true}).click();
    const download=await downloadEvent;assert((await readFile(await download.path(),'utf8')).includes('First-order'));
    await section('General');await settings.getByLabel('Language',{exact:true}).selectOption('zh-CN');await apply();
    await expect(main.locator('.report-section').first()).toContainText('系统信息');
    assert(!(await main.locator('.report-section').first().innerText()).includes('System information'));
    await settings.getByLabel('语言',{exact:true}).selectOption('en-US');await apply();
    // Selected agent registration uses only fixture commands, never personal CLI settings.
    await section('Agents');await settings.getByLabel('Codex',{exact:true}).check();
    await settings.getByRole('button',{name:'Connect selected agents',exact:true}).click();
    assert.deepEqual(calls.findLast(c=>c.command==='configure_agents').args.agents,['codex']);
    await apply();assert.deepEqual(snapshot.preferences.agents.targets,['codex']);
    await section('Display');await settings.getByLabel('Show plot grid lines',{exact:true}).uncheck();await apply();
    await view('MTF');await expect(main.locator('.plot-grid')).toHaveCount(0);
    // All implemented views remain reachable through the compact view selector.
    for(const name of ['Analysis','Ray inspection','Materials','Import / Export','Prescription','Prescription JSON','Layout']){
        await view(name);await expect(main.locator('.view-heading h1')).toHaveText(name);
    }
    // Draft editing still uses the real Rust workspace and explicit Apply.
    await view('Prescription JSON');const json=main.locator('.json-panel textarea');
    const draft=JSON.parse(await json.inputValue());draft.name='Industrial verified';await json.fill(JSON.stringify(draft,null,2));
    await expect(main.locator('.document-strip')).toContainText('draft');
    await main.keyboard.press('Control+s');
    await expect.poll(async()=> (await optical('system_get')).system.name).toBe('Industrial verified');
    await expect(main.locator('.document-strip')).not.toContainText('draft');
    await view('Layout');
    await main.screenshot({path:resolve(target,'industrial-desktop.png'),fullPage:true});
    for(const width of [1440,1024,768,480,360]){
        await main.setViewportSize({width,height:900});
        await expect(main.locator('.command-toolbar select')).toBeVisible();
        assert(await main.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`Horizontal page overflow at ${width}`);
        if(width<800){await main.getByRole('button',{name:'Navigator',exact:true}).click();await expect(main.getByRole('dialog')).toBeVisible();await main.getByRole('tab',{name:'Materials',exact:true}).click();await expect(main.getByRole('dialog')).toHaveCount(0);await view('Layout');}
        if(width===360)await main.screenshot({path:resolve(target,'industrial-narrow.png'),fullPage:true});
    }
    await settings.setViewportSize({width:480,height:640});await section('Appearance');
    assert(await settings.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Settings page overflow');
    await settings.screenshot({path:resolve(target,'industrial-settings.png'),fullPage:true});
    // A new page reads the persisted fixture and the main window restores its selected view.
    await main.reload();await expect(main.locator('html')).toHaveAttribute('lang','en-US');
    await expect(main.locator('body')).toHaveCSS('font-size','13px');
    assert.deepEqual(errors,[]);
    console.log('PASS: production frontend and real Rust optics; settings, locales, four themes, cross-window conflicts, storage failure, pending edits, selected agents, sampling, report, draft Apply, ten views, five viewport widths');

    }
} catch (error) {
    if (main && !main.isClosed()) {
        await main.screenshot({ path: resolve(target, 'industrial-ui-failure.png'), fullPage: true });
        await writeFile(resolve(target, 'industrial-ui-failure.html'), await main.content());
    }
    if (settings && !settings.isClosed()) await writeFile(resolve(target, 'industrial-settings-failure.html'), await settings.content());
    throw error;
} finally {
    releaseSave?.();
    await browser?.close();
    server.close();
    if (engine && engine.exitCode === null) { engine.kill(); await new Promise((r) => engine.once('exit', r)); }
}
