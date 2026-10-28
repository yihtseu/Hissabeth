// Real Vue production bundle + Rust engine, using an injected Tauri invoke test bridge.
// This exercises frontend behavior without claiming native WebKit coverage.
import { chromium, expect } from '@playwright/test';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { createConnection } from 'node:net';
import { readFile, writeFile, mkdir, unlink } from 'node:fs/promises';
import { dirname, resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const target = resolve(root, 'src-tauri/target');
const workspace = resolve(target, 'ui-workspace');
const executable = resolve(target, 'debug/hissabeth');
await mkdir(workspace, { recursive: true });
for (const filename of ['endpoint.json', 'system.json']) await unlink(resolve(workspace, filename)).catch(() => {});
let engine;
async function startEngine() {
  engine = spawn(executable, ['--engine'], { env: { ...process.env, HISSABETH_DATA_DIR: workspace }, stdio: ['ignore', 'ignore', 'pipe'] });
  let errors = '';
  engine.stderr.on('data', (chunk) => { errors += chunk; });
  for (let i = 0; i < 100; i++) {
    if (engine.exitCode !== null) throw new Error(errors);
    try { JSON.parse(await readFile(resolve(workspace, 'endpoint.json'), 'utf8')); return; } catch {}
    await new Promise((r) => setTimeout(r, 50));
  }
  throw new Error('Engine did not start');
}
await startEngine();
let id = 0;
async function optical(name, args = {}) {
  const endpoint = JSON.parse(await readFile(resolve(workspace, 'endpoint.json'), 'utf8'));
  const [host, port] = endpoint.address.split(':');
  return new Promise((resolveResult, reject) => {
    const socket = createConnection({ host, port: Number(port) });
    socket.setTimeout(15000);
    socket.on('timeout', () => { socket.destroy(); reject(new Error('Optical invocation timeout')); });
    socket.on('error', reject);
    let response = '';
    socket.on('data', (chunk) => {
      response += chunk;
      if (!response.includes('\n')) return;
      socket.end();
      const result = JSON.parse(response.split('\n')[0]);
      if (result.error) { reject(new Error(result.error.message)); return; }
      if (result.result.isError) { reject(new Error(result.result.content[0].text)); return; }
      resolveResult(JSON.parse(result.result.content[0].text));
    });
    socket.on('connect', () => socket.write(JSON.stringify({ token: endpoint.token, request: { jsonrpc: '2.0', id: ++id, method: 'tools/call', params: { name, arguments: args } } }) + '\n'));
  });
}
const server = createServer(async (req, res) => {
  try {
    const pathname = new URL(req.url, 'http://localhost').pathname;
    if(pathname === '/favicon.ico'){res.writeHead(204).end();return;}
    const file = resolve(root, 'dist', pathname === '/' ? 'index.html' : pathname.slice(1));
    if (!file.startsWith(resolve(root, 'dist') + '/')) { res.writeHead(403).end(); return; }
    const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' };
    res.setHeader('Content-Type', mime[extname(file)] || 'application/octet-stream');
    res.end(await readFile(file));
  } catch { res.writeHead(404).end(); }
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const browser = await chromium.launch({ headless: true });
const errors = [];
let page;
let holdUiReads = false;
let failRefreshAfter = '';
let failUiRead = false;
let freezeAfterRevision = null;
let holdWriteReply = '';
let writeReplyHeld = false;
let releaseWriteReply = null;
const heldReads = [];
const uiCalls = new Map();
const uiRequests = [];
const uiResults = [];
let analysisGate = null;
let gatedAnalysis = null;
let releaseGatedAnalysis = null;
let dispatchingUiReads = 0;
function resumeAnalysis() {
  const resume = releaseGatedAnalysis;
  releaseGatedAnalysis = null;
  if (resume) resume();
}
async function gateAnalysis(stage, name, args) {
  if (analysisGate?.stage !== stage || analysisGate?.name !== name) return;
  analysisGate = null;
  gatedAnalysis = { name, args: structuredClone(args) };
  await new Promise((resume) => { releaseGatedAnalysis = resume; });
  gatedAnalysis = null;
}
const lastUiResult = (name) => uiResults.findLast((r) => r.name === name)?.result;
function resumeUiReads() {
  holdUiReads = false;
  for (const resume of heldReads.splice(0)) resume();
}
function resumeUiWrite() {
  const resume = releaseWriteReply;
  releaseWriteReply = null;
  if (resume) resume();
}
try {
  page = await browser.newPage({ viewport: { width: process.argv.includes('--spot-aspect') ? 2600 : 1440, height: 1000 } });
  page.setDefaultTimeout(10000);
  page.on('console', (message) => { if(message.type()==='error'){errors.push(message.text());console.error(message.text());} });
  page.on('pageerror', (e) => { errors.push(e.message); console.error('Browser error:', e.message); });
  await page.exposeFunction('__opticalInvoke', async (command, args) => {
    if (command === 'optical_call') {
      if (args.name === 'system_get') {
        if (failUiRead) {
          failUiRead = false;
          holdUiReads = true;
          throw new Error('Injected post-commit refresh failure');
        }
        if (holdUiReads) await new Promise((resume) => heldReads.push(resume));
      }
      uiRequests.push({ name: args.name, args: structuredClone(args.args) });
      await gateAnalysis('before', args.name, args.args);
      if (args.name === 'system_get') dispatchingUiReads++;
      let result;
      try { result = await optical(args.name, args.args); }
      finally { if (args.name === 'system_get') dispatchingUiReads--; }
      uiResults.push({ name: args.name, args: structuredClone(args.args), result: structuredClone(result) });
      await gateAnalysis('after', args.name, args.args);
      uiCalls.set(args.name, (uiCalls.get(args.name) || 0) + 1);
      if (args.name === holdWriteReply) {
        holdWriteReply = '';
        writeReplyHeld = true;
        await new Promise((resume) => { releaseWriteReply = resume; });
        writeReplyHeld = false;
      }
      if (args.name === failRefreshAfter) {
        failRefreshAfter = '';
        failUiRead = true;
      }
      if (args.name === 'system_get' && freezeAfterRevision !== null && result.revision > freezeAfterRevision) {
        holdUiReads = true;
        freezeAfterRevision = null;
      }
      return result;
    }
    if (command === 'detect_agents') return [];
    if (command === 'configure_agents') return [];
    throw new Error('Unexpected Tauri command: ' + command);
  });
  await page.addInitScript(() => { globalThis.isTauri = true; window.__TAURI_INTERNALS__ = { invoke: (command, args) => window.__opticalInvoke(command, args) }; });
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await expect(page.getByText('Engine connected', { exact: true })).toBeVisible();
  await expect(page.getByRole('img').first()).toBeVisible();
  if (process.argv.includes('--spectrum-sampling') || process.argv.includes('--spectrum-maximum') || process.argv.includes('--spot-aspect')) {
    const spotSvg = () => page.getByRole('img', {name:/^Spot diagram of/});
    const polySvg = () => page.getByRole('img', {name:/^Polychromatic spot diagram/});
    const geoSvg = () => page.getByRole('img', {name:/^Geometric MTF along/});
    const setNumber = async (locator,value) => { await locator.fill(String(value)); await locator.press('Tab'); };
    if (!process.argv.includes('--spectrum-maximum') && !process.argv.includes('--spot-aspect')) {
    await page.getByRole('tab',{name:'Prescription',exact:true}).click();
    const mapping=page.getByRole('combobox',{name:'Pupil ray mapping',exact:true});
    await expect(mapping).toHaveValue('real_stop');
    await page.getByLabel('Surface 2 is the aperture stop',{exact:true}).check();
    await mapping.selectOption('reference_entrance_pupil');
    assert.equal((await optical('system_get')).system.pupil_mapping,'real_stop');
    await page.getByRole('button',{name:'Apply changes',exact:true}).click();
    await expect.poll(async()=>(await optical('system_get')).system.pupil_mapping).toBe('reference_entrance_pupil');
    await expect(page.getByRole('button',{name:'Save system',exact:true})).toBeVisible();
    const firstOrder=await optical('paraxial');
    const chiefReference=(await optical('system_get')).system;
    const referenceWavelength=chiefReference.wavelengths[chiefReference.reference_wavelength].value;
    const referenceRay=await optical('trace_ray',{x:0,y:.8*firstOrder.entrance_pupil_radius,field_index:0,wavelength:referenceWavelength});
    await mapping.selectOption('real_stop');
    await page.getByRole('button',{name:'Apply changes',exact:true}).click();
    await expect.poll(async()=>(await optical('system_get')).system.pupil_mapping).toBe('real_stop');
    await expect(page.getByRole('button',{name:'Save system',exact:true})).toBeVisible();
    const realStopRay=await optical('trace_ray',{x:0,y:.8*firstOrder.entrance_pupil_radius,field_index:0,wavelength:referenceWavelength});
    assert.ok(Math.abs(realStopRay.segments[1].point[1]-referenceRay.segments[1].point[1])>1e-5);
    assert.ok(Math.abs((await optical('paraxial')).efl-firstOrder.efl)<1e-12);
    await page.getByRole('tab',{name:'Spot diagram',exact:true}).click();
    await expect(spotSvg()).toBeVisible();
    await page.getByRole('combobox',{name:'Ray pattern',exact:true}).selectOption('hexapolar');
    await expect(spotSvg()).toHaveAttribute('aria-label', /of 217 transmitted rays/);
    // Hold a real computed response while a newer sampling request completes.
    analysisGate={name:'spot_diagram',stage:'after'};
    await setNumber(page.getByRole('spinbutton',{name:/^Rings/}),4);
    await expect.poll(()=>gatedAnalysis?.name).toBe('spot_diagram');
    await expect(spotSvg()).toHaveCount(0);
    await setNumber(page.getByRole('spinbutton',{name:/^Rings/}),5);
    await expect(spotSvg()).toHaveAttribute('aria-label', /of 91 transmitted rays/);
    resumeAnalysis();
    await expect.poll(()=>gatedAnalysis).toBe(null);
    await page.evaluate(()=>new Promise(requestAnimationFrame));
    await expect(spotSvg()).toHaveAttribute('aria-label', /of 91 transmitted rays/);
    await page.getByRole('combobox',{name:'View',exact:true}).selectOption('all');
    await expect(polySvg()).toHaveAttribute('aria-label', /of 273 transmitted rays/);
    await expect(page.locator('.ledger tbody tr')).toHaveCount(3);
    const poly=lastUiResult('polychromatic_spot');
    assert.equal(poly.sampling.kind,'hexapolar');
    assert.equal(poly.sampled_rays,273);
    assert.ok(Math.abs(poly.weights.reduce((a,b)=>a+b,0)-poly.transmitted_weight)<1e-12);
    await page.getByRole('combobox',{name:'View',exact:true}).selectOption('single');
    await page.getByRole('combobox',{name:'Wavelength',exact:true}).selectOption('2');
    await expect(spotSvg()).toBeVisible();
    // Preserve wavelength by value and ray sampling while the saved spectrum is reordered.
    let saved=await optical('system_get');
    const selected=saved.system.wavelengths[2].value;
    saved.system.wavelengths=[{value:selected,weight:1},{value:selected,weight:2},{value:486.1327,weight:.5},{value:435.834,weight:0}];
    saved.system.reference_wavelength=1;
    saved.system.fields[2].x=3;saved.system.fields[2].y=4;
    await optical('system_set',{system:saved.system,revision:saved.revision});
    await expect(page.getByRole('combobox',{name:'Wavelength',exact:true})).toHaveValue('0');
    await expect(page.getByRole('combobox',{name:'Ray pattern',exact:true})).toHaveValue('hexapolar');
    await expect(page.getByRole('spinbutton',{name:/^Rings/})).toHaveValue('5');
    await expect(spotSvg()).toHaveAttribute('aria-label', /of 91 transmitted rays/);
    await page.getByRole('combobox',{name:'View',exact:true}).selectOption('all');
    await expect(polySvg()).toHaveAttribute('aria-label', /of 273 transmitted rays/);
    await expect(page.locator('.ledger tbody tr')).toHaveCount(3); // duplicate λ entries plus one distinct active λ
    assert.equal(lastUiResult('polychromatic_spot').failures.length,3); // zero weight is absent

    await page.getByRole('tab',{name:'MTF',exact:true}).click();
    await expect(geoSvg()).toBeVisible();
    await page.getByRole('combobox',{name:'Spectral mode',exact:true}).selectOption('poly');
    await expect(page.locator('.ledger tbody tr')).toHaveCount(3);
    await expect(page.getByRole('combobox',{name:'Wavelength',exact:true})).toHaveCount(0);
    await page.getByRole('combobox',{name:'Ray pattern',exact:true}).selectOption('hexapolar');
    await setNumber(page.getByRole('spinbutton',{name:/^Rings/}),3);
    await expect(geoSvg()).toBeVisible();
    await expect.poll(()=>lastUiResult('geometric_mtf')?.rays).toBe(111);
    const maximum=page.getByRole('spinbutton',{name:/^Max frequency/});
    await setNumber(maximum,.25);
    await expect.poll(()=>lastUiResult('geometric_mtf')?.frequencies.at(-1)).toBe(.25);
    await expect(geoSvg().locator('text[text-anchor="middle"]').filter({hasText:/^0\.25$/})).toBeVisible();
    const callsBeforeInvalid=uiCalls.get('geometric_mtf');
    await setNumber(maximum,0);
    await expect(maximum).toHaveValue('0.25');
    assert.equal(uiCalls.get('geometric_mtf'),callsBeforeInvalid);
    // Old axis response must never replace the current curve or its provenance.
    analysisGate={name:'geometric_mtf',stage:'after'};
    await page.getByRole('combobox',{name:/^Axis/}).selectOption('x');
    await expect.poll(()=>gatedAnalysis?.name).toBe('geometric_mtf');
    await expect(geoSvg()).toHaveCount(0);
    await page.getByRole('combobox',{name:/^Axis/}).selectOption('y');
    await expect(geoSvg()).toHaveAttribute('aria-labelledby','mtf-svg-title');
    await expect(geoSvg().locator('title')).toContainText('global y');
    resumeAnalysis();
    await expect.poll(()=>gatedAnalysis).toBe(null);
    await page.evaluate(()=>new Promise(requestAnimationFrame));
    await expect(geoSvg().locator('title')).toContainText('global y');
    // Dispatch the captured request only after another real writer advances the engine.
    holdUiReads=true;
    await expect.poll(()=>dispatchingUiReads).toBe(0);
    analysisGate={name:'geometric_mtf',stage:'before'};
    await setNumber(maximum,50);
    await expect.poll(()=>gatedAnalysis?.name).toBe('geometric_mtf');
    saved=await optical('system_get');saved.system.name='Spectrum revision race';
    await optical('system_set',{system:saved.system,revision:saved.revision});
    resumeAnalysis();
    await expect(page.getByRole('alert').filter({hasText:/MTF: the saved system is now at revision/})).toBeVisible();
    await expect(geoSvg()).toHaveCount(0);
    resumeUiReads();
    await expect(page.getByRole('heading',{name:'Spectrum revision race',exact:true})).toBeVisible();
    await expect(geoSvg()).toBeVisible();
    saved=await optical('system_get');
    await expect(page.getByText(new RegExp(`Revision ${saved.revision} · Field`))).toBeVisible();
    await expect(page.getByRole('combobox',{name:'Spectral mode',exact:true})).toHaveValue('poly');
    await expect(page.getByRole('spinbutton',{name:/^Rings/})).toHaveValue('3');

    await page.getByRole('tab',{name:'Analysis',exact:true}).click();
    const analysis=page.getByRole('combobox',{name:/^Analysis/});
    await analysis.selectOption('diffraction_mtf');
    await page.getByRole('combobox',{name:'Field',exact:true}).selectOption('2');
    await page.getByRole('combobox',{name:'Spectral mode',exact:true}).selectOption('poly');
    await expect(page.locator('.analysis-body')).toHaveAttribute('aria-busy','false');
    await expect(page.getByRole('img',{name:/^Diffraction MTF/})).toBeVisible();
    await expect(page.locator('.ledger tbody tr')).toHaveCount(3);
    await expect(page.getByRole('combobox',{name:'Ray pattern',exact:true})).toHaveCount(0);
    await expect(page.getByRole('combobox',{name:'Wavelength',exact:true})).toHaveCount(0);
    await expect(page.getByText(/Local basis:/)).toBeVisible();
    await expect(page.getByText('Sampling limit',{exact:true})).toBeVisible();
    const diffraction=lastUiResult('diffraction_mtf');
    assert.equal(diffraction.components.length,3);
    assert.equal(diffraction.pupil_grid,32);
    assert.equal(diffraction.max_frequency,diffraction.sampling_limit);
    assert.ok(Math.abs(diffraction.frame.tangential[0])>.1);
    assert.ok(Math.abs(diffraction.frame.tangential[1])>.1);
    await page.screenshot({path:resolve(target,'spectrum-sampling-diffraction.png'),fullPage:true});
    await analysis.selectOption('encircled_energy');
    await page.getByRole('combobox',{name:'Ray pattern',exact:true}).selectOption('hexapolar');
    await setNumber(page.getByRole('spinbutton',{name:/^Rings/}),2);
    await expect(page.getByRole('img',{name:/^Encircled energy/})).toBeVisible();
    await expect.poll(()=>lastUiResult('encircled_energy')?.sampled_rays).toBe(19);
    // Every AnalysisPanel outcome uses actual reply revision, not the header's live value.
    holdUiReads=true;
    await expect.poll(()=>dispatchingUiReads).toBe(0);
    analysisGate={name:'encircled_energy',stage:'before'};
    await setNumber(page.getByRole('spinbutton',{name:/^Rings/}),3);
    await expect.poll(()=>gatedAnalysis?.name).toBe('encircled_energy');
    saved=await optical('system_get');saved.system.name='Energy revision race';
    await optical('system_set',{system:saved.system,revision:saved.revision});
    resumeAnalysis();
    await expect(page.getByRole('alert').filter({hasText:/^Encircled energy: the saved system is now at revision/})).toBeVisible();
    await expect(page.getByRole('img',{name:/^Encircled energy/})).toHaveCount(0);
    resumeUiReads();
    await expect(page.getByRole('heading',{name:'Energy revision race',exact:true})).toBeVisible();
    await expect(page.getByRole('img',{name:/^Encircled energy/})).toBeVisible();
    await expect(page.getByRole('spinbutton',{name:/^Rings/})).toHaveValue('3');
    await expect.poll(()=>lastUiResult('encircled_energy')?.sampled_rays).toBe(37);
    await page.getByRole('tab',{name:'MTF',exact:true}).click();
    await expect(geoSvg()).toBeVisible();
    engine.kill();await new Promise((r)=>engine.once('exit',r));
    await expect(page.getByText('Engine offline',{exact:true})).toBeVisible({timeout:5000});
    await expect(geoSvg()).toHaveCount(0);
    await unlink(resolve(workspace,'endpoint.json')).catch(()=>{});
    await startEngine();
    await expect(page.getByText('Engine connected',{exact:true})).toBeVisible({timeout:5000});
    await expect(geoSvg()).toBeVisible();
    saved=await optical('system_get');
    await expect(page.getByText(new RegExp(`Revision ${saved.revision} · Field`))).toBeVisible();
    await expect(page.getByRole('combobox',{name:'Spectral mode',exact:true})).toHaveValue('poly');
    await expect(page.getByRole('spinbutton',{name:/^Rings/})).toHaveValue('3');
    await page.screenshot({path:resolve(target,'spectrum-sampling-mtf.png'),fullPage:true});
    }
    if (process.argv.includes('--spectrum-maximum') || process.argv.includes('--spot-aspect')) {
    // Exercise the largest grid with all 32 active wavelengths through Rust and canvas.
    const wide=process.argv.includes('--spot-aspect');
    const waveCount=wide?1:32, grid=wide?65:129;
    let saved=await optical('system_get');
    const plane={...saved.system.surfaces[0],radius:0,thickness:10,material:'AIR',aperture:2,clear_aperture:false,conic:0,even:[],odd:[],gap_solve:{kind:'fixed'}};
    saved.system.surfaces=[plane,{...plane}];saved.system.stop_index=0;
    saved.system.object_distance=null;saved.system.fields=[{x:0,y:0,weight:1,vignetting:[0,0,0,0]}];
    saved.system.pupil={kind:'entrance_pupil_diameter',value:2};
    saved.system.wavelengths=Array.from({length:waveCount},()=>({value:587.562,weight:1}));saved.system.reference_wavelength=0;
    await optical('system_set',{system:saved.system,revision:saved.revision});
    await page.getByRole('tab',{name:'Spot diagram',exact:true}).click();
    await page.getByRole('combobox',{name:'Ray pattern',exact:true}).selectOption('grid');
    await setNumber(page.getByRole('spinbutton',{name:/^Diameter points/}),grid);
    await page.getByRole('combobox',{name:'View',exact:true}).selectOption('all');
    let gridCount=0;
    for(let i=0;i<grid;i++)for(let j=0;j<grid;j++)if((2*i/(grid-1)-1)**2+(2*j/(grid-1)-1)**2<=1)gridCount++;
    await expect(polySvg()).toHaveAttribute('aria-label',new RegExp(`of ${waveCount*gridCount} transmitted rays`),{timeout:60000});
    const pixels=await polySvg().evaluate(async(el)=>{await new Promise(requestAnimationFrame);const ctx=el.getContext('2d');return ctx.getImageData(0,0,el.width,el.height).data.reduce((n,v,i)=>n+(i%4===3&&v>0),0);});
    assert.ok(pixels>1000);
    if(wide) {
      const picture=await polySvg().screenshot({path:resolve(target,'spectrum-spot-wide.png')});
      const bounds=await page.evaluate(async(base64)=>{
        const img=new Image();img.src='data:image/png;base64,'+base64;await img.decode();
        const c=document.createElement('canvas');c.width=img.width;c.height=img.height;const ctx=c.getContext('2d');ctx.drawImage(img,0,0);
        const pixels=ctx.getImageData(0,0,c.width,c.height).data;let x0=c.width,x1=0,y0=c.height,y1=0;
        for(let y=0;y<c.height;y++)for(let x=0;x<c.width;x++){const i=4*(y*c.width+x);if(pixels[i]>pixels[i+2]+50&&pixels[i+1]>pixels[i+2]+50&&pixels[i+3]>0){x0=Math.min(x0,x);x1=Math.max(x1,x);y0=Math.min(y0,y);y1=Math.max(y1,y);}}
        return {ratio:x1>x0&&y1>y0?(x1-x0)/(y1-y0):null,width:c.width,height:c.height};
      },picture.toString('base64'));
      assert.ok(bounds.width/bounds.height>2.5);assert.ok(bounds.ratio!==null&&Math.abs(bounds.ratio-1)<.03,JSON.stringify(bounds));
      console.log('PASS: wide CSS canvas '+bounds.width+'x'+bounds.height+', rendered circular spot aspect '+bounds.ratio);
    } else {
      await page.screenshot({path:resolve(target,'spectrum-sampling-maximum-spot.png'),fullPage:true});
      console.log('PASS: maximum 129 grid / 32 wavelengths, '+waveCount*gridCount+' actual Rust intercepts drawn on canvas');
    }
    }
    for(const request of uiRequests) {
      if(['spot_diagram','polychromatic_spot','geometric_mtf','encircled_energy'].includes(request.name)) {
        assert.ok(request.args.sampling);assert.ok(!('grid' in request.args));
      }
      if(['geometric_mtf','diffraction_mtf'].includes(request.name)) {
        assert.ok(request.args.spectrum);assert.ok(!('wavelength' in request.args));
      }
    }
    assert.deepEqual(errors,[]);
    if (!process.argv.includes('--spectrum-maximum') && !process.argv.includes('--spot-aspect')) console.log('PASS: grid/hex sampling and weighted spots/energy; duplicate and zero spectral weights; mono/poly geometric and oblique diffraction MTF; sub-unit frequency labels; late-response cancellation; real engine-ahead revisions discarded then recovered; selection preservation and restart/offline recovery; no browser exceptions');
  } else if (process.argv.includes('--apertures')) {
    const initial = await optical('system_get');
    const plane = { ...structuredClone(initial.system.surfaces[0]), radius: 0, thickness: 10, material: 'AIR', aperture: 1, clear_aperture: false, semi_diameter: 999, conic: 0, even: [], odd: [] };
    const second = { ...structuredClone(plane), aperture: 2, clear_aperture: true };
    const system = { ...initial.system, surfaces: [plane, second], fields: [{ x: 0, y: Math.atan(.1) * 180 / Math.PI, weight: 1, vignetting: [0,0,0,0] }], pupil: {kind: 'entrance_pupil_diameter', value: 10}, object_distance: null, stop_index: 0 };
    await optical('system_set', {system, revision: initial.revision});
    await page.reload();
    await expect(page.getByText('Engine connected', {exact:true})).toBeVisible();
    await page.getByRole('tab', {name:'Prescription',exact:true}).click();
    const drawing = page.getByLabel('Surface 2 draw semi-diameter (derived, read-only)', {exact:true});
    await expect(drawing).toHaveAttribute('readonly','');
    assert.ok(Math.abs(Number(await drawing.inputValue())-6)<1e-8);
    const coverage = page.locator('details').filter({has:page.locator('summary').filter({hasText:/^Aperture coverage$/})});
    await coverage.locator('summary').click();
    await expect(coverage.getByText('No geometric failures.',{exact:true})).toBeVisible();
    await expect(coverage).toContainText('5 of 5 rays traced completely');
    failRefreshAfter = 'vignetting_update';
    await page.getByRole('button',{name:'Auto vignetting',exact:true}).click();
    const pending = page.getByRole('alert').filter({ hasText: /Your change was saved, but the view could not refresh/ });
    await expect(pending).toBeVisible();
    await expect(page.getByRole('heading',{name:/Computed boundaries/})).toBeVisible();
    await expect(coverage).not.toContainText('5 of 5 rays traced completely');
    await expect(page.getByRole('button',{name:'Auto vignetting',exact:true})).toBeDisabled();
    const updated = await optical('system_get');
    const expected = [.8,.4,1-Math.sqrt(3)/5,1-Math.sqrt(3)/5];
    updated.system.fields[0].vignetting.forEach((v,i)=>assert.ok(Math.abs(v-expected[i])<2e-9));
    assert.equal(uiCalls.get('vignetting_update'),1);
    resumeUiReads();
    await expect(pending).not.toBeVisible();
    await expect.poll(async()=>Number(await drawing.inputValue())).toBeLessThan(6);
    await expect(coverage.getByText('No geometric failures.',{exact:true})).toBeVisible();
    await expect(page.getByLabel('Surface 2 clear radius',{exact:true})).toHaveValue('2');
    await page.getByRole('button',{name:'Clear vignetting',exact:true}).click();
    await expect.poll(async()=>(await optical('system_get')).system.fields[0].vignetting).toEqual([0,0,0,0]);
    // A dirty draft blocks saved-system vignetting; enlarged boundaries have no input floor.
    await page.getByLabel('Surface 2 clear radius',{exact:true}).fill('10');
    await expect(page.getByRole('button',{name:'Auto vignetting',exact:true})).toBeDisabled();
    await page.getByRole('button',{name:'Apply changes',exact:true}).click();
    await expect(page.getByRole('button',{name:'Auto vignetting',exact:true})).toBeEnabled();
    await page.getByRole('button',{name:'Auto vignetting',exact:true}).click();
    await expect.poll(async()=>(await optical('system_get')).system.fields[0].vignetting[1]).toBeLessThan(-1);
    const vig = page.getByLabel('Field 1 vignetting lower y',{exact:true});
    await expect(vig).not.toHaveAttribute('min', /.+/);
    await expect(vig).not.toHaveAttribute('max', /.+/);
    // A blocked chief rejects the whole automatic update and retains the saved revision.
    await page.getByLabel('Surface 2 clear radius',{exact:true}).fill('0.5');
    await page.getByRole('button',{name:'Apply changes',exact:true}).click();
    await expect(page.getByRole('button',{name:'Auto vignetting',exact:true})).toBeEnabled();
    const beforeFailure=await optical('system_get');
    await page.getByRole('button',{name:'Auto vignetting',exact:true}).click();
    await expect(page.getByRole('alert').filter({hasText:'chief ray'})).toBeVisible();
    assert.deepEqual(await optical('system_get'),beforeFailure);
    // Geometric failures report reached prefixes; the clipping radius remains independent.
    const bad=structuredClone(beforeFailure.system);
    bad.fields=[{x:0,y:0,weight:1,vignetting:[0,0,0,0]}];
    bad.surfaces[1].radius=1; bad.surfaces[1].clear_aperture=false;
    await optical('system_set',{system:bad,revision:beforeFailure.revision});
    await page.reload();
    await expect(page.getByText('Engine connected',{exact:true})).toBeVisible();
    await page.getByRole('tab',{name:'Prescription',exact:true}).click();
    await coverage.locator('summary').click();
    await expect(coverage).toContainText('1 of 5 rays traced completely');
    await expect(coverage.getByRole('heading',{name:'Geometric failures (4)',exact:true})).toBeVisible();
    await expect(page.getByLabel('Surface 2 draw semi-diameter (derived, read-only)',{exact:true})).toHaveValue('0');
    await page.screenshot({path:resolve(target,'apertures-coverage.png'),fullPage:true});
    // Render all analyses changed by this batch against a finite, skew-field prescription.
    const now=await optical('system_get');
    const local=structuredClone(initial.system);
    local.object_distance=200; local.fields=[{x:1.8,y:2.4,weight:1,vignetting:[0,0,0,0]}];
    local.surfaces[1].thickness=70;
    await optical('system_set',{system:local,revision:now.revision});
    await page.reload();
    await expect(page.getByText('Engine connected',{exact:true})).toBeVisible();
    await page.getByRole('tab',{name:'Analysis',exact:true}).click();
    const analysis=page.getByRole('combobox',{name:/^Analysis/});
    for(const kind of ['ray_fan','opd_fan','astigmatism','diffraction_mtf','distortion','wavefront','diffraction_psf']) {
      await analysis.selectOption(kind);
      await expect(page.locator('.analysis-body')).toHaveAttribute('aria-busy','false');
      await expect(page.locator('.analysis-body').getByRole('alert')).toHaveCount(0);
      if(['ray_fan','opd_fan','astigmatism','diffraction_mtf'].includes(kind)) await expect(page.getByText(/Local basis:/)).toBeVisible();
      if(kind==='distortion') await expect(page.getByText(/paraxial chief-ray intercept at the stored image plane/).last()).toBeVisible();
    }
    await analysis.selectOption('ray_fan');
    await expect(page.locator('.analysis-body')).toHaveAttribute('aria-busy','false');
    await page.screenshot({path:resolve(target,'apertures-local-fan.png'),fullPage:true});
    assert.deepEqual(errors,[]);
    console.log('PASS: separate physical/drawing radii; revision-matched partial coverage; auto/clear, negative factors, dirty guard, atomic chief rejection, confirmed-write refresh recovery without duplicate; finite skew-field local analyses and wavefront/PSF; no browser exceptions');
  } else if (process.argv.includes('--layout-wavelength')) {
    const snapshot = await optical('system_get');
    const reference = snapshot.system.wavelengths[snapshot.system.reference_wavelength].value;
    await expect(page.getByText(`λ ${reference} nm`, { exact: true })).toBeVisible();
    await page.getByRole('tab', { name: 'Spot diagram', exact: true }).click();
    const wavelength = page.getByRole('combobox', { name: 'Wavelength', exact: true });
    await expect(wavelength).toHaveValue(String(snapshot.system.reference_wavelength));
    await wavelength.selectOption('0');
    snapshot.system.name = 'Wavelength selection preserved';
    await optical('system_set', { system: snapshot.system, revision: snapshot.revision });
    await expect(page.getByRole('heading', { name: snapshot.system.name, exact: true })).toBeVisible();
    await expect(wavelength).toHaveValue('0');
    await page.getByRole('tab', { name: 'Layout', exact: true }).click();
    await expect(page.getByText(`λ ${reference} nm`, { exact: true })).toBeVisible();
    assert.deepEqual(errors, []);
    console.log('PASS: initial wavelength is reference; saved-system adoption preserves selection; layout label remains reference after spot wavelength change');
  } else if (process.argv.includes('--solve-recovery')) {
    await page.getByRole('tab', { name: 'Prescription', exact: true }).click();
    await page.getByRole('combobox', { name: 'Source gap after surface', exact: true }).selectOption('2');
    await page.getByLabel('Pickup scale', { exact: true }).fill('0.05');
    await page.getByLabel('Pickup offset', { exact: true }).fill('2.5');
    const initial = await optical('system_get');
    await page.getByLabel('Surface 1 radius', { exact: true }).fill('60');
    freezeAfterRevision = initial.revision;
    initial.system.name = 'External revision while editing';
    await optical('system_set', { system: initial.system, revision: initial.revision });
    await expect(page.getByRole('button', { name: 'Discard draft and reload', exact: true })).toBeVisible();
    await page.getByLabel('Surface 1 radius', { exact: true }).fill(String(initial.system.surfaces[0].radius));
    await expect(page.getByRole('button', { name: 'Install solve', exact: true })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Paraxial focus', exact: true })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Save system', exact: true })).toBeDisabled();
    resumeUiReads();
    await expect(page.getByRole('heading', { name: initial.system.name, exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Install solve', exact: true })).toBeEnabled();

    const pending = page.getByRole('alert').filter({ hasText: /Your change was saved, but the view could not refresh/ });
    failRefreshAfter = 'solve';
    await page.getByRole('button', { name: 'Install solve', exact: true }).click();
    await expect(pending).toBeVisible();
    assert.equal(uiCalls.get('solve'), 1);
    let saved = await optical('system_get');
    assert.equal(saved.system.surfaces[0].gap_solve.kind, 'pickup');
    await expect(page.getByRole('button', { name: 'Install solve', exact: true })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Paraxial focus', exact: true })).toBeDisabled();
    resumeUiReads();
    await expect(pending).not.toBeVisible();
    await expect(page.getByText(`Revision ${saved.revision}`, { exact: true })).toBeVisible();
    await expect(page.getByLabel(/^Surface 1 thickness/)).toHaveAttribute('readonly', '');

    await page.getByLabel('Surface 2 thickness', { exact: true }).fill('40');
    failRefreshAfter = 'system_set';
    await page.getByRole('button', { name: 'Apply changes', exact: true }).click();
    await expect(pending).toBeVisible();
    saved = await optical('system_get');
    assert.equal(saved.system.surfaces[1].thickness, 40);
    await expect(page.getByRole('button', { name: 'Apply changes', exact: true })).toBeDisabled();
    resumeUiReads();
    await expect(pending).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Save system', exact: true })).toBeVisible();
    await expect.poll(async () => Number(await page.getByLabel(/^Surface 1 thickness/).inputValue())).toBeCloseTo(4.5, 10);

    failRefreshAfter = 'autofocus';
    await page.getByRole('button', { name: 'Paraxial focus', exact: true }).click();
    await expect(pending).toBeVisible();
    assert.equal(uiCalls.get('autofocus'), 1);
    saved = await optical('system_get');
    const focus = await optical('paraxial');
    assert.ok(Math.abs(saved.system.surfaces[1].thickness - focus.image_distance) < 1e-8);
    resumeUiReads();
    await expect(pending).not.toBeVisible();

    await page.getByLabel('Surface 2 thickness', { exact: true }).fill('42');
    failRefreshAfter = 'system_set';
    await page.getByRole('button', { name: 'Apply changes', exact: true }).click();
    await expect(pending).toBeVisible();
    await page.getByLabel('Surface 2 thickness', { exact: true }).fill('43');
    saved = await optical('system_get');
    assert.equal(saved.system.surfaces[1].thickness, 42);
    resumeUiReads();
    await expect(page.getByRole('button', { name: 'Discard draft and reload', exact: true })).toBeVisible();
    await expect(page.getByLabel('Surface 2 thickness', { exact: true })).toHaveValue('43');
    await page.getByRole('button', { name: 'Discard draft and reload', exact: true }).click();
    await expect(pending).not.toBeVisible();
    await expect(page.getByLabel('Surface 2 thickness', { exact: true })).toHaveValue('42');
    // A newer edit made while the write response is in flight also survives a successful refresh.
    await page.getByLabel('Surface 2 thickness', { exact: true }).fill('44');
    holdWriteReply = 'system_set';
    await page.getByRole('button', { name: 'Apply changes', exact: true }).click();
    await expect.poll(() => writeReplyHeld).toBe(true);
    await page.getByLabel('Surface 2 thickness', { exact: true }).fill('45');
    resumeUiWrite();
    await expect(page.getByRole('button', { name: 'Discard draft and reload', exact: true })).toBeVisible();
    await expect(page.getByLabel('Surface 2 thickness', { exact: true })).toHaveValue('45');
    assert.equal((await optical('system_get')).system.surfaces[1].thickness, 44);
    await page.getByRole('button', { name: 'Discard draft and reload', exact: true }).click();
    await expect(pending).not.toBeVisible();
    await expect(page.getByLabel('Surface 2 thickness', { exact: true })).toHaveValue('44');
    await page.getByRole('button', { name: 'Remove solve on gap after surface 1', exact: true }).click();
    await expect(page.getByLabel('Surface 1 thickness', { exact: true })).not.toHaveAttribute('readonly', '');
    await page.getByRole('combobox', { name: 'Gap after surface', exact: true }).selectOption('2');
    await page.getByRole('combobox', { name: 'Solve', exact: true }).selectOption('marginal_height');
    await page.getByLabel('Pupil zone', { exact: true }).fill('1.5');
    await page.getByLabel('Solve target', { exact: true }).fill('0');
    await page.getByRole('button', { name: 'Install solve', exact: true }).click();
    await expect.poll(async () => (await optical('system_get')).system.surfaces[1].gap_solve.zone).toBe(1.5);
    const paraxial = await optical('paraxial');
    const ray = await optical('trace_ray', { x: 0, y: 1.5 * paraxial.entrance_pupil_radius, field_index: 0 });
    assert.ok(Math.abs(ray.points.at(-1)[1]) < 1e-8);
    assert.deepEqual(errors, []);
    console.log('PASS: stale-but-clean actions blocked; solve/save/autofocus confirmed commits survive refresh failures without retries; safe polling recovery preserves newer drafts; overfilled zone works in UI and real tracing');
  } else if (process.argv.includes('--persistent-solves')) {
    await page.getByRole('tab', { name: 'Prescription', exact: true }).click();
    await page.getByRole('combobox', { name: 'Gap after surface', exact: true }).selectOption('2');
    await page.getByRole('combobox', { name: 'Solve', exact: true }).selectOption('pickup');
    await page.getByRole('combobox', { name: 'Source gap after surface', exact: true }).selectOption('1');
    await page.getByLabel('Pickup scale', { exact: true }).fill('2');
    await page.getByLabel('Pickup offset', { exact: true }).fill('1');
    await page.getByRole('button', { name: 'Install solve', exact: true }).click();
    await expect.poll(async () => (await optical('system_get')).system.surfaces[1].thickness).toBe(11);
    await expect(page.getByLabel(/^Surface 2 thickness/)).toHaveAttribute('readonly', '');
    await expect(page.getByRole('button', { name: 'Paraxial focus', exact: true })).toBeDisabled();
    await expect(page.getByRole('button', { name: /^Cannot remove surface 1:/ })).toBeDisabled();
    await page.getByLabel('Surface 1 thickness', { exact: true }).fill('7');
    await expect(page.getByRole('button', { name: 'Recalculate all', exact: true })).toBeDisabled();
    await page.getByRole('button', { name: 'Apply changes', exact: true }).click();
    await expect.poll(async () => (await optical('system_get')).system.surfaces[1].thickness).toBe(15);
    await page.getByRole('button', { name: 'Edit solve on gap after surface 2', exact: true }).click();
    await expect(page.getByLabel('Pickup scale', { exact: true })).toHaveValue('2');
    await page.getByLabel('Pickup scale', { exact: true }).fill('3');
    await page.getByRole('button', { name: 'Update solve', exact: true }).click();
    await expect.poll(async () => (await optical('system_get')).system.surfaces[1].thickness).toBe(22);
    await page.getByRole('button', { name: 'Recalculate all', exact: true }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Recalculated 1 active solve.' })).toBeVisible();
    await page.getByRole('tab', { name: 'Import / Export', exact: true }).click();
    await page.getByRole('combobox', { name: 'Format', exact: true }).selectOption('hissabeth');
    await page.getByRole('button', { name: 'Export saved system', exact: true }).click();
    await expect(page.getByLabel('Project text', { exact: true })).toHaveValue(/"gap_solve"/);
    const exported = JSON.parse(await page.getByLabel('Project text', { exact: true }).inputValue());
    assert.deepEqual(exported.surfaces[1].gap_solve, { kind: 'pickup', source: 0, scale: 3, offset: 1 });
    const spacer = structuredClone(exported.surfaces[0]);
    Object.assign(spacer, { label: 'Entrance spacer', radius: 0, thickness: 1, material: 'AIR', interaction: 'dummy', gap_solve: { kind: 'fixed' } });
    exported.surfaces.unshift(spacer);
    exported.surfaces[2].gap_solve.source = 1;
    exported.surfaces[2].thickness = 999;
    exported.stop_index += 1;
    exported.name = 'Persistent solve restored';
    await page.getByLabel('Project text', { exact: true }).fill(JSON.stringify(exported));
    await page.getByRole('button', { name: 'Import (replaces saved system)', exact: true }).click();
    await expect(page.getByRole('heading', { name: exported.name, exact: true })).toBeVisible();
    await expect.poll(async () => (await optical('system_get')).system.surfaces[2].thickness).toBe(22);
    await page.getByRole('tab', { name: 'Prescription', exact: true }).click();
    await page.getByRole('button', { name: 'Remove surface 1', exact: true }).click();
    await page.getByRole('button', { name: 'Apply changes', exact: true }).click();
    await expect.poll(async () => (await optical('system_get')).system.surfaces.length).toBe(2);
    assert.equal((await optical('system_get')).system.surfaces[1].gap_solve.source, 0);
    await page.getByRole('button', { name: 'Remove solve on gap after surface 2', exact: true }).click();
    await expect(page.getByLabel('Surface 2 thickness', { exact: true })).not.toHaveAttribute('readonly', '');
    await expect(page.getByRole('button', { name: 'Paraxial focus', exact: true })).toBeEnabled();
    // Re-mount the editor with an active relation on its initial first gap.
    const snapshot = await optical('system_get');
    await optical('solve', { gap: 0, definition: { kind: 'pickup', source: 1, scale: 0.05, offset: 2.5 }, revision: snapshot.revision });
    await page.getByRole('tab', { name: 'Layout', exact: true }).click();
    await expect.poll(async () => (await optical('system_get')).system.surfaces[0].thickness).toBeCloseTo(3.6);
    await page.getByRole('tab', { name: 'Prescription', exact: true }).click();
    await page.getByRole('button', { name: 'Edit solve on gap after surface 1', exact: true }).click();
    await expect(page.getByRole('combobox', { name: 'Source gap after surface', exact: true })).toHaveValue('2');
    await expect(page.getByLabel('Pickup scale', { exact: true })).toHaveValue('0.05');
    await expect(page.getByLabel('Pickup offset', { exact: true })).toHaveValue('2.5');
    const changed = await optical('system_get');
    await optical('solve', { gap: 0, definition: { kind: 'pickup', source: 1, scale: 0.06, offset: 2.5 }, revision: changed.revision });
    await expect(page.getByLabel('Pickup scale', { exact: true })).toHaveValue('0.06');
    await page.getByLabel('Pickup offset', { exact: true }).fill('2.8');
    const recalculated = await optical('solve_recalculate', { revision: (await optical('system_get')).revision });
    // An unrelated revision must not replace an unfinished solve form.
    await expect(page.getByText(`Revision ${recalculated.revision}`, { exact: true })).toBeVisible();
    await expect(page.getByLabel('Pickup offset', { exact: true })).toHaveValue('2.8');
    await page.getByRole('button', { name: 'Update solve', exact: true }).click();
    await expect.poll(async () => (await optical('system_get')).system.surfaces[0].gap_solve.offset).toBe(2.8);
    await page.getByRole('button', { name: 'Paraxial focus', exact: true }).click();
    await expect.poll(async () => {
      const s = (await optical('system_get')).system;
      const p = await optical('paraxial');
      return Math.abs(s.surfaces[1].thickness - p.image_distance) + Math.abs(s.surfaces[0].thickness - 0.06 * s.surfaces[1].thickness - 2.8);
    }).toBeLessThan(1e-8);
    await page.screenshot({ path: resolve(target, 'persistent-solves.png'), fullPage: true });
    engine.kill();
    await new Promise((r) => engine.once('exit', r));
    await expect(page.getByText('Engine offline', { exact: true })).toBeVisible({ timeout: 5000 });
    await unlink(resolve(workspace, 'endpoint.json'));
    await startEngine();
    await expect(page.getByText('Engine connected', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByLabel(/^Surface 1 thickness/)).toHaveAttribute('readonly', '');
    assert.deepEqual(errors, []);
    console.log('PASS: persistent solve UI install/edit/recalculate/remove, readonly thickness, dirty-draft block, autofocus block, native import/export, dependency-preserving deletion, initial selected definition, and recovery; no browser exceptions');
  } else {
  await page.getByRole('tab', { name: 'Prescription', exact: true }).click();
  await page.getByLabel('Surface 1 radius', { exact: true }).fill('55');
  await page.getByRole('button', { name: 'Apply changes', exact: true }).click();
  await expect.poll(async () => (await optical('system_get')).system.surfaces[0].radius).toBe(55);
  await page.getByRole('combobox', { name: /^Gap after surface/ }).selectOption('2');
  await page.getByRole('combobox', { name: 'Solve', exact: true }).selectOption('overall_length');
  await page.getByRole('combobox', { name: 'Last gap after surface (inclusive)', exact: true }).selectOption('2');
  await page.getByLabel('Solve target', { exact: true }).fill('57');
  await page.getByRole('button', { name: 'Install solve', exact: true }).click();
  await expect.poll(async () => (await optical('system_get')).system.surfaces[1].thickness).toBe(52);
  await page.getByRole('tab', { name: 'Analysis', exact: true }).click();
  const analysis = page.getByRole('combobox', { name: /^Analysis/ });
  for (const kind of ['ray_fan', 'opd_fan', 'chromatic_focus', 'longitudinal_aberration', 'astigmatism', 'distortion', 'wavefront', 'diffraction_psf', 'diffraction_mtf', 'encircled_energy']) {
    await analysis.selectOption(kind);
    await expect(page.locator('.analysis-body')).toBeVisible();
    await expect(page.locator('.analysis-body')).toHaveAttribute('aria-busy', 'false');
    assert.equal(await page.locator('.analysis-body').getByRole('alert').count(), 0);
  }
  await page.screenshot({ path: resolve(target, 'workbench-analysis.png'), fullPage: true });
  await page.getByRole('tab', { name: 'Materials', exact: true }).click();
  await page.getByLabel('Search', { exact: true }).fill('N-BK7');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await expect(page.getByText('N-BK7_SCHOTT', { exact: true }).first()).toBeVisible({ timeout: 10000 });
  await page.getByRole('tab', { name: 'Prescription', exact: true }).click();
  await page.getByLabel('Surface 1 radius', { exact: true }).fill('60');
  const snapshot = await optical('system_get');
  snapshot.system.name = 'Agent changed this optical system';
  await optical('system_set', { system: snapshot.system, revision: snapshot.revision });
  await expect(page.getByRole('button', { name: 'Discard draft and reload', exact: true })).toBeVisible({ timeout: 5000 });
  await page.getByRole('button', { name: 'Discard draft and reload', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Agent changed this optical system', exact: true })).toBeVisible();
  await page.getByRole('tab', { name: 'Layout', exact: true }).click();
  await page.screenshot({ path: resolve(target, 'workbench-layout.png'), fullPage: true });
  engine.kill();
  await new Promise((r) => engine.once('exit', r));
  await expect(page.getByText('Engine offline', { exact: true })).toBeVisible({ timeout: 5000 });
  await unlink(resolve(workspace, 'endpoint.json'));
  await startEngine();
  await expect(page.getByText('Engine connected', { exact: true })).toBeVisible({ timeout: 5000 });
  assert.deepEqual(errors, []);
  console.log('PASS: production Vue with real Rust bridge: edit/save, gap solve, ten analysis views, material search, MCP draft conflict, saved-state reconnection; no browser exceptions');
  }
} catch(e) {
  if(page){await page.screenshot({path:resolve(target,'workbench-failure.png'),fullPage:true});await writeFile(resolve(target,'ui-failure.html'),await page.content());}
  throw e;
} finally {
  resumeAnalysis();
  resumeUiWrite();
  resumeUiReads();
  await browser.close();
  server.close();
  if (engine.exitCode === null) { engine.kill(); await new Promise((r) => engine.once('exit', r)); }
}
