// @vitest-environment node
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { shell } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { SkillRegistryView, SkillView } from '@aura/contracts/generated';
import type { CoreSupervisor } from './core-supervisor.js';
import { openSkillDirectory } from './skill-directory.js';

vi.mock('electron', () => ({
  shell: { openPath: vi.fn(async () => '') },
  dialog: { showOpenDialog: vi.fn() },
}));
beforeEach(() => vi.clearAllMocks());

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'aura-skill-directory-'));
  roots.push(root);
  const notes = join(root, 'notes');
  await mkdir(notes);
  // Something replaced a listed folder with a file after Core listed it: the shell would run it.
  const swapped = join(root, 'swapped');
  await writeFile(swapped, 'not a folder');
  const skill = (id: string, path: string) => ({ id, path }) as SkillView;
  const listed = {
    revision: 1,
    skills: [skill('skl_notes', notes), skill('skl_swapped', swapped)],
    roots: [],
    issue: null,
  } as SkillRegistryView;
  const requestAgent = vi.fn(async () => listed);
  return { notes, requestAgent, supervisor: { requestAgent } as unknown as CoreSupervisor };
}

test('only the folder Core lists for the named Skill is opened, and only a folder', async () => {
  const { notes, requestAgent, supervisor } = await fixture();
  const open = (id: string, workspace_id?: string) =>
    openSkillDirectory({ id, workspace_id } as never, supervisor);

  await expect(open('skl_notes', 'wsp_1')).resolves.toEqual({
    completed: true,
    message: '已打开 Skill 文件夹',
  });
  // A workspace Skill is found only with its workspace.
  expect(requestAgent).toHaveBeenCalledWith('agent.skills.list', { workspace_id: 'wsp_1' });
  expect(shell.openPath).toHaveBeenCalledWith(notes);

  vi.mocked(shell.openPath).mockClear();
  expect(await open('skl_missing')).toEqual({
    completed: false,
    message: '没有找到这个 Skill，请刷新后再试。',
  });
  expect(requestAgent).toHaveBeenLastCalledWith('agent.skills.list', { workspace_id: null });
  expect(await open('skl_swapped')).toEqual({
    completed: false,
    message: '这个 Skill 的文件夹已不存在，请刷新后再试。',
  });
  expect(shell.openPath).not.toHaveBeenCalled();

  vi.mocked(shell.openPath).mockResolvedValueOnce('Access is denied.');
  expect((await open('skl_notes')).completed).toBe(false);
});
