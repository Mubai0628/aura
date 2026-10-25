import { spawn } from 'node:child_process';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { closeAuraWindows, killTree, runDevSession } from './dev/session.mjs';

const root = resolve(import.meta.dirname, '..');
const profile = resolve(root, 'target/v2-development');
const env = {
  ...process.env,
  // The application profile is isolated; the build SDK stays in the host cache.
  AURA_NODE_API_SDK:
    process.env.AURA_NODE_API_SDK ??
    resolve(process.env.LOCALAPPDATA ?? '', 'node-gyp', 'Cache', process.versions.node),
  APPDATA: resolve(profile, 'roaming'),
  LOCALAPPDATA: resolve(profile, 'local'),
  // Six build jobs and six test threads.
  CARGO_BUILD_JOBS: '6',
  RUST_TEST_THREADS: '6',
};
if (process.argv.slice(2).join(' ') === '--print-profile') {
  console.log(JSON.stringify({ profile, APPDATA: env.APPDATA, LOCALAPPDATA: env.LOCALAPPDATA }));
} else {
  if (process.argv.length !== 2) throw new Error('dev-v2 accepts no application arguments');
  const pnpm = process.env.npm_execpath;
  if (!pnpm) throw new Error('Start V2 development with corepack pnpm dev');
  await Promise.all([
    mkdir(env.APPDATA, { recursive: true }),
    mkdir(env.LOCALAPPDATA, { recursive: true }),
  ]);
  console.log(`V2 development data: ${profile}`);
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
      spawnRenderer: () => shown('renderer', desktop('dev:renderer', grouped)),
      spawnElectron: () => shown('electron', desktop('dev:electron', grouped)),
      closeAuraWindows: () => closeAuraWindows(root),
      killTree,
      onExit: (code) => process.exit(code),
    });
  }
}
