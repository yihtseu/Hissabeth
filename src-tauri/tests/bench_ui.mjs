// Production Vue and real Rust service. Only the Tauri host/preferences bridge is a fixture.
import { chromium, expect } from '@playwright/test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { createConnection } from 'node:net';
import { readFile, mkdir, unlink, writeFile } from 'node:fs/promises';
import { dirname, resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defaultPreferences } from './ui_preferences.mjs';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const target=resolve(root,'src-tauri/target');
const resume=process.argv.includes('--resume');
const contract=process.argv.includes('--contract');
const workspace=resume?resolve(process.argv[process.argv.indexOf('--resume')+1]):resolve(target,`bench-ui-${Date.now()}`);
await mkdir(workspace,{recursive:true});
const calls=[];const errors=[];const timings=[];let engine;let browser;let id=0;
let holdNextTrace=false;let releaseTrace;let held=false;
async function start(){
    await unlink(resolve(workspace,'endpoint.json')).catch(()=>{});
    engine=spawn(resolve(target,'debug/hissabeth'),['--engine'],{env:{...process.env,HISSABETH_DATA_DIR:workspace},stdio:['ignore','ignore','pipe']});
    let stderr='';engine.stderr.on('data',d=>{stderr+=d;});
    for(let i=0;i<150;i++) {if(engine.exitCode!==null)throw Error(stderr);try{await readFile(resolve(workspace,'endpoint.json'));return;}catch{}await new Promise(r=>setTimeout(r,50));}
    throw Error('Engine startup timeout: '+stderr);
}
async function optical(name,args={}){
    const ep=JSON.parse(await readFile(resolve(workspace,'endpoint.json'),'utf8'));const [host,port]=ep.address.split(':');
    return new Promise((done,fail)=>{const socket=createConnection({host,port:Number(port)});let data='';socket.setTimeout(20000);socket.on('error',fail);socket.on('timeout',()=>{socket.destroy();fail(Error('Optical timeout'));});socket.on('data',chunk=>{data+=chunk;if(!data.includes('\n'))return;socket.end();try{const response=JSON.parse(data.split('\n')[0]);if(response.error)throw Error(response.error.message);if(response.result.isError)throw Error(response.result.content[0].text);done(JSON.parse(response.result.content[0].text));}catch(e){fail(e);}});socket.on('connect',()=>socket.write(JSON.stringify({token:ep.token,request:{jsonrpc:'2.0',id:++id,method:'tools/call',params:{name,arguments:args}}})+'\n'));});
}
const server=createServer(async(req,res)=>{try{const path=new URL(req.url,'http://localhost').pathname;const file=resolve(root,'dist',path==='/'?'index.html':path.slice(1));if(!file.startsWith(resolve(root,'dist')+'/')){res.writeHead(403).end();return;}res.setHeader('Content-Type',{'.html':'text/html','.js':'text/javascript','.css':'text/css','.png':'image/png','.ico':'image/x-icon'}[extname(file)]??'application/octet-stream');res.end(await readFile(file));}catch{res.writeHead(404).end();}});
try {
    await start();
    if(contract){
        const {bench}=await optical('bench_example');const state=await optical('bench_get');
        for(let i=0;i<35;i++)bench.elements.push({id:'E'+(i+1),label:'E'+(i+1),kind:'aperture',position:[80+(i%12)*60,460+Math.floor(i/12)*45,75],rotation:[0,0,0],aperture:12.5,body_radius:15,focal_length:100,reflectance:0,transmittance:1,mount:null});
        await optical('bench_set',{bench,revision:state.revision});
    }
    await new Promise(r=>server.listen(0,'127.0.0.1',r));
    browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:1600,height:1050}});
    page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error'||m.text().includes('[intlify]'))errors.push(m.text());});
    const preferences=structuredClone(defaultPreferences);preferences.workspace.defaultView='Laser bench';preferences.workspace.restoreLastView=false;
    await page.exposeFunction('__bridge',async(command,args={})=>{
        calls.push({command,name:args.name,args:args.args});
        if(command==='get_preferences')return{preferences,revision:0};
        if(command==='plugin:event|listen')return 1;if(command==='plugin:event|unlisten')return;
        if(command==='optical_call'){
            const start=performance.now();const result=await optical(args.name,args.args);if(args.name==='bench_trace')timings.push(performance.now()-start);
            if(args.name==='bench_trace'&&holdNextTrace){holdNextTrace=false;held=true;await new Promise(r=>{releaseTrace=r;});held=false;}
            return result;
        }
        throw Error('Unexpected host command: '+command);
    });
    await page.addInitScript(()=>{let next=0;window.isTauri=true;window.__TAURI_INTERNALS__={invoke:(c,a)=>window.__bridge(c,a),transformCallback:()=>++next,unregisterCallback:()=>{},metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}}};window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};});
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    if(!resume&&!contract)await page.getByRole('button',{name:'载入分光示例',exact:true}).click();
    await expect(page.locator('.bench-ledger')).toBeVisible();
    if(!resume&&!contract)assert.equal((await optical('bench_get')).bench,null,'Example and preview must not save');
    const rawDetails=page.locator('details').filter({has:page.locator('summary',{hasText:'实验台 JSON 草稿'})});
    async function draft(){if(!await rawDetails.locator('textarea').isVisible())await rawDetails.locator('summary').click();await page.getByRole('button',{name:'读取当前草稿',exact:true}).click();return JSON.parse(await rawDetails.locator('textarea').inputValue());}
    const original=await draft();
    if(!resume&&!contract){
    async function drag(locator,dx,dy,button='left',cancel=false){const box=await locator.boundingBox();assert(box);await page.mouse.move(box.x+box.width/2,box.y+box.height/2);await page.mouse.down({button});await page.mouse.move(box.x+box.width/2+dx,box.y+box.height/2+dy,{steps:6});if(cancel)await page.keyboard.press('Escape');await page.mouse.up({button});}
    const bs=()=>page.locator('[data-element="BS1"] circle');
    await drag(bs(),25,-15);const moved=await draft();assert.notEqual(moved.elements[0].position[0],original.elements[0].position[0]);assert.equal(moved.elements[0].position[2],75);assert.deepEqual(moved.elements[0].rotation,original.elements[0].rotation);
    await page.getByRole('button',{name:'撤销',exact:true}).click();assert.deepEqual(await draft(),original);
    await page.getByRole('button',{name:'三维实验台',exact:true}).click();await bs().click();
    await drag(page.locator('[data-axis="Z"] circle'),0,-20);const spatial=await draft();assert(spatial.elements[0].position[2]>75);assert.deepEqual(spatial.elements[0].position.slice(0,2),[300,200]);
    await drag(page.locator('[data-axis="Z"] circle'),0,-25,'left',true);assert.deepEqual(await draft(),spatial,'Escape restores the full drag');
    const viewport=page.locator('.bench-viewport');await drag(viewport,40,20,'right');assert.deepEqual(await draft(),spatial,'Orbit must not edit the model');
    await page.getByRole('button',{name:'撤销',exact:true}).click();assert.deepEqual(await draft(),original,'Canceled drag adds no undo entry');
    // Hold an old successful trace; it must not overwrite the newer failed layout.
    await page.waitForTimeout(240);await expect(page.getByRole('button',{name:'追迹草稿',exact:true})).toBeEnabled();
    holdNextTrace=true;await page.getByRole('button',{name:'追迹草稿',exact:true}).click();await expect.poll(()=>held).toBe(true);
    await page.getByRole('spinbutton',{name:'Z 坐标',exact:true}).fill('175');await page.getByRole('spinbutton',{name:'Z 坐标',exact:true}).blur();
    await expect.poll(async()=>Number(await page.locator('.bench-ledger span').filter({hasText:'探测'}).locator('b').innerText())).toBe(0);
    releaseTrace();await expect.poll(()=>held).toBe(false);await page.waitForTimeout(200);
    assert.equal(Number(await page.locator('.bench-ledger span').filter({hasText:'探测'}).locator('b').innerText()),0);
    await page.getByRole('spinbutton',{name:'Z 坐标',exact:true}).fill('75');await page.getByRole('spinbutton',{name:'Z 坐标',exact:true}).blur();
    await expect.poll(async()=>Number(await page.locator('.bench-ledger span').filter({hasText:'探测'}).locator('b').innerText())).toBe(1);
    await page.getByRole('combobox',{name:'元件类型',exact:true}).selectOption('aperture');await page.getByRole('spinbutton',{name:'添加元件数量',exact:true}).fill('35');await page.getByRole('button',{name:'添加元件',exact:true}).click();
    assert.equal((await draft()).elements.length,40);await expect(page.locator('.bench-count')).toContainText('40 / 128');
    await page.locator('.bench-tools').getByRole('button',{name:'应用更改',exact:true}).click();await expect.poll(async()=>(await optical('bench_get')).bench?.elements.length).toBe(40);
    const committed=await optical('bench_get');assert.equal(committed.bench.elements[0].position[2],75);
    const downloaded=page.waitForEvent('download');await page.getByRole('button',{name:'导出搭建 JSON',exact:true}).click();await (await downloaded).saveAs(resolve(target,'bench-assembly-ui.json'));
    const report=JSON.parse(await readFile(resolve(target,'bench-assembly-ui.json'),'utf8'));assert.equal(report.items.length,40);assert.equal(report.sources.length,1);assert.equal(report.connections.length,5);assert(report.bom.length>0);
    await page.screenshot({path:resolve(target,'bench-spatial-40.png'),fullPage:true});
    await page.getByRole('button',{name:'二维台面',exact:true}).click();await page.screenshot({path:resolve(target,'bench-plan-40.png'),fullPage:true});
    for(const width of [768,480]){await page.setViewportSize({width,height:1050});await page.screenshot({path:resolve(target,`bench-${width}.png`),fullPage:true});assert(await page.locator('.bench-viewport').isVisible());}
    await page.setViewportSize({width:1600,height:1050});
    // Reopen the same persisted project after an engine restart.
    engine.kill();await new Promise(r=>engine.once('exit',r));await unlink(resolve(workspace,'endpoint.json')).catch(()=>{});await start();await page.reload();await expect(page.locator('.bench-count')).toContainText('40 / 128');assert.equal((await optical('bench_get')).bench.elements.length,40);
    } else {
        assert.equal(original.elements.length,40);
        // Continue from the completed 40-component/restart flow; arrange the added
        // apertures away from the active arms and exercise the JSON draft editor.
        original.elements.filter(e=>e.id.startsWith('E')).forEach((e,i)=>{e.position=[80+(i%12)*60,460+Math.floor(i/12)*45,75];});
        await rawDetails.locator('textarea').fill(JSON.stringify(original));await page.getByRole('button',{name:'将 JSON 用作草稿',exact:true}).click();
        await page.locator('.bench-element-list button').filter({has:page.locator('b',{hasText:/^BS1$/})}).click();
        if(contract){
            const base=page.getByRole('spinbutton',{name:/底座 X 尺寸/});await base.fill('42');await base.blur();assert.equal((await draft()).elements[0].mount.base_size[0],42);
        }
        if(!contract){
        await page.getByRole('spinbutton',{name:'偏航',exact:true}).fill('-44');await page.getByRole('spinbutton',{name:'偏航',exact:true}).blur();assert.equal((await draft()).elements[0].rotation[2],-44);
        await page.getByRole('button',{name:'撤销',exact:true}).click();assert.equal((await draft()).elements[0].rotation[2],-45);
        await expect.poll(async()=>Number(await page.locator('.bench-ledger span').filter({hasText:'探测'}).locator('b').innerText())).toBe(1);
        const all=await page.locator('.bench-ray').count();const option=await page.locator('.bench-branch option').evaluateAll(es=>es.find(e=>e.textContent.includes('D1')).value);await page.locator('.bench-branch select').selectOption(option);assert((await page.locator('.bench-ray').count())<all);await page.locator('.bench-branch select').selectOption('');
        }
        await page.locator('.bench-tools').getByRole('button',{name:'应用更改',exact:true}).click();await expect(page.locator('.bench-tools').getByRole('button',{name:'应用更改',exact:true})).toBeDisabled();
        await expect(page.locator('.bench-ledger')).toBeVisible();
        const downloaded=page.waitForEvent('download');await page.getByRole('button',{name:'导出坐标 CSV',exact:true}).click();await(await downloaded).saveAs(resolve(target,'bench-assembly-ui.csv'));assert((await readFile(resolve(target,'bench-assembly-ui.csv'),'utf8')).includes('"S1"'));
        await rawDetails.locator('summary').click();
        await page.getByRole('button',{name:'三维实验台',exact:true}).click();await page.locator('.bench-viewport').screenshot({path:resolve(target,'bench-spatial-40.png')});
        await page.getByRole('button',{name:'二维台面',exact:true}).click();await page.locator('.bench-viewport').screenshot({path:resolve(target,'bench-plan-40.png')});
        for(const width of [768,480]){await page.setViewportSize({width,height:1050});await page.locator('.bench-viewport').screenshot({path:resolve(target,`bench-${width}.png`)});assert(await page.locator('.bench-viewport').isVisible());}
        await page.setViewportSize({width:1600,height:1050});
    }
    await page.locator('.bench-element-list button').filter({has:page.locator('b',{hasText:/^BS1$/})}).click();await page.getByRole('spinbutton',{name:'X 坐标',exact:true}).fill('310');await page.getByRole('spinbutton',{name:'X 坐标',exact:true}).blur();
    const concurrent=await optical('bench_get');concurrent.bench.name='External agent edit';await optical('bench_set',{bench:concurrent.bench,revision:concurrent.revision});
    await expect(page.locator('.banner').filter({hasText:'草稿'}).first()).toBeVisible({timeout:10000});assert.equal((await draft()).elements[0].position[0],310);await expect(page.locator('.bench-tools').getByRole('button',{name:'应用更改',exact:true})).toBeDisabled();
    assert.deepEqual(errors,[]);
    await writeFile(resolve(target,contract?'bench-ui-contract-evidence.json':resume?'bench-ui-continuation-evidence.json':'bench-ui-evidence.json'),JSON.stringify({status:'passed',workspace,traceTimingsMs:timings,scenarios:contract?['mount base-size editor','JSON draft contract','mount change save','CSV export','current-contract stage screenshots','agent/draft conflict']:resume?['JSON draft editing','rotation editing and undo','40-component active ray display','branch filtering','CSV export','1600/768/480 stage screenshots','overlapped selection via component list','agent/draft conflict']:['read-only example/preview','planar locks','spatial Z drag','Escape/undo','camera separation','late reply rejection','40 components','shared save','assembly export','1600/768/480 views','engine restart','agent/draft conflict'],errors,calls:calls.map(({command,name})=>({command,name}))},null,2));
    console.log('PASS: real Rust + production Vue free-space bench; scope '+(contract?'mount/data contract':resume?'continuation':'main'));
} finally {if(releaseTrace)releaseTrace();if(browser)await browser.close();await new Promise(r=>server.close(r));if(engine&&engine.exitCode===null)engine.kill();}
