// @vitest-environment node
import { mkdir, mkdtemp, realpath, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { afterEach, expect, test, vi } from 'vitest';

import type { CoreSupervisor } from '../core-supervisor.js';
import { openWorkspacePage } from './workspace-page.js';

// A workspace web page opens in a page of the
// user's own, found where it really is; nothing else and nothing outside the workspace.

vi.mock('electron', () => ({ shell: {} }));

const workspaceId = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const sessionId = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const bases: string[] = [];
afterEach(async () => {
  for (const base of bases.splice(0)) await rm(base, { recursive: true, force: true });
});

async function fixture() {
  const base = await mkdtemp(join(tmpdir(), 'aura-workspace-page-'));
  bases.push(base);
  const root = join(base, 'workspace');
  const outside = join(base, 'outside');
  await mkdir(join(root, 'game'), { recursive: true });
  await mkdir(outside);
  await writeFile(join(root, 'game', 'index.html'), '<p>game</p>');
  await writeFile(join(root, 'notes.txt'), 'notes');
  await writeFile(join(outside, 'page.html'), '<p>outside</p>');
  await symlink(outside, join(root, 'escape'), 'junction');
  const core = {
    refreshWorkspaceSessionSnapshot: vi.fn(async () => ({
      payload: {
        workspaces: [{ workspace_id: workspaceId, canonical_root: root }],
        sessions: [{ session_id: sessionId, workspace_id: workspaceId }],
      },
    })),
  } as unknown as CoreSupervisor;
  const pages = { openFile: vi.fn(() => undefined), state: vi.fn(() => undefined) };
  return { root, core, pages };
}

const command = (path: string) =>
  ({ kind: 'open-file', sessionId, pageId: 'page-1', workspaceId, path }) as const;

test('a workspace web page opens in the page, with its workspace as the folder it may read', async () => {
  const { root, core, pages } = await fixture();
  await expect(openWorkspacePage(pages, core, command('game/index.html'))).resolves.toEqual({
    ok: true,
    state: undefined,
  });
  const real = await realpath(root);
  expect(pages.openFile).toHaveBeenCalledWith(
    sessionId,
    'page-1',
    pathToFileURL(join(real, 'game', 'index.html')).href,
    `${pathToFileURL(real).href}/`,
  );
});

test('anything but a web page inside the workspace is not opened', async () => {
  const { core, pages } = await fixture();
  for (const path of ['notes.txt', 'escape/page.html', '../outside/page.html', 'game/missing.html'])
    expect((await openWorkspacePage(pages, core, command(path))).ok, path).toBe(false);
  expect(pages.openFile).not.toHaveBeenCalled();
});
