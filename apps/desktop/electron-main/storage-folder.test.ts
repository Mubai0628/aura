// @vitest-environment node
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { dialog, shell, type BrowserWindow } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { StorageOverview } from '@aura/contracts/generated';
import { agentRouteHandler } from './command-directory.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { INSTALLED_SETUP, moveInstallation } from './storage-folder.js';

vi.mock('electron', () => ({
  shell: { openPath: vi.fn(async () => '') },
  dialog: { showOpenDialog: vi.fn() },
}));
beforeEach(() => vi.clearAllMocks());

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function fixture(install: boolean) {
  const root = await mkdtemp(join(tmpdir(), 'aura-storage-folder-'));
  roots.push(root);
  const overview: StorageOverview = {
    data: { path: root, bytes: 1 },
    data_moved: false,
    install: install ? { path: join(root, 'gone'), bytes: 1 } : null,
    requested: null,
    failure: null,
    old_copies: false,
  };
  const requestAgent = vi.fn(async () => overview);
  return { root, requestAgent, supervisor: { requestAgent } as unknown as CoreSupervisor };
}

const window = { isDestroyed: () => false } as BrowserWindow;
const event = {} as never;

test('moving the installation opens its own setup program and closes Aura; nothing happens without one', async () => {
  const { root, supervisor } = await fixture(true);
  const installed = join(root, 'gone');
  const open = vi.fn(() => true);
  const quit = vi.fn();
  await expect(moveInstallation(supervisor, open, quit)).resolves.toEqual({ restarting: false });
  expect(open).not.toHaveBeenCalled();
  await mkdir(installed, { recursive: true });
  await writeFile(join(installed, INSTALLED_SETUP), 'MZ');
  await expect(moveInstallation(supervisor, open, quit)).resolves.toEqual({ restarting: true });
  expect(open).toHaveBeenCalledWith(join(installed, INSTALLED_SETUP), [
    '--relocate',
    '--from-aura',
  ]);
  expect(quit).toHaveBeenCalledTimes(1);
  const development = await fixture(false);
  await expect(moveInstallation(development.supervisor, open, quit)).resolves.toEqual({
    restarting: false,
  });
  expect(quit).toHaveBeenCalledTimes(1);
});

test('a setup program that did not start says why and Aura stays open', async () => {
  const { root, supervisor } = await fixture(true);
  const installed = join(root, 'gone');
  await mkdir(installed, { recursive: true });
  await writeFile(join(installed, INSTALLED_SETUP), 'MZ');
  const quit = vi.fn();
  await expect(moveInstallation(supervisor, () => false, quit)).rejects.toThrow(
    '安装程序没能启动（可能被 Windows 或安全软件拦截），安装位置没有移动，Aura 保持打开；请稍后再试，或手动运行安装目录里的 Uninstall Aura.exe。',
  );
  expect(quit).not.toHaveBeenCalled();
});

test('the 存储 folder dialog belongs to the calling window and Core never sees it', async () => {
  const { requestAgent, supervisor } = await fixture(false);
  const handle = agentRouteHandler(supervisor, () => window);
  vi.mocked(dialog.showOpenDialog).mockResolvedValue({
    canceled: false,
    filePaths: ['D:/AuraData'],
  });
  await expect(handle(event, 'storage.select_folder', {} as never)).resolves.toEqual({
    path: 'D:/AuraData',
  });
  expect(dialog.showOpenDialog).toHaveBeenLastCalledWith(
    window,
    expect.objectContaining({ title: '选择数据要放的文件夹' }),
  );
  expect(requestAgent).not.toHaveBeenCalled();
});

test('opening a folder opens the one Core names, and says when there is none to open', async () => {
  const { root, requestAgent, supervisor } = await fixture(false);
  const handle = agentRouteHandler(supervisor, () => window);
  await expect(handle(event, 'storage.open_folder', { part: 'data' } as never)).resolves.toEqual({
    completed: true,
    message: '已打开',
  });
  expect(requestAgent).toHaveBeenLastCalledWith('storage.overview', {});
  expect(shell.openPath).toHaveBeenCalledWith(root);
  await expect(handle(event, 'storage.open_folder', { part: 'install' } as never)).resolves.toEqual(
    { completed: false, message: '开发版没有安装文件夹。' },
  );

  const installed = await fixture(true);
  const handleInstalled = agentRouteHandler(installed.supervisor, () => window);
  await expect(
    handleInstalled(event, 'storage.open_folder', { part: 'install' } as never),
  ).resolves.toEqual({ completed: false, message: '这个文件夹已不存在。' });
  expect(shell.openPath).toHaveBeenCalledTimes(1);
});

test('closing for the move restarts only when a launcher started Aura; the rest is Core’s', async () => {
  const { requestAgent, supervisor } = await fixture(false);
  const restart = vi.fn(() => true);
  const handle = agentRouteHandler(supervisor, () => window, undefined, undefined, restart);
  await expect(handle(event, 'storage.restart', {} as never)).resolves.toEqual({
    restarting: true,
  });
  expect(restart).toHaveBeenCalledTimes(1);
  const development = agentRouteHandler(supervisor, () => window);
  await expect(development(event, 'storage.restart', {} as never)).resolves.toEqual({
    restarting: false,
  });
  expect(requestAgent).not.toHaveBeenCalled();
  for (const [route, params] of [
    ['storage.overview', {}],
    ['storage.check', { folder: 'D:/AuraData' }],
    ['storage.move.request', { folder: null }],
    ['storage.move.withdraw', {}],
    ['storage.failure.dismiss', {}],
    ['storage.old_data.list', {}],
    ['storage.old_data.remove', { ids: ['early'] }],
  ] as const) {
    await handle(event, route, params as never);
    expect(requestAgent).toHaveBeenLastCalledWith(route, params);
  }
});
