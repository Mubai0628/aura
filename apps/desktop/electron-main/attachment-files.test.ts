// @vitest-environment node
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { shell } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AttachmentFiles } from './attachment-files.js';
import { WorkspaceMedia } from './workspace-media.js';

// An attached
// file outside the workspace is found by Core, never by a path the renderer sends; Main streams
// it, reads its beginning, or opens it — never a program.

vi.mock('electron', () => ({
  ipcMain: { handle: vi.fn() },
  shell: { openPath: vi.fn(async () => ''), showItemInFolder: vi.fn() },
}));

const session_id = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const external_grant_id = 'xgr_01M5NMVMVN9BY58RT7W730MPES';
let base: string;

beforeEach(async () => {
  vi.clearAllMocks();
  base = await mkdtemp(join(tmpdir(), 'aura-l13-4-attachment-'));
});
afterEach(async () => {
  await rm(base, { recursive: true, force: true });
});

const core = (path: string, size: number) => ({
  requestWithBudget: vi.fn(async () => ({ path, byte_length: size })),
});

test('a request names the conversation and the authorization, nothing else', async () => {
  const files = new AttachmentFiles(new WorkspaceMedia());
  const asked = core('C:\\x.mp4', 1);
  for (const value of [
    { action: 'media', session_id, external_grant_id, path: 'C:\\Windows\\win.ini' },
    { action: 'media', session_id, external_grant_id: 'grant' },
    { action: 'run', session_id, external_grant_id },
    { action: 'media', session_id: external_grant_id, external_grant_id: session_id },
  ])
    expect(await files.handle(asked, value)).toEqual({ ok: false, message: '无效的附件请求' });
  expect(asked.requestWithBudget).not.toHaveBeenCalled();
});

test('a video streams from an address of its own, found by Core', async () => {
  const video = join(base, 'SaveTik.io_7638307602505436479.mp4');
  await writeFile(video, 'not decoded here');
  const media = new WorkspaceMedia();
  const asked = core(video, 16);
  const given = await new AttachmentFiles(media).handle(asked, {
    action: 'media',
    session_id,
    external_grant_id,
  });
  expect(asked.requestWithBudget).toHaveBeenCalledWith(
    'agent.attachments.locate',
    { session_id, external_grant_id },
    15_000,
  );
  expect(given).toMatchObject({ ok: true, kind: 'media' });
  const url = (given as { url: string }).url;
  expect(url).toMatch(/^aura:\/\/app\/workspace-media\/[0-9a-f-]{36}\/SaveTik\.io_/u);
  const served = await media.serve(new Request(url, { headers: { range: 'bytes=0-2' } }));
  expect(served?.status).toBe(206);
  expect(await served?.text()).toBe('not');
});

test("Core's refusal is said in its own words, without its code", async () => {
  const refused = {
    requestWithBudget: vi.fn(async () => {
      throw new Error('agent.attachment_missing: 找不到这个附件，它可能已被移动、删除或替换。');
    }),
  };
  expect(
    await new AttachmentFiles(new WorkspaceMedia()).handle(refused, {
      action: 'media',
      session_id,
      external_grant_id,
    }),
  ).toEqual({ ok: false, message: '找不到这个附件，它可能已被移动、删除或替换。' });
});

test('text shows its beginning; a file with NUL bytes is not text', async () => {
  const log = join(base, 'server.log');
  await writeFile(log, 'line one\nline two');
  const files = new AttachmentFiles(new WorkspaceMedia());
  expect(
    await files.handle(core(log, 17), { action: 'text', session_id, external_grant_id }),
  ).toEqual({ ok: true, kind: 'text', text: 'line one\nline two', truncated: false, size: 17 });
  const data = join(base, 'data.bin');
  await writeFile(data, Buffer.from([1, 0, 2]));
  expect(
    await files.handle(core(data, 3), { action: 'text', session_id, external_grant_id }),
  ).toEqual({ ok: true, kind: 'binary', size: 3 });
});

test('a program is never opened, only shown in the file manager', async () => {
  const program = join(base, 'setup.exe');
  await writeFile(program, 'MZ');
  const files = new AttachmentFiles(new WorkspaceMedia());
  const opened = await files.handle(core(program, 2), {
    action: 'default_application',
    session_id,
    external_grant_id,
  });
  expect(opened).toMatchObject({ ok: false });
  expect(shell.openPath).not.toHaveBeenCalled();
  expect(
    await files.handle(core(program, 2), { action: 'file_manager', session_id, external_grant_id }),
  ).toMatchObject({ ok: true, kind: 'opened' });
  expect(shell.showItemInFolder).toHaveBeenCalledWith(program);
});

