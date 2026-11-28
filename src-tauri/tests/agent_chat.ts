// Bridge behavior against SDK fixtures: no real agent processes are launched.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runTurn, type BridgeRequest } from '../../scripts/agent-bridge';
import { CHECK_ITEMS, FIGURE_LIMIT, FIGURE_MAX, checksOf, figureOf, storeFigure, toolName } from '../../scripts/chat-figures';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

type Sdk = NonNullable<Parameters<typeof runTurn>[3]>;
/* eslint-disable @typescript-eslint/no-explicit-any */
const request: BridgeRequest = { app: '/apps/Optical Workbench/hissabeth', workspace: '/data/current project', executable: '/agents/codex', provider: 'codex', prompt: 'Inspect the saved project', model: 'model-from-picker', session: null };
test('Codex scopes MCP, resumes by token, streams tools and text, and requires completion', async () => {
    let configuration: any, options: any, resumed: string | undefined;
    const events: Record<string, unknown>[] = [];
    class Codex {
        constructor(config: any) { configuration = config; }
        resumeThread(session: string, opts: any) { resumed = session; return this.startThread(opts); }
        startThread(opts: any) { options = opts; return { runStreamed: async (_: unknown, turn: { signal: AbortSignal }) => {
            assert.ok(turn.signal instanceof AbortSignal);
            return { events: (async function* () {
                yield { type: 'thread.started', thread_id: 'session-one' };
                yield { type: 'item.started', item: { type: 'mcp_tool_call', id: 'tool-one', tool: 'system_get', status: 'in_progress', arguments: {} } };
                yield { type: 'item.updated', item: { type: 'agent_message', id: 'answer', text: 'Saved revision 2' } };
                yield { type: 'turn.completed' };
            })() };
        } }; }
    }
    await runTurn({ ...request, session: 'previous' }, new AbortController().signal, event => events.push(event), { Codex } as unknown as Sdk);
    assert.equal(resumed, 'previous');
    assert.equal(options.model, 'model-from-picker');
    assert.equal(options.sandboxMode, 'read-only');
    assert.equal(options.approvalPolicy, 'never');
    assert.equal(configuration.config.features.shell_tool, false);
    assert.match(configuration.configOverrides[0], /^mcp_servers=\{/);
    assert.match(configuration.configOverrides[0], /HISSABETH_DATA_DIR/);
    assert.match(configuration.configOverrides[0], /"default_tools_approval_mode"="approve"/);
    assert.equal(events[0]!.session, 'session-one');
    assert.equal(events[1]!.name, 'system_get');
    assert.equal(events[2]!.text, 'Saved revision 2');
    class Broken { startThread() { return { runStreamed: async () => ({ events: (async function* () {})() }) }; } }
    await assert.rejects(runTurn(request, new AbortController().signal, () => {}, { Codex: Broken } as unknown as Sdk), /before turn completion/);
});

test('Claude streams without duplicate text, scopes tools, carries session and rejects failed results', async () => {
    let options: any;
    const emitted: Record<string, unknown>[] = [];
    function query(input: any) {
        options = input.options;
        const stream = (async function* () {
            yield { type: 'system', subtype: 'init', session_id: 'claude-session', mcp_servers: [{ name: 'hissabeth', status: 'connected' }] };
            yield { type: 'stream_event', event: { type: 'message_start', message: { id: 'reply' } } };
            yield { type: 'stream_event', event: { type: 'content_block_delta', delta: { type: 'text_delta', text: 'Hello' } } };
            yield { type: 'assistant', message: { id: 'reply', content: [{ type: 'text', text: 'Hello' }, { type: 'tool_use', id: 't', name: 'mcp__hissabeth__system_get', input: {} }] } };
            yield { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 't', content: 'revision 2', is_error: false }] } };
            yield { type: 'result', subtype: 'success', is_error: false };
        })();
        (stream as unknown as { close: () => void }).close = () => {};
        return stream;
    }
    await runTurn({ ...request, provider: 'claude', session: 'old' }, new AbortController().signal, event => emitted.push(event), { query } as unknown as Sdk);
    assert.equal(options.resume, 'old');
    assert.equal(options.model, 'model-from-picker');
    assert.equal(options.strictMcpConfig, true);
    assert.deepEqual(options.tools, []);
    assert.deepEqual(options.settingSources, []);
    assert.equal((await options.canUseTool('Bash', {})).behavior, 'deny');
    assert.equal((await options.canUseTool('mcp__hissabeth__system_set', {})).behavior, 'allow');
    assert.equal(emitted.filter(event => event.kind === 'text').every(event => event.id === 'reply'), true);
    assert.equal(emitted.at(-1)!.status, 'completed');
    function failedQuery() {
        const stream = (async function* () { yield { type: 'result', subtype: 'error_during_execution', is_error: true, errors: ['Model unavailable'] }; })();
        (stream as unknown as { close: () => void }).close = () => {};
        return stream;
    }
    await assert.rejects(runTurn({ ...request, provider: 'claude' }, new AbortController().signal, () => {}, { query: failedQuery } as unknown as Sdk), /Model unavailable/);
});

