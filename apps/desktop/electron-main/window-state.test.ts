// @vitest-environment node
import type { BrowserWindow, Display, Rectangle } from 'electron';
import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readdir, readFile, rm, utimes, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, expect, test } from 'vitest';
import { WindowStateStore } from './window-state.js';

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

const namespaceRoot = async (): Promise<string> => {
  const root = await mkdtemp(join(tmpdir(), 'aura-window-state-'));
  roots.push(root);
  return root;
};

const desktopEntries = async (root: string): Promise<string[]> =>
  (await readdir(join(root, 'desktop'))).sort();

const savedX = async (root: string): Promise<number> => {
  const text = await readFile(join(root, 'desktop', 'window-state.v1.json'), 'utf8');
  return (JSON.parse(text) as { normal_bounds_dip: Rectangle }).normal_bounds_dip.x;
};

/** The id of a process that has already ended. */
const endedPid = (): number => spawnSync(process.execPath, ['-e', ''], { windowsHide: true }).pid;

const noLog = (): void => undefined;

const window = (x: number) =>
  ({
    getNormalBounds: () => ({ height: 720, width: 1100, x, y: 80 }),
    isFullScreen: () => false,
    isMaximized: () => false,
    isMinimized: () => false,
  }) as unknown as BrowserWindow;
const display = { id: 1, scaleFactor: 1 } as Display;

/** A folder where the saved file belongs: every write fails at its rename while it is there. */
const blockSavedFile = async (root: string): Promise<void> => {
  const saved = join(root, 'desktop', 'window-state.v1.json');
  await rm(saved, { force: true });
  await mkdir(join(saved, 'held'), { recursive: true });
};
const unblockSavedFile = (root: string): Promise<void> =>
  rm(join(root, 'desktop', 'window-state.v1.json'), { recursive: true });

const STORE_URL = new URL('./window-state.ts', import.meta.url).href;

// A Main process in miniature: it saves x=100, quits (or keeps running), and captures x=200. Then
// it ends the way Electron does after quit, without waiting for writes: at the first moment a write
// has created its temporary file, or once nothing is left to write.
const MAIN_PROCESS = [
  "import { existsSync } from 'node:fs';",
  "import { join } from 'node:path';",
  'const [storeUrl, root, shutdown] = process.argv.slice(1);',
  'const { WindowStateStore } = await import(storeUrl);',
  'const window = (x) => ({',
  '  getNormalBounds: () => ({ height: 720, width: 1100, x, y: 80 }),',
  '  isFullScreen: () => false,',
  '  isMaximized: () => false,',
  '  isMinimized: () => false,',
  '});',
  'const display = { id: 1, scaleFactor: 1 };',
  'const store = new WindowStateStore(root, () => {});',
  'store.capture(window(100), display);',
  "await (shutdown === 'quit' ? store.close() : store.flush());",
  'store.capture(window(200), display);',
  "const temporary = join(root, 'desktop', 'window-state.v1.json.' + process.pid + '.tmp');",
  'let settled = false;',
  'void store.flush().then(() => { settled = true; });',
  'const exit = () => (existsSync(temporary) || settled ? process.exit(0) : setImmediate(exit));',
  'exit();',
].join('\n');

/** Runs MAIN_PROCESS in its own Node process, which strips the store's types; returns its id. */
const runMainProcess = (root: string, shutdown: 'quit' | 'running'): number => {
  const run = spawnSync(
    process.execPath,
    ['--no-warnings', '--input-type=module', '-e', MAIN_PROCESS, STORE_URL, root, shutdown],
    { encoding: 'utf8', timeout: 10_000, windowsHide: true },
  );
  expect(run.error).toBeUndefined();
  expect(run.stderr).toBe('');
  expect(run.status).toBe(0);
  return run.pid;
};

test('a capture after quit has started leaves no temporary file for the exit to cut off', async () => {
  const root = await namespaceRoot();
  // The second capture is the window's close event in Electron's second quit pass, which comes
  // after Main's quit handler has stopped waiting for writes.
  runMainProcess(root, 'quit');

  expect(await desktopEntries(root)).toEqual(['window-state.v1.json']);
  expect(await savedX(root)).toBe(100);
});