// An entry of an
// attached zip is copied out by Core, which says where; Main shows or opens that copy.
test("an entry of an attached zip is Core's copy of it", async () => {
  const notes = join(base, '说明.md');
  await writeFile(notes, '# 说明');
  const files = new AttachmentFiles(new WorkspaceMedia());
  const asked = core(notes, 8);
  const entry = '项目/说明.md';
  expect(
    await files.handle(asked, { action: 'text', session_id, external_grant_id, entry }),
  ).toEqual({ ok: true, kind: 'text', text: '# 说明', truncated: false, size: 8 });
  // Within the longest budget the supervisor takes (CoreSupervisor.MAX_CALLER_BUDGET_MS).
  expect(asked.requestWithBudget).toHaveBeenCalledWith(
    'agent.attachments.entry',
    { session_id, external_grant_id, entry },
    15_000,
  );
  for (const wrong of ['', 'x'.repeat(1025), 'a\u0000b', 7])
    expect(
      await files.handle(asked, { action: 'text', session_id, external_grant_id, entry: wrong }),
    ).toEqual({ ok: false, message: '无效的附件请求' });
});

test('an entry that did not come out for a reason Core did not give says so, not that it is gone', async () => {
  const failing = {
    requestWithBudget: vi.fn(async () => {
      throw new Error('a caller budget must not exceed the production default of 15000ms');
    }),
  };
  expect(
    await new AttachmentFiles(new WorkspaceMedia()).handle(failing, {
      action: 'text',
      session_id,
      external_grant_id,
      entry: '项目/说明.md',
    }),
  ).toEqual({ ok: false, message: '没能从压缩包里取出这个文件，请稍后重试。' });
});

test('a zip in the workspace, and a file a message names, are asked of Core in their own words', async () => {
  const notes = join(base, 'notes.md');
  await writeFile(notes, '# notes');
  const files = new AttachmentFiles(new WorkspaceMedia());
  const asked = core(notes, 7);
  const entry = {
    action: 'text',
    session_id,
    workspace_path: '资料/课程.zip',
    entry: '课程/说明.md',
  };
  expect(await files.handle(asked, entry)).toMatchObject({ ok: true, kind: 'text' });
  expect(asked.requestWithBudget).toHaveBeenLastCalledWith(
    'agent.attachments.entry',
    { session_id, workspace_path: '资料/课程.zip', entry: '课程/说明.md' },
    15_000,
  );
  const mentioned_in = { kind: 'assistant', model_step_id: 'stp_01M5PF3QWJN5S3P0JFPVBSFY3R' };
  const named = { action: 'text', session_id, mentioned_path: 'D:/报告/notes.md', mentioned_in };
  expect(await files.handle(asked, named)).toMatchObject({ ok: true, kind: 'text' });
  expect(asked.requestWithBudget).toHaveBeenLastCalledWith(
    'agent.files.mentioned',
    { session_id, path: 'D:/报告/notes.md', mentioned_in },
    15_000,
  );
  for (const wrong of [
    { ...named, mentioned_path: '报告/notes.md' },
    { ...named, mentioned_in: { kind: 'reasoning', model_step_id: mentioned_in.model_step_id } },
    { ...entry, external_grant_id },
  ])
    expect(await files.handle(asked, wrong)).toEqual({ ok: false, message: '无效的附件请求' });
});

test('a file the user picked is viewed where it is, and only once the preload said it was picked', async () => {
  const picked = join(base, '课程设计.zip');
  await writeFile(picked, 'PK');
  const files = new AttachmentFiles(new WorkspaceMedia());
  const asked = core(picked, 2);
  const request = { action: 'file_manager', picked_path: picked };
  expect(await files.handle(asked, request)).toEqual({
    ok: false,
    message: '请重新选择这个文件。',
  });
  files.picked(picked.toUpperCase());
  expect(await files.handle(asked, request)).toMatchObject({ ok: true, kind: 'opened' });
  expect(shell.showItemInFolder).toHaveBeenCalledWith(picked);
  expect(asked.requestWithBudget).not.toHaveBeenCalled();
});
