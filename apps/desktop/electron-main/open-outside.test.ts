// @vitest-environment node
import { mkdir, mkdtemp, realpath, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { shell } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { CoreSupervisor } from './core-supervisor.js';
import { openExternalLink, openWorkspaceFileOutside } from './open-outside.js';

// A workspace file opens with its default application or in the file
// manager, never a program or a script, never anything outside the workspace; a web link opens in
// the default browser, http(s) only.

vi.mock('electron', () => ({
  shell: {
    openPath: vi.fn(async () => ''),
    openExternal: vi.fn(async () => undefined),
    showItemInFolder: vi.fn(),
  },
}));
beforeEach(() => vi.clearAllMocks());

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

const workspace_id = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const session_id = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';

async function fixture() {
  const base = await mkdtemp(join(tmpdir(), 'aura-open-outside-'));
  roots.push(base);
  const root = join(base, 'workspace');
  const outside = join(base, 'outside');
  await mkdir(join(root, 'docs'), { recursive: true });
  await mkdir(outside);
  await writeFile(join(root, 'docs', 'plan.pdf'), 'pdf');
  await writeFile(join(root, 'setup.bat'), '@echo off');
  await writeFile(join(outside, 'secret.txt'), 'outside');
  // A junction needs no privilege: a folder in the workspace that leads out of it.
  await symlink(outside, join(root, 'escape'), 'junction');
  const supervisor = {
    refreshWorkspaceSessionSnapshot: vi.fn(async () => ({
      payload: {
        workspaces: [{ workspace_id, canonical_root: root }],
        sessions: [{ session_id, workspace_id }],
      },
    })),
  } as unknown as CoreSupervisor;
  return { root, supervisor };
}

const open = (supervisor: CoreSupervisor, path: string, opener = 'default_application') =>
  openWorkspaceFileOutside(
    { workspace_id, session_id, path, opener } as Parameters<typeof openWorkspaceFileOutside>[0],
    supervisor,
  );

test('a workspace file opens with its default application, found where it really is', async () => {
  const { root, supervisor } = await fixture();
  await expect(open(supervisor, 'docs/plan.pdf')).resolves.toEqual({
    completed: true,
    message: '已用默认应用打开',
  });
  expect(shell.openPath).toHaveBeenCalledWith(await realpath(join(root, 'docs', 'plan.pdf')));
  await expect(open(supervisor, 'docs/plan.pdf', 'file_manager')).resolves.toMatchObject({
    completed: true,
  });
  expect(shell.showItemInFolder).toHaveBeenCalledWith(
    await realpath(join(root, 'docs', 'plan.pdf')),
  );
});

test('a program or a script is never opened, only shown in the file manager', async () => {
  const { supervisor } = await fixture();
  const refused = await open(supervisor, 'setup.bat');
  expect(refused.completed).toBe(false);
  expect(refused.message).toContain('不会直接运行');
  expect(shell.openPath).not.toHaveBeenCalled();
  await expect(open(supervisor, 'setup.bat', 'file_manager')).resolves.toMatchObject({
    completed: true,
  });
});

test('nothing outside the workspace opens, whatever the path says', async () => {
  const { supervisor } = await fixture();
  for (const path of ['../outside/secret.txt', 'C:/Windows/notepad.exe', 'docs/../../x', 'nul']) {
    expect((await open(supervisor, path)).completed, path).toBe(false);
  }
  // Through a junction the path stays in the workspace, but the file does not.
  const escaped = await open(supervisor, 'escape/secret.txt');
  expect(escaped).toEqual({
    completed: false,
    message: '这个文件在工作区外，Aura 不会打开工作区外的文件。',
  });
  await expect(open(supervisor, 'docs/missing.pdf')).resolves.toMatchObject({ completed: false });
  expect(shell.openPath).not.toHaveBeenCalled();
  expect(shell.showItemInFolder).not.toHaveBeenCalled();
});

test('a conversation of another workspace is refused', async () => {
  const { supervisor } = await fixture();
  const refused = await openWorkspaceFileOutside(
    {
      workspace_id: 'wsp_01BX5ZZKBKACTAV9WEVGEMMVRZ',
      session_id,
      path: 'docs/plan.pdf',
      opener: 'default_application',
    },
    supervisor,
  );
  expect(refused.completed).toBe(false);
  expect(shell.openPath).not.toHaveBeenCalled();
});

test('a web link opens in the browser: http(s) only, never one with an account in it', async () => {
  await expect(openExternalLink({ url: 'https://example.com/docs?q=1' })).resolves.toEqual({
    completed: true,
    message: '已在默认浏览器中打开',
  });
  expect(shell.openExternal).toHaveBeenCalledWith('https://example.com/docs?q=1');
  for (const url of [
    'javascript:alert(1)',
    'file:///C:/Windows/System32/calc.exe',
    'mailto:a@example.com',
    'https://user:pass@example.com/',
    'not a link',
  ]) {
    expect((await openExternalLink({ url })).completed, url).toBe(false);
  }
  expect(shell.openExternal).toHaveBeenCalledTimes(1);
});
