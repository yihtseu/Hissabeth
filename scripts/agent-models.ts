import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { once } from 'node:events';
import { query, type SDKUserMessage } from '@anthropic-ai/claude-agent-sdk';

export interface AgentModelInfo {
    id: string;
    label: string;
    description: string;
}
export interface ModelsRequest {
    provider: string;
    executable: string;
    workspace: string;
    version: string;
}
interface CodexModelPage {
    data: { model: string; displayName: string; description?: string; hidden?: boolean }[];
    nextCursor?: string;
}

export async function listModels(
    request: ModelsRequest,
    sdk: { query: typeof query; spawn: typeof spawn } = { query, spawn },
): Promise<AgentModelInfo[]> {
    const controller = new AbortController();
    const timeout = setTimeout(
        () => controller.abort(new Error('Model discovery timed out')),
        15000,
    );
    try {
        let models: AgentModelInfo[];
        if (request.provider === 'claude') {
            // Keep input open without submitting a user turn. supportedModels reads initialization metadata.
            const input = {
                [Symbol.asyncIterator]() {
                    return {
                        async next() {
                            await new Promise<void>((resolve) => {
                                if (controller.signal.aborted) resolve();
                                else
                                    controller.signal.addEventListener('abort', () => resolve(), {
                                        once: true,
                                    });
                            });
                            return { done: true, value: undefined };
                        },
                    };
                },
            };
            const session = sdk.query({
                prompt: input as unknown as AsyncIterable<SDKUserMessage>,
                options: {
                    pathToClaudeCodeExecutable: request.executable,
                    cwd: request.workspace,
                    mcpServers: {},
                    strictMcpConfig: true,
                    settingSources: [],
                    tools: [],
                    persistSession: false,
                    abortController: controller,
                },
            });
            try {
                models = (await session.supportedModels()).map((model) => ({
                    id: model.value,
                    label: model.displayName,
                    description: model.description,
                }));
            } finally {
                session.close();
            }
        } else if (request.provider === 'codex') {
            const child = sdk.spawn(request.executable, ['-c', 'mcp_servers={}', 'app-server'], {
                cwd: request.workspace,
                stdio: ['pipe', 'pipe', 'pipe'],
            });
            const closed = once(child, 'close').catch(() => {});
            const lines = createInterface({ input: child.stdout! });
            const pending = new Map<
                number,
                { resolve: (value: unknown) => void; reject: (error: unknown) => void }
            >();
            let nextId = 0;
            let diagnostic = '';
            child.stderr.on('data', (bytes) => {
                diagnostic = (diagnostic + bytes.toString()).slice(-4096);
            });
            const rejectAll = (error: unknown) => {
                for (const { reject } of pending.values()) reject(error);
                pending.clear();
            };
            const abort = () => {
                rejectAll(controller.signal.reason);
                child.kill();
            };
            controller.signal.addEventListener('abort', abort, { once: true });
            child.on('error', rejectAll);
            child.on('close', () =>
                rejectAll(new Error(`Model discovery exited. ${diagnostic.trim()}`)),
            );
            lines.on('line', (line) => {
                try {
                    const response = JSON.parse(line) as {
                        id?: number;
                        error?: { message: string };
                        result?: unknown;
                    };
                    const waiting = pending.get(response.id ?? -1);
                    if (!waiting) return;
                    pending.delete(response.id!);
                    if (response.error) waiting.reject(new Error(response.error.message));
                    else waiting.resolve(response.result);
                } catch (error) {
                    rejectAll(error);
                }
            });
            const call = <T>(method: string, params: Record<string, unknown>) =>
                new Promise<T>((resolve, reject) => {
                    const id = ++nextId;
                    pending.set(id, { resolve: resolve as (value: unknown) => void, reject });
                    child.stdin!.write(`${JSON.stringify({ id, method, params })}\n`, (error) => {
                        if (error) {
                            pending.delete(id);
                            reject(error);
                        }
                    });
                });
            child.stdin!.on('error', rejectAll);
            try {
                await call<unknown>('initialize', {
                    clientInfo: { name: 'hissabeth', title: 'Hissabeth', version: request.version },
                });
                child.stdin!.write(`${JSON.stringify({ method: 'initialized', params: {} })}\n`);
                models = [];
                async function readPage(cursor?: string): Promise<void> {
                    const page = await call<CodexModelPage>('model/list', {
                        includeHidden: false,
                        limit: 100,
                        ...(cursor ? { cursor } : {}),
                    });
                    models.push(
                        ...page.data
                            .filter((model) => !model.hidden)
                            .map((model) => ({
                                id: model.model,
                                label: model.displayName,
                                description: model.description || '',
                            })),
                    );
                    if (page.nextCursor) await readPage(page.nextCursor);
                }
                await readPage();
            } finally {
                controller.signal.removeEventListener('abort', abort);
                lines.close();
                child.stdin!.destroy();
                child.kill();
                await closed;
            }
        } else throw new Error('Unknown provider');
        return models.filter(
            (model, index) =>
                model.id && models.findIndex((other) => other.id === model.id) === index,
        );
    } finally {
        clearTimeout(timeout);
        controller.abort();
    }
}
