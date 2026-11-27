import { chatGPTAccount, chatGPTModels, chatGPTTurn, type ChatGPTMessage } from './chatgpt';
import { listModels } from './agent-models';
import { Codex } from '@openai/codex-sdk';
import { query } from '@anthropic-ai/claude-agent-sdk';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';

const instructions =
    'You are the optical design assistant inside Hissabeth. The user can edit the shared project directly in the desktop app, and you edit it through Hissabeth MCP tools. ' +
    'Always read workspace_status first and read system_get before editing. During recovery, use the recovery tools before system_get. ' +
    'Use the latest revision for every write and never edit project files directly on disk. Desktop views refresh automatically after MCP saves. ' +
    'Use design_summary for quick optical inspection and optimize for numerical Damped Least Squares optimization instead of manual trial-and-error loops. ' +
    'Use tolerance_sensitivity and tolerance_monte_carlo to check manufacturability. Use history_list and history_compare to inspect changes and history_undo to revert an edit. ' +
    'For a real lens group on the bench, use kind lens_group. It references the current sequential prescription, its position locates the first vertex, and local +X maps to the prescription +Z axis. ' +
    'Use bench_trace to inspect its continued path and power. For free-space benches, use bench_catalog for hardware, bench_template for mounted layouts, and bench_auto_aim to orient optics and snap mounts to the hole grid. ' +
    'Before treating a bench as lab-ready, use bench_lab_check to review 3ω clear aperture, clamping fork access, mount collisions, and beam dump positions. Use bench_alignment_guide for installation order and bench_export_blueprint for drawings and alignment SOPs. ' +
    'Use bench_field for interference, speckle, masks, SLMs and tip-tilt: give the source field sampling and a polarization, then read a branch end or superpose every branch at one input port. ' +
    'Use bench_readout for modal cameras: give a detector a readout (ROI ports, a Hermite-Gauss, Laguerre-Gauss or custom mode basis, balanced pairs) and read per-port mode intensities and balanced quadratures; bench_template modal_hybrid is a four-port example. ' +
    'Report actual results and tool errors. Respond in the language of the user.';

export interface BridgeRequest {
    provider: 'codex' | 'claude';
    app: string;
    workspace: string;
    executable: string;
    model: string;
    prompt: string;
    session?: string | null;
}
type ChatGPTRequest = {
    provider: 'chatgpt';
    runtime: string;
    storage: string;
    model: string;
    input: ChatGPTMessage[];
    session?: string | null;
};
export type BridgeEvent = Record<string, unknown>;
interface CodexStreamItem {
    type: string;
    id: string;
    text?: string;
    tool?: string;
    status?: string;
    arguments?: unknown;
    result?: unknown;
    error?: unknown;
}

