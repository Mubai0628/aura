// @vitest-environment node
import { beforeEach, expect, test, vi } from 'vitest';
import type { BrowserWindow } from 'electron';
import { selectHookDirectory } from './hook-directory.js';

const { showOpenDialog } = vi.hoisted(() => ({ showOpenDialog: vi.fn() }));
vi.mock('electron', () => ({ dialog: { showOpenDialog } }));
beforeEach(() => showOpenDialog.mockReset());

test('the Hook dialog picks exactly one folder in the owning window and reads nothing', async () => {
  const owner = { isDestroyed: () => false } as BrowserWindow;
  showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ['C:/hooks/守卫'] });
  expect(await selectHookDirectory(owner)).toEqual({ path: 'C:/hooks/守卫' });
  expect(showOpenDialog).toHaveBeenCalledWith(owner, {
    title: '选择 Hook 文件夹',
    buttonLabel: '选择文件夹',
    properties: ['openDirectory', 'dontAddToRecent'],
  });
});

test('cancel, a closed window and a malformed answer never return a path', async () => {
  const owner = { isDestroyed: vi.fn(() => false) };
  const window = owner as unknown as BrowserWindow;
  showOpenDialog.mockResolvedValue({ canceled: true, filePaths: ['C:/ignored'] });
  expect(await selectHookDirectory(window)).toEqual({ path: null });
  showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ['C:/a', 'C:/b'] });
  await expect(selectHookDirectory(window)).rejects.toThrow('无效的选择');
  owner.isDestroyed.mockReturnValue(true);
  showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ['C:/ignored'] });
  expect(await selectHookDirectory(window)).toEqual({ path: null });
});

test('a second request while the dialog is open shares it', async () => {
  const owner = { isDestroyed: () => false } as BrowserWindow;
  let finish: (value: unknown) => void = () => undefined;
  showOpenDialog.mockReturnValue(new Promise((resolve) => (finish = resolve)));
  const first = selectHookDirectory(owner);
  const second = selectHookDirectory(owner);
  finish({ canceled: false, filePaths: ['C:/hooks/one'] });
  expect(await first).toEqual({ path: 'C:/hooks/one' });
  expect(await second).toEqual({ path: 'C:/hooks/one' });
  expect(showOpenDialog).toHaveBeenCalledTimes(1);
});