test('attached reference images reach Codex as local files and Claude as base64 blocks', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'chat-images-'));
    try {
        const png = Buffer.from('89504e470d0a1a0a626f6479', 'hex');
        const path = join(dir, 'ref.png');
        writeFileSync(path, png);
        const images = [{ path, mime: 'image/png' }];
        let codexInput: unknown;
        class Codex {
            startThread() { return { runStreamed: async (input: unknown) => {
                codexInput = input;
                return { events: (async function* () { yield { type: 'turn.completed' }; })() };
            } }; }
        }
        await runTurn({ ...request, images }, new AbortController().signal, () => {}, { Codex } as unknown as Sdk);
        assert.deepEqual(codexInput, [{ type: 'text', text: request.prompt }, { type: 'local_image', path }]);
        // An image alone is sent without an empty text item.
        await runTurn({ ...request, prompt: '', images }, new AbortController().signal, () => {}, { Codex } as unknown as Sdk);
        assert.deepEqual(codexInput, [{ type: 'local_image', path }]);
        await runTurn(request, new AbortController().signal, () => {}, { Codex } as unknown as Sdk);
        assert.equal(codexInput, request.prompt);
        const messages: any[] = [];
        function query(input: any) {
            const stream = (async function* () {
                if (typeof input.prompt === 'string') messages.push(input.prompt);
                else for await (const message of input.prompt) messages.push(message);
                yield { type: 'result', subtype: 'success', is_error: false };
            })();
            (stream as unknown as { close: () => void }).close = () => {};
            return stream;
        }
        await runTurn({ ...request, provider: 'claude', images }, new AbortController().signal, () => {}, { query } as unknown as Sdk);
        assert.equal(messages.length, 1);
        assert.equal(messages[0].type, 'user');
        assert.equal(messages[0].parent_tool_use_id, null);
        assert.deepEqual(messages[0].message, { role: 'user', content: [
            { type: 'image', source: { type: 'base64', media_type: 'image/png', data: png.toString('base64') } },
            { type: 'text', text: request.prompt },
        ] });
        await runTurn({ ...request, provider: 'claude' }, new AbortController().signal, () => {}, { query } as unknown as Sdk);
        assert.equal(messages[1], request.prompt);
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
});

test('cancellation reaches both SDKs', async () => {
    const controller = new AbortController();
    let closed = false;
    function query({ options }: any) {
        const stream = (async function* () {
            controller.abort();
            assert.equal(options.abortController.signal.aborted, true);
        })();
        (stream as unknown as { close: () => void }).close = () => { closed = true; };
        return stream;
    }
    await runTurn({ ...request, provider: 'claude' }, controller.signal, () => {}, { query } as unknown as Sdk);
    assert.equal(closed, true);
});

