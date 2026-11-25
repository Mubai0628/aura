import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { DEVELOPMENT_RENDERER_URL } from '../apps/desktop/shared/development-renderer.ts';
import { moveRequestedData } from './dev/move-data.mjs';
import { rendererPortInUse } from './dev/renderer.mjs';
import { closeAuraWindows, killTree, runDevSession } from './dev/session.mjs';

const root = resolve(import.meta.dirname, '..');
const profile = resolve(root, 'target/v2-development');
const option = process.argv.slice(2).join(' ');
const keepDesktopProfile = option === '--desktop-profile';
const env = {
  ...process.env,
  // The application profile is isolated; the build SDK stays in the host cache.
  AURA_NODE_API_SDK:
    process.env.AURA_NODE_API_SDK ??
    resolve(process.env.LOCALAPPDATA ?? '', 'node-gyp', 'Cache', process.versions.node),
  ...(keepDesktopProfile
    ? {}
    : {
        APPDATA: resolve(profile, 'roaming'),
        LOCALAPPDATA: resolve(profile, 'local'),
      }),
  // Six build jobs and six test threads.
  CARGO_BUILD_JOBS: '6',
  RUST_TEST_THREADS: '6',
};
if (option === '--print-profile') {
  console.log(JSON.stringify({ profile, APPDATA: env.APPDATA, LOCALAPPDATA: env.LOCALAPPDATA }));
} else {
  if (option && !keepDesktopProfile) throw new Error('dev-v2 accepts no application arguments');
  const pnpm = process.env.npm_execpath;
  if (!pnpm) throw new Error('Start V2 development with corepack pnpm dev');
  if (!keepDesktopProfile) {
    await Promise.all([
      mkdir(env.APPDATA, { recursive: true }),
      mkdir(env.LOCALAPPDATA, { recursive: true }),
    ]);
    console.log(`V2 development data: ${profile}`);
  }
  const desktop = (script, options) =>
    spawn(process.execPath, [pnpm, '--filter', '@aura/desktop', script], {
      cwd: root,
      env,
      windowsHide: true,
      ...options,
    });
  // The build in the terminal's own group: a Ctrl+C here just stops the build.
  const prepared = await new Promise((done) => {
    const build = desktop('dev:prepare', { stdio: 'inherit' });
    build.on('error', (error) => {
      console.error(error.message);
      done(1);
    });
    build.on('exit', (code) => done(code ?? 1));
  });
  if (prepared !== 0) {
    process.exitCode = prepared;
  } else {
    if (!keepDesktopProfile) await moveRequestedData({ root, env });
    const portInUse = await rendererPortInUse();
    delete env.AURA_RENDERER_URL;
    delete env.AURA_DEV_RENDERER_INSTANCE;
    if (portInUse) {
      console.log('开发端口 5175 已被占用，Aura 将从本地构建打开。');
    } else {
      env.AURA_RENDERER_URL = DEVELOPMENT_RENDERER_URL;
      env.AURA_DEV_RENDERER_INSTANCE = randomBytes(16).toString('hex');
    }
    // The renderer and Electron in a process group of their own (see dev/session.mjs),
    // their output still shown here, each line marked with where it came from.
    const shown = (name, child) => {
      for (const stream of [child.stdout, child.stderr]) {
        createInterface({ input: stream }).on('line', (line) =>
          process.stdout.write(`[${name}] ${line}\n`),
        );
      }
      return child;
    };
    const grouped = { detached: true, stdio: ['ignore', 'pipe', 'pipe'] };
    runDevSession({
      spawnRenderer: portInUse
        ? undefined
        : () => shown('renderer', desktop('dev:renderer', grouped)),
      spawnElectron: () => shown('electron', desktop('dev:electron', grouped)),
      closeAuraWindows: () => closeAuraWindows(root),
      killTree,
      onExit: (code) => process.exit(code),
    });
  }
}
