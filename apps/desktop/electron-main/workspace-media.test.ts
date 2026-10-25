// @vitest-environment node
import { mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { isAbsolute, join, relative, resolve, toNamespacedPath } from 'node:path';
import { afterEach, expect, test, vi } from 'vitest';

import type { CoreSupervisor } from './core-supervisor.js';
import { WorkspaceMedia } from './workspace-media.js';

// A 文件 tab plays a workspace's video and
// sound and shows its pictures from an address Main streams; nothing that is not media, and
// nothing outside the workspace, gets one.

// Only IPC registration is mocked: responses stream the real disposable local file.
vi.mock('electron', () => ({
  ipcMain: { handle: vi.fn() },
}));

const workspaceId = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const sessionId = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const bases: string[] = [];
afterEach(async () => {
  for (const base of bases.splice(0)) {
    const target = resolve(base);
    const rel = relative(resolve(tmpdir()), target);
    if (isAbsolute(rel) || rel.startsWith('..') || !rel.startsWith('aura-workspace-media-'))
      throw new Error(`Refusing to remove a non-fixture directory: ${target}`);
    await rm(toNamespacedPath(target), { recursive: true, force: true });
  }
});

async function fixture() {
  const base = await mkdtemp(join(tmpdir(), 'aura-workspace-media-'));
  bases.push(base);
  const root = join(base, 'workspace');
  const outside = join(base, 'outside');
  await mkdir(join(root, 'baidu_video'), { recursive: true });
  await mkdir(outside);
  await writeFile(join(root, 'baidu_video', 'clip.mp4'), Buffer.from('0123456789'));
  await writeFile(join(root, 'notes.txt'), 'notes');
  await writeFile(join(outside, 'secret.mp4'), 'outside');
  await symlink(outside, join(root, 'escape'), 'junction');
  const core = {
    refreshWorkspaceSessionSnapshot: vi.fn(async () => ({
      payload: {
        workspaces: [{ workspace_id: workspaceId, canonical_root: root }],
        sessions: [{ session_id: sessionId, workspace_id: workspaceId }],
      },
    })),
  } as unknown as Pick<CoreSupervisor, 'refreshWorkspaceSessionSnapshot'>;
  return { core };
}

const ask = (path: string) => ({ session_id: sessionId, workspace_id: workspaceId, path });

test('a workspace video gets an address that streams the range the player asks for', async () => {
  const { core } = await fixture();
  const media = new WorkspaceMedia();
  const given = await media.address(core, ask('baidu_video/clip.mp4'));
  expect(given.ok).toBe(true);
  if (!given.ok) return;
  expect(given.url).toMatch(/^aura:\/\/app\/workspace-media\/[0-9a-f-]{36}\/clip\.mp4$/u);
  const whole = await media.serve(new Request(given.url));
  expect(whole?.status).toBe(200);
  expect(whole?.headers.get('accept-ranges')).toBe('bytes');
  expect(whole?.headers.get('content-length')).toBe('10');
  expect(whole?.headers.get('content-type')).toBe('video/mp4');
  expect(await whole?.text()).toBe('0123456789');
  expect(whole?.headers.get('content-security-policy')).toContain('sandbox');
  const part = await media.serve(new Request(given.url, { headers: { range: 'bytes=2-5' } }));
  expect(part?.status).toBe(206);
  expect(part?.headers.get('content-range')).toBe('bytes 2-5/10');
  expect(part?.headers.get('accept-ranges')).toBe('bytes');
  expect(part?.headers.get('content-length')).toBe('4');
  expect(await part?.text()).toBe('2345');
});

test.each([
  ['bytes=7-', '789', 'bytes 7-9/10'],
  ['bytes=-3', '789', 'bytes 7-9/10'],
  ['bytes=7-99', '789', 'bytes 7-9/10'],
  ['bytes=-99', '0123456789', 'bytes 0-9/10'],
])(
  'a single player range %s returns exactly its bytes and complete headers',
  async (range, bytes, contentRange) => {
    const { core } = await fixture();
    const media = new WorkspaceMedia();
    const given = await media.address(core, ask('baidu_video/clip.mp4'));
    if (!given.ok) throw new Error(given.message);
    const response = await media.serve(new Request(given.url, { headers: { range } }));
    expect(response?.status).toBe(206);
    expect(response?.headers.get('content-range')).toBe(contentRange);
    expect(response?.headers.get('accept-ranges')).toBe('bytes');
    expect(response?.headers.get('content-length')).toBe(String(bytes.length));
    expect(await response?.text()).toBe(bytes);
  },
);

test.each(['bytes=10-', 'bytes=6-2', 'bytes=-0', 'bytes=a-b', 'bytes=0-1,4-5', 'items=0-1'])(
  'an invalid player range %s returns 416 without file bytes',
  async (range) => {
    const { core } = await fixture();
    const media = new WorkspaceMedia();
    const given = await media.address(core, ask('baidu_video/clip.mp4'));
    if (!given.ok) throw new Error(given.message);
    const response = await media.serve(new Request(given.url, { headers: { range } }));
    expect(response?.status).toBe(416);
    expect(response?.headers.get('content-range')).toBe('bytes */10');
    expect(response?.headers.get('accept-ranges')).toBe('bytes');
    expect(response?.headers.get('content-length')).toBe('0');
    expect(await response?.text()).toBe('');
  },
);

test('nothing that is not media, outside the workspace or unknown gets streamed', async () => {
  const { core } = await fixture();
  const media = new WorkspaceMedia();
  expect(await media.address(core, ask('notes.txt'))).toMatchObject({ ok: false });
  expect(await media.address(core, ask('escape/secret.mp4'))).toMatchObject({ ok: false });
  expect(await media.address(core, ask('../outside/secret.mp4'))).toMatchObject({ ok: false });
  expect(await media.address(core, { ...ask('baidu_video/clip.mp4'), extra: 1 })).toMatchObject({
    ok: false,
  });
  const unknown = await media.serve(
    new Request('aura://app/workspace-media/00000000-0000-4000-8000-000000000000/clip.mp4'),
  );
  expect(unknown?.status).toBe(404);
  expect(await media.serve(new Request('aura://app/index.html'))).toBeUndefined();
});
