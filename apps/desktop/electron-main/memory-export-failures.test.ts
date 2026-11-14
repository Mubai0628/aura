// @vitest-environment node
import type { BrowserWindow, IpcMainInvokeEvent } from 'electron';
import { expect, test, vi } from 'vitest';
import type { CoreSupervisor } from './core-supervisor.js';
import { dispatchMemoryCommand } from './memory-requests.js';

// An export that could not be written says why in words and what to do; the ZIP is written by
// a stand-in here, and nothing touches the disk.

vi.mock('electron', () => ({
  shell: { openPath: vi.fn(async () => '') },
  dialog: {
    showSaveDialog: vi.fn(async () => ({
      canceled: false,
      filePath: 'Z:\\aura-missing-test-folder\\memories.zip',
    })),
  },
}));
const failure = vi.hoisted(() => ({ code: 'ENOSPC' }));
vi.mock('./memory-zip.js', () => ({
  writeVaultZip: vi.fn(async () => {
    throw Object.assign(new Error(`${failure.code}: file system error`), { code: failure.code });
  }),
}));

const memory = vi.fn(async (command: string) => {
  if (command === 'memory.vaults.get')
    return {
      vaults: [{ scope: 'project', directory: 'Z:\\aura-missing-test-folder', exists: true }],
      recent_runs: [],
    };
  if (command === 'memory.export.prepare') return { export_id: 'exp_1' };
  return null;
});
const supervisor = { domains: { memory } } as unknown as CoreSupervisor;

const exported = () =>
  dispatchMemoryCommand(
    {} as IpcMainInvokeEvent,
    'memory.vault.export',
    { workspace_id: 'wsp_1', scope: 'project' } as never,
    supervisor,
    () => ({}) as BrowserWindow,
  );

test.each([
  ['ENOSPC', '磁盘空间不够，Vault 没有导出；腾出空间后再试。'],
  ['EBUSY', '同名 ZIP 正被其他程序打开，Vault 没有导出；关闭它或换个文件名再试。'],
  [
    'EPERM',
    '没有权限写入所选位置（文件夹不允许写入，或同名 ZIP 正被其他程序打开），Vault 没有导出；请换一个文件夹或文件名再试。',
  ],
])('an export that failed with %s says why', async (code, words) => {
  failure.code = code;
  await expect(exported()).rejects.toThrow(words);
  expect(memory).toHaveBeenCalledWith('memory.export.close', { export_id: 'exp_1' });
});
