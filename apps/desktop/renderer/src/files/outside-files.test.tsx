import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileLinkContext } from './file-links';
import { readFile } from './file-reads';
import {
  fileSource,
  mentionedEntry,
  outsideTarget,
  pickedEntry,
  workspaceZipEntry,
} from './file-sources';
import { root, session, workspace } from './files-fixture';
import { MentionContext } from './mention-context';
import { zipEntryFinder } from './zip-entries';
import { MarkdownBody } from '../markdown-body';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';

// A file outside the workspace that a message names by its full path
// opens from that message; an entry of a zip in the workspace opens as one outside it does.

const STEP = 'stp_01M5PF3QWJN5S3P0JFPVBSFY3R';
const from = { kind: 'assistant' as const, model_step_id: STEP };
const attachmentFile = vi.fn();
const readAttachmentText = vi.fn();
beforeEach(() => {
  resetToolTabsForTests();
  attachmentFile.mockReset().mockResolvedValue({
    ok: true,
    kind: 'text',
    text: '外部报告',
    truncated: false,
    size: 12,
  });
  readAttachmentText.mockReset().mockResolvedValue({
    ok: true,
    value: { text: '共 2 项：\n- 课程/ — 0 B [目录]\n- 课程/说明.md — 9 B' },
  });
  vi.stubGlobal('aura', { v1: { attachmentFile, readAttachmentText } });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const scope = { session, workspace, root, notify: vi.fn() };
const said = (text: string, mentioned = true) =>
  render(
    <FileLinkContext.Provider value={scope}>
      {mentioned ? (
        <MentionContext.Provider value={from}>
          <MarkdownBody text={text} />
        </MentionContext.Provider>
      ) : (
        <MarkdownBody text={text} />
      )}
    </FileLinkContext.Provider>,
  );
const opened = () =>
  toolTabs.get(session).tabs.flatMap((tab) => (tab.kind === 'files' ? [tab.file] : []));

test('a full path outside the workspace in a message opens that file, found through the message', async () => {
  const user = userEvent.setup();
  said(
    '已保存到 `D:\\报告\\notes.md`；不是文件的 `D:\\报告`，工作区里的 `C:\\Dev\\Demo\\src\\main.rs`。',
  );
  await user.click(screen.getByRole('button', { name: '打开 D:\\报告\\notes.md' }));
  const entry = mentionedEntry(from, 'D:\\报告\\notes.md');
  expect(opened()).toEqual([entry]);
  expect(screen.queryByRole('button', { name: '打开 D:\\报告' })).toBeNull();
  // A path in the workspace stays a workspace file's link.
  expect(screen.getByRole('button', { name: '打开 src/main.rs' })).toBeTruthy();

  expect(await readFile({ session, workspace }, entry)).toMatchObject({ kind: 'text' });
  expect(attachmentFile).toHaveBeenLastCalledWith({
    action: 'text',
    session_id: session,
    mentioned_path: 'D:\\报告\\notes.md',
    mentioned_in: from,
  });
});

test('outside a message, a full path stays code', () => {
  said('`D:\\报告\\notes.md`', false);
  expect(screen.queryByRole('button', { name: /^打开 /u })).toBeNull();
});

test('an entry of a zip in the workspace is found by its listing and read by its zip there', async () => {
  const find = zipEntryFinder(
    [{ workspace: '资料/课程.zip', listing: 'art_01M5P7141ZA9PE2D5CMS34V0GC' }],
    session,
  )!;
  const entry = await find('说明.md');
  expect(entry).toBe(workspaceZipEntry('资料/课程.zip', '课程/说明.md'));
  expect(fileSource(entry!)).toEqual({
    kind: 'zipped',
    zip: '资料/课程.zip',
    entry: '课程/说明.md',
    name: '说明.md',
  });
  await readFile({ session, workspace }, entry!);
  expect(attachmentFile).toHaveBeenLastCalledWith({
    action: 'text',
    session_id: session,
    workspace_path: '资料/课程.zip',
    entry: '课程/说明.md',
  });
});

test('a file the user picked is asked of Main by its place', () => {
  const source = fileSource(pickedEntry('C:\\Users\\me\\Downloads\\课程设计.zip'));
  expect(source).toEqual({
    kind: 'picked',
    path: 'C:\\Users\\me\\Downloads\\课程设计.zip',
    name: '课程设计.zip',
  });
  expect(outsideTarget(source, session)).toEqual({
    picked_path: 'C:\\Users\\me\\Downloads\\课程设计.zip',
  });
});