test('tool results carry their drawing as a chat figure from either SDK', async () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>';
    const exported = { content: [{ type: 'text', text: JSON.stringify({ revision: 3, saved: true, format: 'layout3d', filename: 'hissabeth-layout3d.svg', mime: 'image/svg+xml', content: svg }) }] };
    const events: Record<string, any>[] = [];
    class Codex {
        startThread() { return { runStreamed: async () => ({ events: (async function* () {
            yield { type: 'item.started', item: { type: 'mcp_tool_call', id: 'draw', tool: 'bench_export_blueprint', status: 'in_progress', arguments: { format: 'layout3d' } } };
            yield { type: 'item.completed', item: { type: 'mcp_tool_call', id: 'draw', tool: 'bench_export_blueprint', status: 'completed', arguments: { format: 'layout3d' }, result: exported } };
            yield { type: 'turn.completed' };
        })() }) }; }
    }
    const workspace = mkdtempSync(join(tmpdir(), 'chat-figures-'));
    try {
    await runTurn({ ...request, workspace }, new AbortController().signal, event => events.push(event), { Codex } as unknown as Sdk);
    assert.equal(events[0]!.figure, undefined);
    const { file, ...shown } = events[1]!.figure;
    assert.deepEqual(shown, { mime: 'image/svg+xml', filename: 'hissabeth-layout3d.svg', bytes: svg.length, data: Buffer.from(svg).toString('base64'), revision: 3, saved: true });
    // The stored copy is named by its content hash.
    assert.match(file, /^[0-9a-f]{32}\.svg$/);
    assert.equal(readFileSync(join(workspace, 'chat-figures', file), 'utf8'), svg);
    // Claude: the MCP prefix is dropped and an artifact_fetch image arrives in the SDK's image block.
    const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
    const claude: Record<string, any>[] = [];
    function query() {
        const stream = (async function* () {
            yield { type: 'assistant', message: { id: 'm', content: [{ type: 'tool_use', id: 'f', name: 'mcp__hissabeth__artifact_fetch', input: { id: 'job' } }] } };
            yield { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 'f', is_error: false, content: [
                { type: 'text', text: JSON.stringify({ id: 'job', filename: 'hissabeth-layout3d.png', mime: 'image/png' }) },
                { type: 'image', source: { type: 'base64', media_type: 'image/png', data: png } },
            ] }] } };
            yield { type: 'result', subtype: 'success', is_error: false };
        })();
        (stream as unknown as { close: () => void }).close = () => {};
        return stream;
    }
    await runTurn({ ...request, workspace, provider: 'claude' }, new AbortController().signal, event => claude.push(event), { query } as unknown as Sdk);
    assert.equal(claude[0]!.name, 'artifact_fetch');
    assert.equal(claude[1]!.figure.filename, 'hissabeth-layout3d.png');
    assert.equal(claude[1]!.figure.data, png);
    assert.equal(claude[1]!.figure.bytes, 70);
    assert.deepEqual(readFileSync(join(workspace, 'chat-figures', claude[1]!.figure.file)), Buffer.from(png, 'base64'));
    } finally { rmSync(workspace, { recursive: true, force: true }); }
});

