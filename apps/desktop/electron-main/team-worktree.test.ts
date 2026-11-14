// @vitest-environment node
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { shell } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { CoreSupervisor } from './core-supervisor.js';
import { openAgentWorktree } from './team-worktree.js';

vi.mock('electron', () => ({
  shell: { openPath: vi.fn(async () => '') },
  dialog: { showOpenDialog: vi.fn() },
}));
beforeEach(() => vi.clearAllMocks());

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

/** A worktree folder, and a file standing where Core named a folder. */
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'aura-team-worktree-'));
  roots.push(root);
  const copy = join(root, 'a1');
  await mkdir(copy);
  const swapped = join(root, 'swapped');
  await writeFile(swapped, 'not a folder');
  return { copy, swapped };
}

const supervisor = (answer: (params: unknown) => Promise<unknown>) => {
  const requestWithBudget = vi.fn(async (_method: string, params: unknown) => answer(params));
  return { requestWithBudget, supervisor: { requestWithBudget } as unknown as CoreSupervisor };
};

test('the folder Core names for the sub-agent opens, and only a folder', async () => {
  const { copy, swapped } = await fixture();
  const request = { session_id: 'ses_1', agent: 'A1' } as never;
  const found = supervisor(async () => ({ path: copy }));
  await expect(openAgentWorktree(request, found.supervisor)).resolves.toEqual({
    completed: true,
    message: '已打开',
  });
  expect(found.requestWithBudget).toHaveBeenCalledWith(
    'agent.team.worktree.folder',
    { session_id: 'ses_1', agent: 'A1' },
    15_000,
  );
  expect(shell.openPath).toHaveBeenCalledWith(copy);

  vi.mocked(shell.openPath).mockClear();
  const file = supervisor(async () => ({ path: swapped }));
  await expect(openAgentWorktree(request, file.supervisor)).resolves.toEqual({
    completed: false,
    message: '它的工作副本文件夹已不存在。',
  });
  expect(shell.openPath).not.toHaveBeenCalled();
});

test('a sub-agent without a worktree in use opens nothing: Core refuses it', async () => {
  const refused = supervisor(async () => {
    throw new Error('agents.no_worktree: A1 没有在用的工作副本。');
  });
  await expect(
    openAgentWorktree({ session_id: 'ses_1', agent: 'A1' } as never, refused.supervisor),
  ).rejects.toThrow('agents.no_worktree');
  expect(shell.openPath).not.toHaveBeenCalled();
});
