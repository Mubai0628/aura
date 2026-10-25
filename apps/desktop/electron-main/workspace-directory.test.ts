// @vitest-environment node
import { beforeEach, expect, test, vi } from 'vitest';
import type { BrowserWindow } from 'electron';
import { selectWorkspaceDirectory } from './workspace-directory.js';

const { showOpenDialog } = vi.hoisted(() => ({ showOpenDialog: vi.fn() }));
vi.mock('electron', () => ({ dialog: { showOpenDialog } }));
beforeEach(() => showOpenDialog.mockReset());

test('native selection is single-directory, owned, and returns a suggested name without creating data', async () => {
  const owner = { isDestroyed: () => false } as BrowserWindow;
  showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ['C:/synthetic/中文 project'] });
  expect(await selectWorkspaceDirectory(owner)).toEqual({
    status: 'selected',
    selected_root: 'C:/synthetic/中文 project',
    suggested_display_name: '中文 project',
  });
  expect(showOpenDialog).toHaveBeenCalledWith(owner, {
    title: '选择工作区文件夹',
    buttonLabel: '选择文件夹',
    properties: ['openDirectory', 'dontAddToRecent'],
  });
});

test('cancel and a closed owning window never return a path', async () => {
  const owner = { isDestroyed: vi.fn(() => false) };
  showOpenDialog.mockResolvedValue({ canceled: true, filePaths: ['C:/ignored'] });
  expect(await selectWorkspaceDirectory(owner as unknown as BrowserWindow)).toEqual({
    status: 'cancelled',
  });
  owner.isDestroyed.mockReturnValue(true);
  showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ['C:/ignored'] });
  expect(await selectWorkspaceDirectory(owner as unknown as BrowserWindow)).toEqual({
    status: 'cancelled',
  });
});

test('concurrent requests share the owning window dialog and a rejected dialog can be retried', async () => {
  const owner = { isDestroyed: () => false } as BrowserWindow;
  let finish: (value: unknown) => void = () => undefined;
  showOpenDialog.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const first = selectWorkspaceDirectory(owner);
  const second = selectWorkspaceDirectory(owner);
  expect(showOpenDialog).toHaveBeenCalledTimes(1);
  finish({ canceled: true, filePaths: [] });
  expect(await first).toEqual(await second);
  showOpenDialog.mockRejectedValueOnce(new Error('dialog failure'));
  await expect(selectWorkspaceDirectory(owner)).rejects.toThrow('dialog failure');
  showOpenDialog.mockResolvedValueOnce({ canceled: false, filePaths: ['C:/'] });
  expect(await selectWorkspaceDirectory(owner)).toMatchObject({
    status: 'selected',
    suggested_display_name: 'C:/',
  });
});