test('a temporary file left by a process that ended mid-write is removed at the next start', async () => {
  const root = await namespaceRoot();
  const pid = runMainProcess(root, 'running');
  const leftover = `window-state.v1.json.${String(pid)}.tmp`;
  expect(await desktopEntries(root)).toEqual(['window-state.v1.json', leftover]);

  await new WindowStateStore(root, noLog).flush();

  expect(await desktopEntries(root)).toEqual(['window-state.v1.json']);
  expect(await savedX(root)).toBe(100);
});

test('the start removes only stale temporary files of this store, nothing else', async () => {
  const root = await namespaceRoot();
  const desktop = join(root, 'desktop');
  const [ended, alsoEnded] = [endedPid(), endedPid()];
  const old = new Date(Date.now() - 120_000);
  const plant = async (path: string) => {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, '{}');
    await utimes(path, old, old);
  };
  const stale = `window-state.v1.json.${String(ended)}.tmp`;
  const notTemporary = [
    'window-state.v1.json',
    'window-state.v1.json.tmp',
    `window-state.v1.json.${String(ended)}.tmp.bak`,
    `window-state.v1.json.x${String(ended)}.tmp`,
    `window-state.v2.json.${String(ended)}.tmp`,
    `notes.${String(ended)}.tmp`,
  ];
  const folder = `window-state.v1.json.${String(alsoEnded)}.tmp`;
  for (const name of [stale, ...notTemporary]) await plant(join(desktop, name));
  await plant(join(desktop, folder, 'inside.json'));
  await plant(join(desktop, 'temp', stale));
  await plant(join(root, stale));
  // A recent one of a running process may be another Aura's write that is about to be renamed.
  const inFlight = `window-state.v1.json.${String(process.pid)}.tmp`;
  await writeFile(join(desktop, inFlight), '{}');

  await new WindowStateStore(root, noLog).flush();

  expect(await desktopEntries(root)).toEqual([...notTemporary, folder, inFlight, 'temp'].sort());
  expect(await readdir(join(desktop, folder))).toEqual(['inside.json']);
  expect(await readdir(join(desktop, 'temp'))).toEqual([stale]);
  expect((await readdir(root)).sort()).toEqual(['desktop', stale]);
});

test('an old temporary file is stale even when a running process holds its id', async () => {
  const root = await namespaceRoot();
  const path = join(root, 'desktop', `window-state.v1.json.${String(process.pid)}.tmp`);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, '{}');
  const old = new Date(Date.now() - 120_000);
  await utimes(path, old, old);

  await new WindowStateStore(root, noLog).flush();

  expect(await desktopEntries(root)).toEqual([]);
});

test('a failed write leaves no temporary file and the next capture still saves', async () => {
  const root = await namespaceRoot();
  await blockSavedFile(root);
  const logged: string[] = [];
  const store = new WindowStateStore(root, (event) => void logged.push(event));

  store.capture(window(100), display);
  await store.flush();
  expect(logged).toEqual(['window_state.write_failed']);
  expect(await desktopEntries(root)).toEqual(['window-state.v1.json']);

  await unblockSavedFile(root);
  store.capture(window(200), display);
  await store.flush();
  expect(await savedX(root)).toBe(200);
  expect(await desktopEntries(root)).toEqual(['window-state.v1.json']);
});

test('quit finishes after a failed write, and each run of failures is logged once', async () => {
  const root = await namespaceRoot();
  await blockSavedFile(root);
  const logged: string[] = [];
  const store = new WindowStateStore(root, (event) => void logged.push(event));

  store.capture(window(100), display);
  store.capture(window(150), display);
  await store.flush();
  await unblockSavedFile(root);
  store.capture(window(200), display);
  await store.flush();
  await blockSavedFile(root);
  store.capture(window(250), display);
  await store.close();

  expect(logged).toEqual(['window_state.write_failed', 'window_state.write_failed']);
  expect(await desktopEntries(root)).toEqual(['window-state.v1.json']);
  expect(await readdir(join(root, 'desktop', 'window-state.v1.json'))).toEqual(['held']);
});
