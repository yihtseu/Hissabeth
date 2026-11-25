// Bundled discovery against subprocess fixtures; no model inference or personal settings writes.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { readFile, writeFile, chmod } from 'node:fs/promises';
import { resolve } from 'node:path';
const root = resolve(import.meta.dirname, '../..');
const fixture = resolve(root, 'src-tauri/target/agent-cli-fixtures');
const source = await readFile(resolve(root, 'src-tauri/tests/agent_sdk_fixture.py'));
for (const provider of ['codex', 'claude']) {
    const path = resolve(fixture, provider);
    await writeFile(path, source); await chmod(path, 0o755);
}
const control = resolve(fixture, 'chat-claude-control.jsonl');
const before = (await readFile(control).catch(() => Buffer.alloc(0))).length;
function run(provider: string, executable = resolve(fixture, provider)) {
    return new Promise<{ code: number | null; output: string; error: string }>((done, fail) => {
        const child = spawn(process.execPath, [resolve(root, 'dist/agent-bridge.mjs'), '--models', provider, executable, fixture, '0.1.0']);
        let output = '', error = '';
        child.stdout!.on('data', bytes => { output += bytes; });
        child.stderr!.on('data', bytes => { error += bytes; });
        child.on('error', fail);
        child.on('close', code => done({ code, output, error }));
    });
}
const [codex, claude] = await Promise.all([run('codex'), run('claude')]);
assert.equal(codex.code, 0, codex.error);
assert.equal(claude.code, 0, claude.error);
assert.deepEqual(JSON.parse(codex.output).map((model: { id: string }) => model.id), ['codex-fixture-a', 'codex-fixture-b']);
assert.deepEqual(JSON.parse(claude.output).map((model: { id: string }) => model.id), ['claude-fixture-a', 'claude-fixture-b']);
const requests = (await readFile(control)).subarray(before).toString().trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
assert(requests.some(request => request.type === 'control_request' && request.request.subtype === 'initialize'));
assert(!requests.some(request => request.type === 'user'), 'Model discovery must not submit a user turn');
const failed = await run('codex', resolve(fixture, 'missing-model-cli'));
assert.notEqual(failed.code, 0);
assert.match(failed.error, /ENOENT/);
await writeFile(resolve(root, 'src-tauri/target/chat-model-catalogs.json'), JSON.stringify({ codex: JSON.parse(codex.output), claude: JSON.parse(claude.output) }, null, 2));
console.log('PASS: bundled Codex paginated model/list, hidden filtering and deduplication; Claude SDK supportedModels without inference; process failure reports');
