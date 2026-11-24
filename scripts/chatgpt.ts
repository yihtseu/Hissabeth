import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto';
import { access } from 'node:fs/promises';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import type { BridgeEvent } from './agent-bridge';
import type { AgentModelInfo } from './agent-models';

type Session = {
    status: 'disconnected' | 'connected' | 'reauth_required';
    sharing: boolean;
    profileId?: string;
    identity?: { name?: string; email?: string };
    error?: { message: string };
};

interface CredentialEncryption {
    id: string;
    isAvailable(): Promise<boolean>;
    encrypt(value: string): Promise<Uint8Array>;
    decrypt(value: Uint8Array): Promise<string>;
}

export interface ChatGPTMessage {
    role: 'user' | 'assistant';
    content: string;
}

interface ChatGPTClient {
    getSession(): Promise<Session>;
    signIn(options: { signal: AbortSignal; reconsent: boolean }): Promise<Session>;
    disconnect(): Promise<void>;
    listModels(options: { signal: AbortSignal }): Promise<{ slug: string; displayName: string }[]>;
    streamResponse(options: {
        model: string;
        input: ChatGPTMessage[];
        instructions: string;
        signal: AbortSignal;
        onDelta(delta: string): void;
    }): Promise<{ text: string }>;
}

interface KeyringEntry {
    getPassword(): Promise<string | null>;
    setPassword(value: string): Promise<void>;
}

interface ChatGPTModule {
    createChatGPT(config: {
        appName: string;
        appId: string;
        redirectPort: number;
        storageDir: string;
        sendHostId: boolean;
        credentialEncryption: CredentialEncryption;
    }): ChatGPTClient;
}

function readKey(encoded: string | null | undefined): Buffer {
    const key = Buffer.from(encoded ?? '', 'base64');
    if (key.length !== 32) throw new Error('ChatGPT encryption key is unavailable');
    return key;
}

async function openClient(runtime: string, storageDir: string): Promise<ChatGPTClient> {
    const sdkPath = join(runtime, 'sdk.mjs');
    await access(sdkPath).catch(() => {
        throw new Error('Install the ChatGPT DevKit with bun run setup:chatgpt.');
    });
    const require = createRequire(pathToFileURL(sdkPath));
    const { AsyncEntry } = require('@napi-rs/keyring') as {
        AsyncEntry: new (
            service: string,
            account: string,
            options: { linux: { store: 'secret-service' } },
        ) => KeyringEntry;
    };
    const entry = () =>
        new AsyncEntry('cn.ac.yizhou.hissabeth', 'chatgpt-encryption', {
            linux: { store: 'secret-service' },
        });
    const credentialEncryption: CredentialEncryption = {
        id: 'hissabeth-keyring-aes256gcm',
        async isAvailable() {
            await entry().getPassword();
            return true;
        },
        async encrypt(value) {
            const item = entry();
            let stored = await item.getPassword();
            if (stored == null) {
                stored = randomBytes(32).toString('base64');
                await item.setPassword(stored);
            }
            const iv = randomBytes(12);
            const cipher = createCipheriv('aes-256-gcm', readKey(stored), iv);
            const ciphertext = Buffer.concat([cipher.update(value, 'utf8'), cipher.final()]);
            return Buffer.concat([iv, cipher.getAuthTag(), ciphertext]);
        },
        async decrypt(value) {
            const bytes = Buffer.from(value);
            const decipher = createDecipheriv(
                'aes-256-gcm',
                readKey(await entry().getPassword()),
                bytes.subarray(0, 12),
            );
            decipher.setAuthTag(bytes.subarray(12, 28));
            return Buffer.concat([decipher.update(bytes.subarray(28)), decipher.final()]).toString(
                'utf8',
            );
        },
    };
    const { createChatGPT } = (await import(pathToFileURL(sdkPath).href)) as ChatGPTModule;
    return createChatGPT({
        appName: 'Hissabeth',
        appId: 'cn.ac.yizhou.hissabeth',
        redirectPort: 0,
        storageDir,
        sendHostId: true,
        credentialEncryption,
    });
}

export async function chatGPTAccount(
    runtime: string,
    storage: string,
    action: string,
): Promise<Session> {
    const client = await openClient(runtime, storage);
    if (action === 'sign-in' || action === 'enable-plan') {
        await client.signIn({
            signal: AbortSignal.timeout(180_000),
            reconsent: action === 'enable-plan',
        });
    }
    if (action === 'disconnect') await client.disconnect();
    return client.getSession();
}

export async function chatGPTModels(runtime: string, storage: string): Promise<AgentModelInfo[]> {
    const client = await openClient(runtime, storage);
    const models = await client.listModels({ signal: AbortSignal.timeout(15_000) });
    return models.map(({ slug, displayName }) => ({
        id: slug,
        label: displayName,
        description: '',
    }));
}

export async function chatGPTTurn(
    request: {
        runtime: string;
        storage: string;
        model: string;
        input: ChatGPTMessage[];
        session?: string | null;
    },
    signal: AbortSignal,
    emit: (event: BridgeEvent) => void,
): Promise<void> {
    const client = await openClient(request.runtime, request.storage);
    const session = await client.getSession();
    if (session.error) throw new Error(session.error.message);
    if (!session.sharing || !session.profileId)
        throw new Error('Connect ChatGPT and enable ChatGPT plan usage first.');
    if (request.session && request.session !== session.profileId)
        throw new Error('Start a new conversation to use this ChatGPT account.');
    emit({ kind: 'session', session: session.profileId });
    let text = '';
    await client.streamResponse({
        model: request.model,
        input: request.input,
        instructions:
            'You are a conversational assistant in Hissabeth, an optical design workbench. You can answer optical design questions using the information the user provides. You have no access to the workspace or its optical tools. Respond in the language of the user.',
        signal,
        onDelta(delta) {
            text += delta;
            emit({ kind: 'text', id: 'chatgpt-response', text });
        },
    });
}
