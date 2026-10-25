// @vitest-environment node
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { shell, type BrowserWindow, type IpcMainInvokeEvent } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { CoreSupervisor } from './core-supervisor.js';
import { dispatchMemoryCommand } from './memory-requests.js';

vi.mock('electron', () => ({
  shell: { openPath: vi.fn(async () => '') },
  dialog: { showSaveDialog: vi.fn() },
}));
beforeEach(() => vi.clearAllMocks());

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

/** Core's answer for the project Vault, naming `log` as its maintenance log. */
function core(log: string | null) {
  const memory = vi.fn(async (command: string) => {
    if (command !== 'memory.vaults.get') throw new Error(`unexpected ${command}`);
    return {
      vaults: [
        {
          scope: 'project',
          directory: 'C:\\work\\.memories',
          exists: true,
          valid: 1,
          invalid: 0,
          rebuilding: false,
          watcher_error: null,
          recent_changes: [],
          maintenance_log: log,
        },
      ],
      recent_runs: [],
    };
  });
  return { memory, supervisor: { domains: { memory } } as unknown as CoreSupervisor };
}

const open = async (supervisor: CoreSupervisor) =>
  (await dispatchMemoryCommand(
    {} as IpcMainInvokeEvent,
    'memory.maintenance.open_log',
    { workspace_id: 'wsp_1', scope: 'project' } as never,
    supervisor,
    () => ({}) as BrowserWindow,
  )) as { completed: boolean; message: string };

// The memory page's maintenance log opens in the user's editor; the renderer names no path.
test('the maintenance log Core names opens, in its ordinary spelling', async () => {
  const root = await mkdtemp(join(tmpdir(), 'aura-maintenance-log-'));
  roots.push(root);
  const log = join(root, 'maintenance.log');
  await writeFile(log, '2026-10-18T13:03:15Z  已完成\n');
  const { memory, supervisor } = core(`\\\\?\\${log}`);
  await expect(open(supervisor)).resolves.toEqual({ completed: true, message: '已打开整理日志' });
  expect(memory).toHaveBeenCalledWith('memory.vaults.get', { workspace_id: 'wsp_1' });
  expect(shell.openPath).toHaveBeenCalledWith(log);
});

test('without a log there is nothing to open, and the user is told when there will be', async () => {
  const { supervisor } = core(null);
  const answer = await open(supervisor);
  expect(answer.completed).toBe(false);
  expect(answer.message).toContain('第一次整理完成后');
  expect(shell.openPath).not.toHaveBeenCalled();
});

test('only a log file goes to the shell, never a folder or another file put in its place', async () => {
  const root = await mkdtemp(join(tmpdir(), 'aura-maintenance-log-'));
  roots.push(root);
  const folder = join(root, 'maintenance.log');
  await mkdir(folder);
  expect((await open(core(folder).supervisor)).completed).toBe(false);
  const program = join(root, 'maintenance.exe');
  await writeFile(program, 'not a log');
  expect((await open(core(program).supervisor)).completed).toBe(false);
  expect(shell.openPath).not.toHaveBeenCalled();
});
