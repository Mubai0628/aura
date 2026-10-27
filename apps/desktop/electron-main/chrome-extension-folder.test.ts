// @vitest-environment node
import { mkdir, mkdtemp, readdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { app, shell } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  bundledChromeExtension,
  chromeExtensionCopy,
  openChromeExtensionFolder,
  refreshChromeExtensionCopy,
  refreshLoadedChromeExtension,
} from './chrome-extension-folder.js';
import { agentRouteHandler } from './command-directory.js';
import type { CoreSupervisor } from './core-supervisor.js';

vi.mock('electron', () => ({
  app: { isPackaged: false, getAppPath: vi.fn(() => '') },
  shell: { openPath: vi.fn(async () => '') },
  dialog: { showOpenDialog: vi.fn() },
}));

const roots: string[] = [];
const resources = process.resourcesPath;
beforeEach(() => vi.clearAllMocks());
afterEach(async () => {
  Object.defineProperty(process, 'resourcesPath', { configurable: true, value: resources });
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function root(): Promise<string> {
  const made = await mkdtemp(join(tmpdir(), 'aura-chrome-extension-folder-'));
  roots.push(made);
  return made;
}

async function extension(folder: string, files: Record<string, string>): Promise<string> {
  for (const [path, text] of Object.entries(files)) {
    await mkdir(join(folder, path, '..'), { recursive: true });
    await writeFile(join(folder, path), text);
  }
  return folder;
}

async function listing(folder: string): Promise<Record<string, string>> {
  const found: Record<string, string> = {};
  for (const entry of await readdir(folder, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const path = join(entry.parentPath, entry.name);
    found[path.slice(folder.length + 1).replaceAll('\\', '/')] = await readFile(path, 'utf8');
  }
  return found;
}

test('an installed Aura opens a copy of the extension beside its resources, in its data folder', async () => {
  const installed = await root();
  const data = await root();
  Object.defineProperty(app, 'isPackaged', { configurable: true, value: true });
  Object.defineProperty(process, 'resourcesPath', { configurable: true, value: installed });
  await extension(join(installed, 'chrome-extension'), {
    'manifest.json': '{"version":"0.1.4"}',
    'side-panel/index.html': '<!doctype html>',
  });
  const copy = chromeExtensionCopy(data);
  await expect(openChromeExtensionFolder(data)).resolves.toEqual({ path: copy, opened: true });
  expect(shell.openPath).toHaveBeenCalledWith(copy);
  expect(await listing(copy)).toEqual({
    'manifest.json': '{"version":"0.1.4"}',
    'side-panel/index.html': '<!doctype html>',
  });
});

test('the copy becomes the running version whole, and a current copy is left alone', async () => {
  const work = await root();
  const source = await extension(join(work, 'shipped'), {
    'manifest.json': '{"version":"0.1.4"}',
    'service-worker.js': 'old',
    'chunks/old.js': 'gone next version',
  });
  const copy = join(work, 'data', 'chrome', 'extension');
  await expect(refreshChromeExtensionCopy(source, copy)).resolves.toBe('updated');
  const written = (await stat(join(copy, 'service-worker.js'))).mtimeMs;
  await expect(refreshChromeExtensionCopy(source, copy)).resolves.toBe('current');
  expect((await stat(join(copy, 'service-worker.js'))).mtimeMs).toBe(written);

  await rm(join(source, 'chunks', 'old.js'));
  await extension(source, {
    'manifest.json': '{"version":"0.1.5"}',
    'service-worker.js': 'new',
    'chunks/new.js': 'added',
  });
  await expect(refreshChromeExtensionCopy(source, copy)).resolves.toBe('updated');
  expect(await listing(copy)).toEqual({
    'manifest.json': '{"version":"0.1.5"}',
    'service-worker.js': 'new',
    'chunks/new.js': 'added',
  });
  expect((await readdir(join(work, 'data', 'chrome'))).sort()).toEqual(['extension']);
});

test('at start only a copy the user already has follows the running version', async () => {
  const installed = await root();
  const data = await root();
  Object.defineProperty(app, 'isPackaged', { configurable: true, value: true });
  Object.defineProperty(process, 'resourcesPath', { configurable: true, value: installed });
  await extension(join(installed, 'chrome-extension'), { 'manifest.json': '{"version":"0.1.5"}' });
  const log = vi.fn();
  await refreshLoadedChromeExtension(data, log);
  expect(await readdir(data)).toEqual([]);
  expect(log).not.toHaveBeenCalled();

  await extension(chromeExtensionCopy(data), { 'manifest.json': '{"version":"0.1.4"}' });
  await refreshLoadedChromeExtension(data, log);
  expect(await listing(chromeExtensionCopy(data))).toEqual({
    'manifest.json': '{"version":"0.1.5"}',
  });
  expect(log).toHaveBeenCalledWith('chrome.extension.copy', { state: 'updated' });
});

test('a development build copies its own test build, and says when it is not built', async () => {
  const checkout = await root();
  const data = await root();
  Object.defineProperty(app, 'isPackaged', { configurable: true, value: false });
  vi.mocked(app.getAppPath).mockReturnValue(join(checkout, 'apps', 'desktop'));
  expect(bundledChromeExtension()).toBe(join(checkout, 'apps', 'chrome-extension', 'dist', 'test'));
  const copy = chromeExtensionCopy(data);
  await expect(openChromeExtensionFolder(data)).resolves.toEqual({ path: copy, opened: false });
  expect(shell.openPath).not.toHaveBeenCalled();

  await extension(copy, { 'manifest.json': '{"version":"0.1.4"}' });
  await expect(openChromeExtensionFolder(data)).resolves.toEqual({ path: copy, opened: true });
});

test('Main opens the folder itself; Core is not asked', async () => {
  const requestAgent = vi.fn();
  const handle = agentRouteHandler(
    { requestAgent } as unknown as CoreSupervisor,
    () => ({}) as never,
  );
  await handle({} as never, 'chrome.extension.open_folder', {} as never);
  expect(requestAgent).not.toHaveBeenCalled();
});
