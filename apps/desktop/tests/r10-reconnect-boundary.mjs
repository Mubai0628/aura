// Same-process start, injected pipe disconnect, and reconnect on the latest
// Electron main and debug Core. Synthetic app data only.
import { _electron as electron } from '@playwright/test';
import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { namespaceObjectHash } from './history-pages.mjs';

const desktop = resolve(import.meta.dirname, '..');
const repo = resolve(desktop, '../..');
const stamp = new Date().toISOString().replaceAll(/[:.]/gu, '-');
const output = resolve(repo, 'target', 'r10-reconnect-boundary', `${stamp}-${process.pid}`);
const localAppData = join(output, 'Local');
await mkdir(join(output, 'Electron'), { recursive: true });
await mkdir(join(output, 'Roaming'), { recursive: true });
await mkdir(localAppData, { recursive: true });

const manifestPath = join(repo, 'target/aura-dev-runtime/component-manifest.json');
const corePath = join(repo, 'target/debug/aura-core.exe');
const mainPath = join(desktop, 'dist/electron-main/main.js');
const supervisorPath = join(desktop, 'dist/electron-main/core-supervisor.js');
const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
const [coreFile, mainFile, supervisorFile] = await Promise.all([
  stat(corePath),
  stat(mainPath),
  stat(supervisorPath),
]);
const namespace = namespaceObjectHash(manifest.bundle_namespace);
const report = {
  output,
  namespace,
  core: { path: corePath, mtimeMs: coreFile.mtimeMs, size: coreFile.size },
  electronMain: { path: mainPath, mtimeMs: mainFile.mtimeMs, size: mainFile.size },
  coreSupervisor: {
    path: supervisorPath,
    mtimeMs: supervisorFile.mtimeMs,
    size: supervisorFile.size,
  },
  passed: false,
};
console.log(`Evidence: ${output}`);
console.log(`Core: ${corePath} mtime=${coreFile.mtime.toISOString()} size=${coreFile.size}`);
console.log(`Main: ${mainPath} mtime=${mainFile.mtime.toISOString()} size=${mainFile.size}`);
console.log(
  `Supervisor: ${supervisorPath} mtime=${supervisorFile.mtime.toISOString()} size=${supervisorFile.size}`,
);

async function poll(read, ready, timeoutMs, label) {
  const started = Date.now();
  let last;
  while (Date.now() - started < timeoutMs) {
    last = await read();
    if (ready(last)) return last;
    await new Promise((done) => setTimeout(done, 200));
  }
  throw new Error(`${label} timed out: ${JSON.stringify(last)}`);
}

let app;
let failure;
try {
  const env = {
    ...process.env,
    APPDATA: join(output, 'Roaming'),
    AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
    AURA_E2E: '1',
    AURA_E2E_CORE_MODE: 'disconnect-after-ready',
    LOCALAPPDATA: localAppData,
  };
  delete env.AURA_RENDERER_URL;
  delete env.AURA_IPC_TRACE_FILE;
  app = await electron.launch({
    args: [`--user-data-dir=${join(output, 'Electron')}`, desktop],
    cwd: desktop,
    env,
    timeout: 60_000,
  });
  const page = await app.firstWindow();
  await page.waitForLoadState('domcontentloaded');
  await page.evaluate(() => {
    const renderer = globalThis;
    renderer.auraCoreTransitions = [];
    renderer.aura.v1.onCoreHealth((change) => {
      renderer.auraCoreTransitions.push({
        runtimeInstanceId: change.runtime_instance_id,
        state: change.payload.state,
      });
    });
  });
  await poll(
    () =>
      page.evaluate(async () => {
        const result = await globalThis.aura.v1.getCoreHealth();
        return result?.ok ? result.value.payload.state : result?.error?.message;
      }),
    (state) => state === 'ready',
    90_000,
    'first Core ready',
  );
  const cycle = await poll(
    () =>
      page.evaluate(() => {
        const transitions = globalThis.auraCoreTransitions ?? [];
        const firstReady = transitions.findIndex((item) => item.state === 'ready');
        const restarting = transitions.findIndex(
          (item, index) => index > firstReady && item.state === 'restarting',
        );
        const readyAgain = transitions.findIndex(
          (item, index) => index > restarting && item.state === 'ready',
        );
        return {
          firstReady,
          restarting,
          readyAgain,
          sameRuntime:
            firstReady >= 0 &&
            restarting > firstReady &&
            readyAgain > restarting &&
            transitions[firstReady].runtimeInstanceId ===
              transitions[restarting].runtimeInstanceId &&
            transitions[restarting].runtimeInstanceId === transitions[readyAgain].runtimeInstanceId,
          transitions,
        };
      }),
    (value) => value.sameRuntime === true,
    30_000,
    'same-process reconnect',
  );
  report.transitions = cycle.transitions.map((item) => item.state);
  report.runtimeInstanceId = cycle.transitions[cycle.firstReady].runtimeInstanceId;
  const logPath = join(localAppData, 'Aura', 'namespaces', namespace, 'logs', 'main.jsonl');
  const log = await readFile(logPath, 'utf8');
  const events = [...log.matchAll(/"event":"([^"]+)"/gu)].map((match) => match[1]);
  report.events = events;
  const spawned = events.filter((event) => event === 'core_spawned').length;
  const reconnected = events.filter((event) => event === 'core_transport_reconnected').length;
  const exited = events.filter((event) => event === 'core_exited').length;
  if (spawned !== 1) throw new Error(`expected one core_spawned, saw ${spawned}`);
  if (reconnected !== 1)
    throw new Error(`expected one core_transport_reconnected, saw ${reconnected}`);
  if (exited !== 0) throw new Error('Core exited during same-process reconnect');
  if (!log.includes('"connectionEpoch":2'))
    throw new Error('reconnect did not record connection epoch 2');
  report.passed = true;
  console.log(
    `Reconnect: runtime=${report.runtimeInstanceId} states=${report.transitions.join(' -> ')}`,
  );
} catch (error) {
  failure = error;
  report.error = String(error?.stack ?? error).slice(0, 2000);
  console.error(report.error);
} finally {
  if (app) await app.close();
  await writeFile(join(output, 'reconnect-boundary.json'), JSON.stringify(report, null, 2));
}
if (failure || !report.passed) process.exitCode = 1;
