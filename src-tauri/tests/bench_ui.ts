// Production Vue and real Rust service. Only the Tauri host/preferences bridge is a fixture.
import { chromium, expect } from '@playwright/test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { createConnection } from 'node:net';
import { readFile, mkdir, unlink, writeFile } from 'node:fs/promises';
import { dirname, resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defaultPreferences } from './ui_preferences';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const target=resolve(root,'src-tauri/target');
const workspace=resolve(target,`bench-ui-${Date.now()}`);
await mkdir(workspace,{recursive:true});
const calls: any[]=[];const errors: string[]=[];let engine!: import('node:child_process').ChildProcess;let browser!: import('@playwright/test').Browser;let id=0;
let holdNextTrace=false;let releaseTrace: ((v?: unknown) => void) | undefined;let held=false;
let holdNextLab = false;
/** Rewrites the next lab check's delay_opl as an undecided lower bound, to see the panel render it. */
let undecideNextLab = false;
let releaseLab: (() => void) | undefined;
async function start(){
    await unlink(resolve(workspace,'endpoint.json')).catch(()=>{});
    engine=spawn(resolve(target,'debug/hissabeth'),['--engine'],{env:{...process.env,HISSABETH_DATA_DIR:workspace},stdio:['ignore','ignore','pipe']});
    let stderr='';engine.stderr!.on('data',d=>{stderr+=d;});
    for(let i=0;i<150;i++) {if(engine.exitCode!==null)throw Error(stderr);try{await readFile(resolve(workspace,'endpoint.json'));return;}catch{}await new Promise(r=>setTimeout(r,50));}
    throw Error('Engine startup timeout: '+stderr);
}
async function optical(name: string,args: any = {}){
    const ep=JSON.parse(await readFile(resolve(workspace,'endpoint.json'),'utf8'));const [host,port]=ep.address.split(':');
    return new Promise<any>((done,fail)=>{const socket=createConnection({host,port:Number(port)});let data='';socket.setTimeout(20000);socket.on('error',fail);socket.on('timeout',()=>{socket.destroy();fail(Error('Optical timeout'));});socket.on('data',chunk=>{data+=chunk;if(!data.includes('\n'))return;socket.end();try{const response=JSON.parse(data.split('\n')[0]);if(response.error)throw Error(response.error.message);if(response.result.isError)throw Error(response.result.content[0].text);done(appResult(name,response.result.content));}catch(e){fail(e);}});socket.on('connect',()=>socket.write(JSON.stringify({token:ep.token,request:{jsonrpc:'2.0',id:++id,method:'tools/call',params:{name,arguments:args}}})+'\n'));});
}
/** Over MCP artifact_fetch carries its bytes in a second content item; the app receives them as base64 `data`. */
function appResult(name: string,content: any[]){
    const result=JSON.parse(content[0].text);
    if(name!=='artifact_fetch')return result;
    const item=content[1];
    result.data=item.type==='image'?item.data:item.resource.blob??Buffer.from(item.resource.text,'utf8').toString('base64');
    return result;
}
const server=createServer(async(req,res)=>{try{const path=new URL(req.url ?? '/','http://localhost').pathname;const file=resolve(root,'dist',path==='/'?'index.html':path.slice(1));if(!file.startsWith(resolve(root,'dist')+'/')){res.writeHead(403).end();return;}res.setHeader('Content-Type',{'.html':'text/html','.js':'text/javascript','.css':'text/css','.png':'image/png','.ico':'image/x-icon'}[extname(file)]??'application/octet-stream');res.end(await readFile(file));}catch{res.writeHead(404).end();}});
try {
    await start();
    const initial=await optical('system_get');
    const {bench}=await optical('bench_example');
    const group={...bench.elements[0],id:'G',label:'Current prescription',kind:'lens_group',position:[100,200,75],rotation:[0,0,0],reflectance:0,transmittance:1,mount:null};
    const detector={...group,id:'D',kind:'detector',position:[160,200,75],aperture:100,body_radius:100};
    bench.elements=[group,detector];bench.sources=[{...bench.sources[0],position:[50,202,75],direction:[1,0,0],rings:0,beam_radius:0}];
    bench.connections=[{from:bench.sources[0].id,port:'out',to:'G'},{from:'G',port:'out',to:'D'}];
    initial.system.bench=bench;
    await optical('system_set',{system:initial.system,revision:initial.revision});
    const original=await optical('bench_trace');assert.equal(original.power.detected,1);
    await new Promise<void>(r=>server.listen(0,'127.0.0.1',()=>r()));
    browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:1600,height:1050}});
    page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error'||m.text().includes('[intlify]'))errors.push(m.text());});
    const preferences=structuredClone(defaultPreferences);preferences.locale='en-US';preferences.workspace.defaultView='Layout';preferences.workspace.restoreLastView=false;
    await page.exposeFunction('__bridge',async(command: string,args: any = {})=>{
        calls.push({command,name:args.name,args:args.args});
        if(command==='get_preferences')return{preferences,revision:0};
        if(command==='plugin:event|listen')return 1;if(command==='plugin:event|unlisten')return;
        if(command==='chat_runtime')return{node:'/node',agents:[{name:'codex',installed:true}]};
        if(command==='chat_models')return[];
        if(command==='chat_history')return{conversations:[],activeId:null,activeConversationId:null};
        if(command==='conversation_edit'){
            const state=await optical('system_get');state.system.surfaces[0].radius=80;
            await optical('system_set',{system:state.system,revision:state.revision});return;
        }
        if(command==='optical_call'){
            const result=await optical(args.name,args.args);
            if(args.name==='bench_trace'&&holdNextTrace){holdNextTrace=false;held=true;await new Promise(r=>{releaseTrace=r;});held=false;}
            if (args.name === 'bench_lab_check' && holdNextLab) {
                holdNextLab = false;
                await new Promise<void>(resolve => { releaseLab = resolve; });
                releaseLab = undefined;
            }
            if (args.name === 'bench_lab_check' && undecideNextLab) {
                undecideNextLab = false;
                const opl = result.assertions.find((a: any) => a.assertion === 'delay_opl');
                Object.assign(opl, { status: 'indeterminate', basis: { kind: 'lower_bound', assumptions: [{ code: 'thin_lens', elements: ['L9'] }] } });
            }
            return result;
        }
        throw Error('Unexpected host command: '+command);
    });
    await page.addInitScript(()=>{
        let next=0;window.isTauri=true;
        window.__TAURI_INTERNALS__={invoke:async(c: string,a: any)=>{
            if(c==='chat_history')return {conversations:window.__history??[],activeId:null,activeConversationId:null};
            if(c==='chat_start'){
                await window.__bridge('conversation_edit');
                window.__history=[{id:a.request.conversationId||a.request.id,provider:a.request.provider,model:a.request.model,title:a.request.prompt,updatedAt:Date.now(),messages:[{id:a.request.id,kind:'user',text:a.request.prompt},{id:a.request.id+':reply',kind:'text',text:'Saved the new lens curvature.'}],outcome:'done',error:''}];
                setTimeout(()=>{a.channel.onmessage({kind:'text',id:'reply',text:'Saved the new lens curvature.'});a.channel.onmessage({kind:'done'});},30);return;
            }
            return window.__bridge(c,a);
        },transformCallback:()=>++next,unregisterCallback:()=>{},metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}}};
        window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
    });
    await page.goto(`http://127.0.0.1:${(server.address() as import('node:net').AddressInfo).port}`);
    if (process.argv.includes('--delay-stage')) {
        await expect(page.getByLabel('Lab template').locator('option[value="modal_hybrid"]')).toHaveText('Four-port × four-mode coherent readout');
        await page.getByLabel('Lab template').selectOption('delay_line');
        await page.getByRole('button', { name: 'Load template', exact: true }).click();
        const stage = page.getByRole('group', { name: 'Delay stage delay' });
        await expect(stage).toBeVisible();
        const initialStage = await optical('bench_get');
        const lab = page.getByRole('region', { name: 'Lab engineering assistant' });
        const stageReading = lab.locator('[data-stage-reading="delay"]');
        const delayArm = lab.locator('[data-branch-reading="arm_delay"]');
        const oplAssertion = lab.locator('[data-assertion="delay_opl"]');
        const outdated = lab.locator('[data-lab-stale]');
        await expect(lab.getByRole('status')).toContainText(`Revision ${initialStage.revision} · Saved bench`);
        await expect(oplAssertion).toContainText('ΔOPL 600 mm');
        await expect(oplAssertion.locator('.badge')).toHaveText('Pass');
        // The 2D view draws the same symbols, branch colours and assertion marks as the blueprint.
        const plan = await optical('bench_drawing', {});
        const delayColor = plan.legend.find((entry: any) => entry.key === 'arm_delay').color;
        await expect(page.locator('.plan-beams line[data-branch="arm_delay"]').first()).toHaveAttribute('stroke', delayColor);
        await expect(page.locator('[data-element="M3"] .plan-symbol')).toHaveAttribute('data-symbol', 'mirror');
        await expect(page.locator('[data-element="BS1"] .plan-symbol')).toHaveAttribute('data-symbol', plan.symbols.find((s: any) => s.id === 'BS1').kind);
        await expect(page.locator('.plan-labels text[data-label="assertion"][data-target="delay_opl"]')).toHaveClass(/pass/);
        await expect(page.locator('.plan-legend [data-legend="branch"][data-key="arm_delay"] .swatch')).toHaveCSS('border-top-color', delayColor.replace(/^#(..)(..)(..)$/, (_: string, r: string, g: string, b: string) => `rgb(${parseInt(r, 16)}, ${parseInt(g, 16)}, ${parseInt(b, 16)})`));
        // Every shared hole is drawn (the blueprint draws the same list), and the result cards match.
        await expect(page.locator('.plan-holes')).toHaveAttribute('data-holes', String(plan.holes.length));
        expect(((await page.locator('.plan-holes').getAttribute('d')) ?? '').split('M').length - 1).toBe(plan.holes.length);
        await expect(page.locator('.plan-cards li')).toHaveCount(plan.cards.length);
        await page.locator('.bench-element-list').getByRole('button', { name: /^M3\b/ }).click();
        const mirrorX = page.locator('.bench-inspector').getByRole('spinbutton', { name: 'X (mm)' });
        const position = stage.getByRole('spinbutton', { name: 'Position (mm)' });
        const slider = stage.getByRole('slider', { name: 'Position (mm)' });

        await slider.hover();
        const box = (await slider.boundingBox())!;
        await page.mouse.down();
        await page.mouse.move(box.x + box.width - 1, box.y + box.height / 2, { steps: 6 });
        await expect(mirrorX).toHaveValue('562.5');
        await page.mouse.up();

        await position.fill('13');
        await position.press('Tab');
        await expect(stage.getByRole('alert')).toHaveText(
            'delay · Position (mm) · 13 is outside [-12.5, 12.5].',
        );
        await expect(mirrorX).toHaveValue('562.5');

        // The stage-solved drawing lands before the trace: M3 must not be shifted twice.
        holdNextTrace = true;
        await position.fill('2.5');
        await position.press('Tab');
        await expect.poll(() => held).toBe(true);
        await expect(page.locator('.plan-cards li', { hasText: 'Position 2.5 mm' })).toHaveCount(1);
        releaseTrace!();
        await expect.poll(() => held).toBe(false);
        await expect(mirrorX).toHaveValue('552.5');
        await expect(page.locator('[data-element="M3"] .plan-symbol')).not.toHaveAttribute('transform', /./);
        await expect(stageReading).toContainText('Position 2.5 mm');
        await expect(oplAssertion).toContainText('ΔOPL 605 mm');

        holdNextLab = true;
        await lab.getByRole('button', { name: 'Refresh', exact: true }).click();
        await expect.poll(() => Boolean(releaseLab)).toBe(true);
        const pzt = stage.getByRole('spinbutton', { name: 'PZT (µm)' });
        await pzt.fill('1');
        await pzt.press('Tab');
        await expect(mirrorX).toHaveValue('552.501');
        await expect(stageReading).toContainText('PZT 1 µm');
        await expect(outdated).toHaveCount(0);
        releaseLab!();
        await expect.poll(() => Boolean(releaseLab)).toBe(false);
        await expect(lab.getByRole('status')).toContainText(`Revision ${initialStage.revision} · Bench draft`);
        await expect(lab.locator('[data-branch-reading="arm_reference"]')).toContainText('OPL 125 mm');
        await expect(delayArm).toContainText('Delay stage delay OPL 730.002 mm');
        await expect(oplAssertion).toContainText('ΔOPL 605.002 mm');
        await expect(oplAssertion).toContainText('tolerance ±0.001 mm');
        await expect(oplAssertion.locator('.badge')).toHaveText('Fail');
        await expect(lab.locator('[data-assertion="combiner_angle"]')).toContainText(/Δr .* mm · Δθ .* mrad/);
        await expect(lab.locator('[data-assertion="combiner_angle"] .badge')).toHaveText('Pass');
        // An assertion its value's basis cannot decide is amber, with the basis spelled out.
        undecideNextLab = true;
        await lab.getByRole('button', { name: 'Refresh', exact: true }).click();
        await expect(oplAssertion.locator('.badge')).toHaveText('Indeterminate');
        await expect(oplAssertion.locator('.badge')).toHaveClass(/badge-warn/);
        await expect(oplAssertion).toHaveClass(/undecided-assertion/);
        await expect(oplAssertion.locator('[data-basis]')).toHaveText('Basis: lower bound · thin lenses L9');
        await lab.getByRole('button', { name: 'Refresh', exact: true }).click();
        await expect(oplAssertion.locator('.badge')).toHaveText('Fail');

        const downloaded = page.waitForEvent('download');
        await page.getByRole('button', { name: 'Export', exact: true }).click();
        const svgFile = await (await downloaded).path();
        const drawing = await readFile(svgFile, 'utf8');
        const svgReadings = await page.evaluate(svg => {
            const document = new DOMParser().parseFromString(svg, 'image/svg+xml');
            const highlight = document.querySelector('[data-branch="arm_delay"]')!;
            const swatch = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
            swatch.style.color = highlight.getAttribute('stroke')!;
            return {
                color: swatch.style.color,
                assertion: document.querySelector('[data-kind="assertion"][data-id="delay_opl"]')!.textContent,
                title: document.querySelector('#title-block')!.textContent,
            };
        }, drawing);
        await expect(delayArm.locator('.arm-swatch')).toHaveCSS('background-color', svgReadings.color);
        assert(svgReadings.assertion?.includes('ΔOPL 605.002 mm'));
        assert(svgReadings.title?.includes(`Revision ${initialStage.revision} · bench draft`));

        // Role and extension properties are edited in the inspector and saved with the bench.
        await page.locator('.bench-element-list').getByRole('button', { name: /^BS1\b/ }).click();
        const inspector = page.locator('.bench-inspector');
        await inspector.getByRole('textbox', { name: 'Role', exact: true }).fill('Reference_Combiner');
        await inspector.getByRole('textbox', { name: 'Role', exact: true }).press('Tab');
        const properties = inspector.getByRole('group', { name: 'Extension properties' });
        await properties.getByRole('button', { name: 'Add property' }).click();
        await properties.getByRole('textbox', { name: 'Property name' }).fill('Part');
        await properties.getByRole('textbox', { name: 'Property name' }).press('Tab');
        await properties.getByRole('textbox', { name: 'Value of part', exact: true }).fill('BSW10');
        await properties.getByRole('textbox', { name: 'Value of part', exact: true }).press('Tab');
        await properties.getByRole('button', { name: 'Add property' }).click();
        await properties.getByRole('textbox', { name: 'Value of note', exact: true }).fill('12.5');
        await properties.getByRole('textbox', { name: 'Value of note', exact: true }).press('Tab');
        await properties.getByRole('button', { name: 'Add property' }).click();
        await properties.getByRole('textbox', { name: 'Value of note2', exact: true }).fill('true');
        await properties.getByRole('textbox', { name: 'Value of note2', exact: true }).press('Tab');
        await properties.getByRole('button', { name: 'Add property' }).click();
        await properties.getByRole('button', { name: 'Remove property note3' }).click();
        await expect(properties.getByRole('textbox', { name: 'Property name' })).toHaveCount(3);
        // Descriptive fields stay editable on a stage member, whose position is locked.
        await page.locator('.bench-element-list').getByRole('button', { name: /^M3\b/ }).click();
        await inspector.getByRole('textbox', { name: 'Role', exact: true }).fill('delay_mirror');
        await inspector.getByRole('textbox', { name: 'Role', exact: true }).press('Tab');
        assert.deepEqual(await optical('bench_get'), initialStage, 'Preview must preserve the saved stage');
        const save = page.getByRole('button', { name: 'Save bench', exact: true });
        holdNextLab = true;
        await save.click();
        await expect(save).toHaveCount(0);
        await expect.poll(() => Boolean(releaseLab)).toBe(true);
        await expect(outdated).toHaveText('Results out of date');
        await expect(stageReading).toContainText('PZT 1 µm');
        releaseLab!();
        const saved = await optical('bench_trace');
        await expect(outdated).toHaveCount(0);
        await expect(lab.getByRole('status')).toContainText(`Revision ${saved.revision} · Saved bench`);
        const opl = saved.branches.find((branch: { id: string }) => branch.id === 'arm_delay').paths[0].opl_mm;
        const savedBs1 = (await optical('bench_get')).bench.elements.find((e: { id: string }) => e.id === 'BS1');
        assert.equal(savedBs1.role, 'reference_combiner');
        assert.deepEqual(savedBs1.metadata, { part: 'BSW10', note: 12.5, note2: true });
        assert.equal((await optical('bench_get')).bench.elements.find((e: { id: string }) => e.id === 'M3').role, 'delay_mirror');
        assert(Math.abs(opl - 730.002) <= 1e-9, `Saved delay arm OPL: ${opl}`);
        // 3D mode shows the engine layout3d render at the interactive yaw and pitch, and follows rotation.
        await page.getByRole('button',{name:'3D bench',exact:true}).click();
        const renderText=()=>page.locator('.render3d img').evaluate(async(img:HTMLImageElement)=>(await fetch(img.src)).text());
        await expect.poll(renderText,{timeout:20000}).toContain('azimuth -25° · elevation 35°');
        const view=page.locator('.bench-viewport');const area=(await view.boundingBox())!;
        await page.mouse.move(area.x+area.width*.5,area.y+area.height*.5);await page.mouse.down({button:'right'});
        await page.mouse.move(area.x+area.width*.5+40,area.y+area.height*.5);await page.mouse.up({button:'right'});
        await expect.poll(renderText,{timeout:20000}).toContain('azimuth -11° · elevation 35°');
        assert((await renderText()).includes('width="420mm"'));
        // Zooming re-renders at the new view; the old render is marked stale meanwhile.
        const before=await renderText();
        await page.mouse.wheel(0,-100);
        await expect.poll(renderText,{timeout:20000}).not.toBe(before);
        await expect(page.locator('.render3d.stale')).toHaveCount(0);
        await expect(page.locator('.render3d figcaption')).toHaveText('Engine render');
        await page.getByRole('button',{name:'2D table',exact:true}).click();
        await expect(page.locator('.render3d')).toHaveCount(0);
        // A restarted engine and reloaded page read the role and typed properties back.
        engine.kill();
        await new Promise((r) => engine.once('exit', r));
        await start();
        await page.reload();
        await page.locator('.bench-element-list').getByRole('button', { name: /^BS1\b/ }).click();
        const reloaded = page.locator('.bench-inspector');
        await expect(reloaded.getByRole('textbox', { name: 'Role', exact: true })).toHaveValue('reference_combiner');
        const reloadedProperties = reloaded.getByRole('group', { name: 'Extension properties' });
        await expect(reloadedProperties.getByRole('textbox', { name: 'Property name' })).toHaveCount(3);
        await expect(reloadedProperties.getByRole('textbox', { name: 'Value of part', exact: true })).toHaveValue('BSW10');
        await expect(reloadedProperties.getByRole('textbox', { name: 'Value of note', exact: true })).toHaveValue('12.5');
        await expect(reloadedProperties.getByRole('textbox', { name: 'Value of note2', exact: true })).toHaveValue('true');
        const reread = (await optical('bench_get')).bench.elements.find((e: { id: string }) => e.id === 'BS1');
        assert.deepEqual(reread.metadata, { part: 'BSW10', note: 12.5, note2: true });
        assert.deepEqual(errors, []);
        console.log('PASS: delay stage preview, revisioned lab readings, and SVG agree after coarse and PZT motion');
    } else if(process.argv.includes('--lifecycle')) {
        await expect(page.locator('.bench-ledger')).toBeVisible();
        await page.locator('[data-element="G"] .bench-center').click();
        await page.getByRole('button',{name:'Inspect lens group section',exact:true}).click();
        await expect(page.locator('.prescription-dock table')).toBeVisible();
        await page.getByRole('button',{name:'3D bench',exact:true}).click();
        await expect(page.locator('.bench-inspector')).toContainText('G');
        await page.locator('.chat-panel').getByRole('button',{name:'Close assistant',exact:true}).click();
        for(const width of [1600,768,480]) {
            await page.setViewportSize({width,height:1050});await page.waitForTimeout(150);
            await page.screenshot({path:resolve(target,`optical-workspace-${width}.png`),fullPage:true});
            assert(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth+1));
            assert((await page.locator('#main').boundingBox())!.width > width*.7);
        }
        engine.kill();await new Promise(r=>engine.once('exit',r));await start();await page.reload();
        await expect(page.locator('[data-element="G"]')).toBeVisible();
        engine.kill();await new Promise(r=>engine.once('exit',r));await writeFile(resolve(workspace,'system.json'),'{ broken project');await start();await page.reload();
        await expect(page.locator('.recovery-panel')).toBeVisible();
        await page.locator('.recovery-panel button').click();await expect(page.getByRole('textbox',{name:'Message',exact:true})).toBeVisible();
        const recovery=await optical('workspace_status');await optical('workspace_recover',{revision:recovery.revision,source:{kind:'default'}});
        await expect(page.locator('.recovery-panel')).toHaveCount(0,{timeout:10000});
        await page.locator('.chat-panel').getByRole('button',{name:'Close assistant',exact:true}).click();
        await page.getByRole('button',{name:'2D table',exact:true}).click();await expect(page.locator('.bench .empty')).toBeVisible();
        assert.deepEqual(errors,[]);
        await writeFile(resolve(target,'optical-workspace-lifecycle-evidence.json'),JSON.stringify({status:'passed',workspace,scenarios:['initial group mount','section dock toggling','persistent selection','assistant close fills workspace','1600/768/480 layouts','restart','conversation-only recovery'],errors},null,2));
        console.log('PASS: optical workspace pane lifecycle and responsive viewing');
    } else {
    await expect(page.locator('.bench-ledger')).toBeVisible();
    await expect(page.locator('[data-element="G"]')).toBeVisible();
    await expect(page.locator('.bench-grid input, .bench-grid textarea, .bench-gizmo')).toHaveCount(0);
    const selector=page.locator('.command-toolbar select');
    const options=await selector.locator('option').allTextContents();
    assert(options.some(t=>t.includes('Optical layout')));assert(!options.some(t=>/JSON|Laser bench|Import/.test(t)));
    await page.locator('[data-element="G"] .bench-center').click();
    await expect(page.locator('.bench-inspector')).toContainText('Real lens group');
    const before=await optical('system_get');
    const svg=page.locator('.bench-viewport');const box=(await svg.boundingBox())!;
    await page.mouse.move(box.x+box.width*.6,box.y+box.height*.7);await page.mouse.down();await page.mouse.move(box.x+box.width*.7,box.y+box.height*.8);await page.mouse.up();
    await svg.focus();await page.keyboard.press('ArrowRight');await page.keyboard.press('Control+s');
    assert.deepEqual(await optical('system_get'),before,'Viewer navigation must not write the project');
    await page.getByRole('button',{name:'Inspect lens group section',exact:true}).click();
    await expect(page.locator('.prescription-dock table')).toBeVisible();
    await expect(page.locator('.prescription-dock input, .prescription-dock select, .prescription-dock textarea')).toHaveCount(0);
    await page.getByRole('button',{name:'3D bench',exact:true}).click();
    await expect(page.locator('.bench-inspector')).toContainText('G');
    await expect(page.locator('.metrics')).toHaveCount(0);
    await page.getByRole('textbox',{name:'Message',exact:true}).fill('Set the first lens radius to 80 mm.');
    await page.getByRole('button',{name:'Send',exact:true}).click();
    await expect(page.locator('.chat-panel')).toContainText('Saved the new lens curvature.');
    await expect.poll(async()=>(await optical('system_get')).system.surfaces[0].radius).toBe(80);
    const changed=await optical('bench_trace');assert.notEqual(changed.terminations[0].position[1],original.terminations[0].position[1]);
    await expect.poll(async()=>page.locator('.document-strip').innerText()).toContain('Revision '+(await optical('system_get')).revision);
    await page.getByRole('button',{name:'Lens group section',exact:true}).click();
    await expect(page.locator('.prescription-dock tbody tr').first()).toContainText('80');
    await page.getByRole('button',{name:'2D table',exact:true}).click();
    // A late result from an older saved revision must not replace current rays.
    holdNextTrace=true;await page.getByRole('button',{name:'Refresh rays',exact:true}).click();await expect.poll(()=>held).toBe(true);
    const external=await optical('system_get');external.system.bench.elements[1].position[2]=300;
    await optical('system_set',{system:external.system,revision:external.revision});
    await expect.poll(async()=>Number(await page.locator('.bench-ledger span').filter({hasText:'Detected'}).locator('b').innerText())).toBe(0);
    releaseTrace!();await expect.poll(()=>held).toBe(false);await page.waitForTimeout(200);
    assert.equal(Number(await page.locator('.bench-ledger span').filter({hasText:'Detected'}).locator('b').innerText()),0);
    const restore=await optical('system_get');restore.system.bench.elements[1].position[2]=75;
    await optical('system_set',{system:restore.system,revision:restore.revision});
    await expect.poll(async()=>Number(await page.locator('.bench-ledger span').filter({hasText:'Detected'}).locator('b').innerText())).toBe(1);
    const downloaded=page.waitForEvent('download');await page.getByRole('button',{name:'Export coordinates CSV',exact:true}).click();
    await(await downloaded).saveAs(resolve(target,'bench-assembly-ui.csv'));
    assert((await readFile(resolve(target,'bench-assembly-ui.csv'),'utf8')).includes('lens_group'));
    const readonlyCalls=new Set(['workspace_status','system_get','materials_list','paraxial','layout','spot_diagram','bench_trace','bench_assembly','material_search','material_index','prescription_report','aperture_report']);
    for(const call of calls.filter(c=>c.command==='optical_call'))assert(readonlyCalls.has(call.name),'Unexpected UI optical write '+call.name);
    for(const view of ['Prescription','Materials']){
        await selector.selectOption(view);await expect(page.locator('#main textarea')).toHaveCount(0);
        assert(!await page.getByRole('button',{name:/Apply changes|Import catalog|Use on surface/}).count());
    }
    await selector.selectOption('Layout');
    await page.locator('.chat-panel').getByRole('button',{name:'Close assistant',exact:true}).click();
    for(const width of [1600,768,480]){
        await page.setViewportSize({width,height:1050});
        await page.screenshot({path:resolve(target,`optical-workspace-${width}.png`),fullPage:true});
        assert(await svg.isVisible());
        assert(await page.evaluate(()=>document.documentElement.scrollWidth <= window.innerWidth+1));
    }
    engine.kill();await new Promise(r=>engine.once('exit',r));await start();await page.reload();
    await expect(page.locator('[data-element="G"]')).toBeVisible();assert.equal((await optical('system_get')).system.surfaces[0].radius,80);
    // Recovery remains available through conversation, with no raw text editor.
    engine.kill();await new Promise(r=>engine.once('exit',r));
    await writeFile(resolve(workspace,'system.json'),'{ broken project');await start();await page.reload();
    await expect(page.locator('.recovery-panel')).toBeVisible();await expect(page.locator('.recovery-panel textarea, .recovery-panel input')).toHaveCount(0);
    await page.locator('.recovery-panel button').click();await expect(page.getByRole('textbox',{name:'Message',exact:true})).toBeVisible();
    const recovery=await optical('workspace_status');await optical('workspace_recover',{revision:recovery.revision,source:{kind:'default'}});
    await expect(page.locator('.recovery-panel')).toHaveCount(0,{timeout:10000});
    assert.deepEqual(errors,[]);
    await writeFile(resolve(target,'bench-ui-evidence.json'),JSON.stringify({status:'passed',workspace,scenarios:['real prescription group and continued rays','table/3D/section selection','conversation fixture saves through real MCP','revision refresh','late reply rejection','read-only controls and keyboard','CSV assembly','1600/768/480 screenshots','engine restart','conversation-only recovery'],limits:['SDK conversation is a fixture; optical broker is real','native pointer behavior not exercised'],errors,calls:calls.map(({command,name})=>({command,name}))},null,2));
    console.log('PASS: unified optical viewer with real Rust group tracing and conversation edit fixture');
    }
} finally {if(releaseTrace)releaseTrace();if(releaseLab)releaseLab();if(browser)await browser.close();await new Promise(r=>server.close(r));if(engine&&engine.exitCode===null)engine.kill();}
