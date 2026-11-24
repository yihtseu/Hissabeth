// Bridge behavior against SDK fixtures: no real agent processes are launched.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runTurn, type BridgeRequest } from '../../scripts/agent-bridge';

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