test('large drawings are stored, not inlined, and lab checks become an assertion summary', () => {
    const workspace = mkdtempSync(join(tmpdir(), 'chat-figures-'));
    try {
        const dir = join(workspace, 'chat-figures');
        const big = figureOf([{ type: 'image', data: 'A'.repeat(FIGURE_LIMIT + 4), mimeType: 'image/png' }])!;
        assert.equal(big.data?.length, FIGURE_LIMIT + 4);
        const stored = storeFigure(big, dir);
        assert.equal(stored.data, undefined);
        assert.equal(stored.omitted, undefined);
        assert.equal(readFileSync(join(dir, stored.file!)).length, big.bytes);
        // Small drawings keep their inline copy as well; without a store a large one is a note.
        assert.equal(storeFigure(figureOf([{ type: 'image', data: 'AAAA', mimeType: 'image/png' }])!, dir).data, 'AAAA');
        assert.deepEqual(storeFigure(big, undefined), { mime: 'image/png', filename: 'figure.png', bytes: big.bytes, omitted: 'too_large' });
        assert.equal(storeFigure(big, join(workspace, 'missing\0dir')).omitted, 'too_large');
    } finally { rmSync(workspace, { recursive: true, force: true }); }
    const report = {
        saved: false, revision: 7, readiness_score: 60, passed: false, counts: { errors: 3, warnings: 1, info: 2 },
        assertions: [
            { assertion: 'equal_arms', kind: 'opl_difference', branches: ['arm_a', 'arm_b'], at: { element: 'bs2', port: 'in' }, unit: 'mm', target: 0, tolerance: 0.01, residual: 0.42, status: 'fail' },
            { assertion: 'combiner_angle', kind: 'angle', branches: ['arm_a', 'arm_b'], at: { element: 'bs2', port: 'in' }, unit: 'mrad', target: 0, tolerance: 1, residual: null, status: 'unreachable', break: { element: 'm3', port: 'in', reason: 'clipped', detail: 'outside clear aperture' } },
            { assertion: 'parity', kind: 'parity', branches: ['arm_a', 'arm_a'], at: { element: 'cam', port: 'in' }, unit: 'count', target: 0, tolerance: 0, residual: 0, status: 'pass' },
        ],
        branches: [{ id: 'arm_b', stage_ids: [], color: '#000', paths: [], break: { element: 'm3', port: 'in', reason: 'clipped' } }, { id: 'arm_a', stage_ids: [], color: '#111', paths: [] }],
        issues: [
            { severity: 'error', code: 'assertion_failed', element_id: 'bs2', branch: null, message: 'equal_arms: residual 0.42 exceeds tolerance 0.01' },
            { severity: 'warning', code: 'mount_collision', element_id: 'm1', branch: null, message: 'm1 collides with m2' },
            { severity: 'error', code: 'clear_aperture', element_id: 'm3', branch: 'arm_b', message: '3ω beam exceeds the clear aperture of m3' },
            { severity: 'info', code: 'note', element_id: null, branch: null, message: 'ignored' },
        ],
    };
    const checks = checksOf({ content: [{ type: 'text', text: JSON.stringify(report) }] })!;
    assert.deepEqual({ ...checks, items: undefined }, { passed: false, readiness: 60, assertions: { pass: 1, fail: 1, unreachable: 1 }, errors: 3, warnings: 1, items: undefined, more: 0, revision: 7, saved: false });
    assert.deepEqual(checks.items.map(item => [item.kind, item.label, item.severity, item.element, item.branch]), [
        ['assertion', 'equal_arms', 'error', 'bs2', 'arm_a'],
        ['assertion', 'combiner_angle', 'error', 'm3', 'arm_a'],
        ['branch', 'arm_b', 'error', 'm3', 'arm_b'],
        ['issue', 'clear_aperture', 'error', 'm3', 'arm_b'],
        ['issue', 'mount_collision', 'warning', 'm1', undefined],
    ]);
    assert.match(checks.items[0]!.detail, /residual 0\.42 mm, tolerance ±0\.01/);
    assert.match(checks.items[1]!.detail, /m3:in clipped \(outside clear aperture\)/);
    // Long failure lists are cut to CHECK_ITEMS with a count of the rest.
    const many = { ...report, issues: Array.from({ length: 40 }, (_, i) => ({ severity: 'error', code: 'c' + i, element_id: 'e', branch: null, message: 'm' })) };
    const cut = checksOf(JSON.stringify(many))!;
    assert.equal(cut.items.length, CHECK_ITEMS);
    assert.equal(cut.more, 43 - CHECK_ITEMS);
    // Other tool results carry no summary.
    assert.equal(checksOf(JSON.stringify({ revision: 1, assertions: [] })), undefined);
});

test('chat figures keep only displayable, well-formed and bounded images', () => {
    const meta = (fields: object) => [{ type: 'text', text: JSON.stringify(fields) }];
    // MCP image and embedded SVG resource content (artifact_fetch over Codex).
    assert.equal(figureOf({ content: [...meta({ filename: 'a.png' }), { type: 'image', data: 'AAAA', mimeType: 'image/png' }] })?.filename, 'a.png');
    const resource = figureOf([{ type: 'resource', resource: { uri: 'artifact://j/plan.svg', mimeType: 'image/svg+xml', text: '<svg/>' } }]);
    assert.deepEqual(resource, { mime: 'image/svg+xml', filename: 'plan.svg', bytes: 6, data: Buffer.from('<svg/>').toString('base64') });
    // PDF, DXF and plain text stay in the raw result.
    assert.equal(figureOf([{ type: 'resource', resource: { uri: 'artifact://j/p.pdf', mimeType: 'application/pdf', blob: 'JVBERi0=' } }]), undefined);
    assert.equal(figureOf(meta({ filename: 'p.pdf', mime: 'application/pdf', content: 'JVBERi0=', encoding: 'base64' })), undefined);
    assert.equal(figureOf('revision 2'), undefined);
    // Data that is not base64 never reaches a data URL.
    assert.equal(figureOf([{ type: 'image', data: 'AA"><script>', mimeType: 'image/png' }]), undefined);
    assert.equal(figureOf([{ type: 'image', data: 'AAAA', mimeType: 'text/html' }]), undefined);
    // Beyond the hard cap: kept as a note without data.
    const big = figureOf([{ type: 'image', data: 'A'.repeat(FIGURE_MAX + 4), mimeType: 'image/png' }]);
    assert.equal(big?.omitted, 'too_large');
    assert.equal(big?.data, undefined);
    assert.equal(toolName('mcp__hissabeth__bench_lab_check'), 'bench_lab_check');
});