export async function runTurn(
    request: BridgeRequest,
    signal: AbortSignal,
    emit: (event: BridgeEvent) => void,
    sdk: { Codex: typeof Codex; query: typeof query } = { Codex, query },
): Promise<void> {
    const server = {
        command: request.app,
        args: ['--mcp'],
        env: { HISSABETH_DATA_DIR: request.workspace },
    };
    if (request.provider === 'codex') {
        const client = new sdk.Codex({
            codexPathOverride: request.executable,
            configOverrides: [
                `mcp_servers=${toml({ hissabeth: { ...server, enabled: true, default_tools_approval_mode: 'approve' } })}`,
            ],
            config: { features: { shell_tool: false }, developer_instructions: instructions },
        });
        const options = {
            workingDirectory: request.workspace,
            skipGitRepoCheck: true,
            sandboxMode: 'read-only' as const,
            approvalPolicy: 'never' as const,
            webSearchMode: 'disabled' as const,
            ...(request.model ? { model: request.model } : {}),
        };
        const thread = request.session
            ? client.resumeThread(request.session, options)
            : client.startThread(options);
        let complete = false;
        const { events } = await thread.runStreamed(request.prompt, { signal });
        for await (const event of events) {
            if (event.type === 'thread.started')
                emit({ kind: 'session', session: event.thread_id });
            else if (event.type === 'error' || event.type === 'turn.failed') {
                const failure = event as unknown as {
                    message?: string;
                    error?: { message: string };
                };
                throw new Error(failure.message || failure.error?.message);
            } else if (event.type.startsWith('item.')) {
                const item = (event as unknown as { item: CodexStreamItem }).item;
                if (item.type === 'agent_message')
                    emit({ kind: 'text', id: item.id, text: item.text });
                if (item.type === 'mcp_tool_call')
                    emit({
                        kind: 'tool',
                        id: item.id,
                        name: item.tool,
                        status: item.status,
                        detail: JSON.stringify(
                            { arguments: item.arguments, result: item.result, error: item.error },
                            null,
                            2,
                        ).slice(0, 24000),
                    });
            } else if (event.type === 'turn.completed') complete = true;
        }
        if (!complete && !signal.aborted)
            throw new Error('Codex stream ended before turn completion');
    } else if (request.provider === 'claude') {
        const controller = new AbortController();
        const abort = () => controller.abort();
        signal.addEventListener('abort', abort, { once: true });
        if (signal.aborted) abort();
        let complete = false;
        const text = new Map();
        let messageId = '';
        const stream = sdk.query({
            prompt: request.prompt,
            options: {
                pathToClaudeCodeExecutable: request.executable,
                cwd: request.workspace,
                mcpServers: { hissabeth: server },
                strictMcpConfig: true,
                settingSources: [],
                tools: [],
                permissionMode: 'default',
                canUseTool: async (name, input) =>
                    name.startsWith('mcp__hissabeth__')
                        ? { behavior: 'allow', updatedInput: input }
                        : { behavior: 'deny', message: 'Use Hissabeth optical tools.' },
                systemPrompt: instructions,
                includePartialMessages: true,
                abortController: controller,
                ...(request.session ? { resume: request.session } : {}),
                ...(request.model ? { model: request.model } : {}),
            },
        });
        try {
            for await (const message of stream) {
                if (message.type === 'system' && message.subtype === 'init') {
                    emit({ kind: 'session', session: message.session_id });
                    const failed = message.mcp_servers?.filter(
                        (connection) => connection.status !== 'connected',
                    );
                    if (failed?.length)
                        throw new Error(
                            `MCP connection failed: ${failed.map((connection) => connection.name).join(', ')}`,
                        );
                }
                if (message.type === 'stream_event') {
                    const event = message.event;
                    if (event.type === 'message_start') messageId = event.message.id;
                    if (event.type === 'content_block_delta' && event.delta.type === 'text_delta') {
                        text.set(messageId, (text.get(messageId) || '') + event.delta.text);
                        emit({ kind: 'text', id: messageId, text: text.get(messageId) });
                    }
                }
                if (message.type === 'assistant') {
                    const content = message.message.content;
                    const body = content
                        .filter((block) => block.type === 'text')
                        .map((block) => block.text)
                        .join('\n');
                    if (body) emit({ kind: 'text', id: message.message.id, text: body });
                    for (const block of content)
                        if (block.type === 'tool_use') {
                            emit({
                                kind: 'tool',
                                id: block.id,
                                name: block.name,
                                status: 'in_progress',
                                detail: JSON.stringify(block.input, null, 2).slice(0, 24000),
                            });
                        }
                }
                if (message.type === 'user' && Array.isArray(message.message.content)) {
                    for (const block of message.message.content)
                        if (block.type === 'tool_result') {
                            emit({
                                kind: 'tool',
                                id: block.tool_use_id,
                                status: block.is_error ? 'failed' : 'completed',
                                detail: JSON.stringify(block.content, null, 2).slice(0, 24000),
                            });
                        }
                }
                if (message.type === 'result') {
                    const result = message as unknown as {
                        is_error: boolean;
                        subtype: string;
                        errors?: string[];
                    };
                    if (result.is_error || result.subtype !== 'success')
                        throw new Error(result.errors?.join('\n') || result.subtype);
                    complete = true;
                }
            }
            if (!complete && !signal.aborted)
                throw new Error('Claude stream ended before turn completion');
        } finally {
            signal.removeEventListener('abort', abort);
            stream.close();
        }
    } else throw new Error('Unknown provider');
}

// Inline TOML replaces the server table for this invocation, including inherited aliases.
function toml(value: unknown): string {
    if (Array.isArray(value)) return `[${value.map(toml).join(',')}]`;
    if (value && typeof value === 'object')
        return `{${Object.entries(value)
            .map(([key, item]) => `${JSON.stringify(key)}=${toml(item)}`)
            .join(',')}}`;
    return JSON.stringify(value) as string;
}

const emitOutput = (event: BridgeEvent) => process.stdout.write(`${JSON.stringify(event)}\n`);
async function main() {
    const input = createInterface({ input: process.stdin });
    const controller = new AbortController();
    let started = false;
    input.on('close', () => controller.abort());
    input.on('line', (line) => {
        let request: (BridgeRequest | ChatGPTRequest) & { kind?: string };
        try {
            request = JSON.parse(line) as typeof request;
        } catch {
            emitOutput({ kind: 'error', text: 'Invalid bridge request' });
            return;
        }
        if (request.kind === 'cancel') {
            controller.abort();
            return;
        }
        if (started) return;
        started = true;
        const turn =
            request.provider === 'chatgpt'
                ? chatGPTTurn(request, controller.signal, emitOutput)
                : runTurn(request, controller.signal, emitOutput);
        void turn
            .then(() => emitOutput({ kind: controller.signal.aborted ? 'cancelled' : 'done' }))
            .catch((error) =>
                emitOutput({
                    kind: controller.signal.aborted ? 'cancelled' : 'error',
                    text: error.message,
                }),
            )
            .finally(() => {
                input.close();
                process.stdin.destroy();
            });
    });
}
function printResult(result: Promise<unknown>): void {
    void result
        .then((value) => process.stdout.write(`${JSON.stringify(value)}\n`))
        .catch((error: Error) => {
            process.stderr.write(`${error.message}\n`);
            process.exitCode = 1;
        });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
    if (process.argv[2] === '--chatgpt') {
        const [, , , action, runtime, storage] = process.argv;
        printResult(
            action === 'models'
                ? chatGPTModels(runtime, storage)
                : chatGPTAccount(runtime, storage, action),
        );
    } else if (process.argv[2] === '--models') {
        printResult(
            listModels({
                provider: process.argv[3],
                executable: process.argv[4],
                workspace: process.argv[5],
                version: process.argv[6],
            }),
        );
    } else void main();
}
