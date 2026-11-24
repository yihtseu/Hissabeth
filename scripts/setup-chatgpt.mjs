import { spawnSync } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

const configRoot =
    process.platform === 'win32'
        ? process.env.APPDATA || join(homedir(), 'AppData', 'Roaming')
        : process.platform === 'darwin'
          ? join(homedir(), 'Library', 'Application Support')
          : process.env.XDG_CONFIG_HOME || join(homedir(), '.config');
const destination = resolve(process.argv[2] || join(configRoot, 'cn.ac.yizhou.hissabeth', 'siwc'));
const parent = dirname(destination);
await mkdir(parent, { recursive: true });
const stage = await mkdtemp(join(parent, '.siwc-'));
const checkout = join(stage, 'upstream');
const runtime = join(stage, 'runtime');
const revision = '0a36fefeb913055c8c7a1a29b63d82b96b2e841a';

function run(program, args, cwd) {
    const result = spawnSync(program, args, { cwd, stdio: 'inherit' });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`${program} failed`);
}

try {
    run('git', ['init', checkout], stage);
    run(
        'git',
        [
            'fetch',
            '--depth=1',
            'https://github.com/openai/sign-in-with-chatgpt-devkit.git',
            revision,
        ],
        checkout,
    );
    run('git', ['checkout', '--detach', 'FETCH_HEAD'], checkout);
    const local = join(checkout, 'packages', 'local');
    const { dependencies } = JSON.parse(await readFile(join(local, 'package.json'), 'utf8'));
    await mkdir(runtime);
    await cp(join(local, 'src'), join(runtime, 'src'), { recursive: true });
    await cp(join(checkout, 'LICENSE'), join(runtime, 'LICENSE'));
    await cp(join(checkout, 'THIRD_PARTY_NOTICES.md'), join(runtime, 'THIRD_PARTY_NOTICES.md'));
    await writeFile(
        join(runtime, 'package.json'),
        JSON.stringify({
            private: true,
            type: 'module',
            dependencies: { ...dependencies, '@napi-rs/keyring': '2.1.2' },
        }),
    );
    run('bun', ['install', '--ignore-scripts'], runtime);
    run(
        'bun',
        ['build', 'src/index.ts', '--target=node', '--packages=external', '--outfile=sdk.mjs'],
        runtime,
    );
    await rename(runtime, destination);
    process.stdout.write(`Installed ChatGPT DevKit in ${destination}\n`);
} finally {
    await rm(stage, { recursive: true, force: true });
}
