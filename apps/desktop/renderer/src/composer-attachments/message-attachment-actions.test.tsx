import type { AgentMessageAttachment } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { FileLinkContext } from '../files/file-links';
import { openFile } from '../files/file-menu';
import { carriedByName } from '../files/carried-menu';
import { root, session, workspace } from '../files/files-fixture';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { MessageAttachments } from './MessageAttachments';

// What a message carried opens in the sidebar and has a right-click menu —
// "我发送的消息中预览图、视频等以及各种类型的文件此次需要实现文件的右键菜单功能" — and a file outside the
// workspace the model names by its name alone opens the attachment, not a missing workspace path.

const GRANT = 'xgr_01M5NMVMVN9BY58RT7W730MPES';
const LISTING = 'art_01M5NMX46E2KT4KAXHQ2GCW0KY';
const TEXT = 'art_01M5NMX47G84NTN17N06XFEG7Z';
const PICTURE = 'art_01M5NMX46HWY85MHRGS06BMJ1N';
const VIDEO = 'SaveTik.io_7638307602505436479.mp4';

const notify = vi.fn();
const scope = { session, workspace, root, notify };
const attachmentFile = vi.fn();
const messagePicture = vi.fn();
const readAttachmentText = vi.fn();
const writeClipboard = vi.fn();
const statWorkspaceFile = vi.fn();
const globWorkspacePaths = vi.fn();

beforeEach(() => {
  resetToolTabsForTests();
  notify.mockReset();
  attachmentFile.mockReset().mockResolvedValue({ ok: true, kind: 'opened', message: '' });
  messagePicture.mockReset().mockResolvedValue({ completed: true, message: '已复制图片' });
  readAttachmentText.mockReset().mockResolvedValue({ ok: true, value: { text: '第一行' } });
  writeClipboard
    .mockReset()
    .mockResolvedValue({ ok: true, value: { completed: true, message: '' } });
  statWorkspaceFile.mockReset().mockResolvedValue({ ok: false, error: { code: 'file.not_found' } });
  globWorkspacePaths.mockReset().mockResolvedValue({ ok: true, value: { paths: [] } });
  vi.stubGlobal('aura', {
    v1: {
      attachmentFile,
      messagePicture,
      readAttachmentText,
      writeClipboard,
      statWorkspaceFile,
      globWorkspacePaths,
      browser: {
        readScreenshot: vi.fn(async () => ({
          ok: true,
          value: { media_type: 'image/png', data_base64: 'iVBORw0KGgo=' },
        })),
      },
    },
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const attachments: AgentMessageAttachment[] = [
  {
    kind: 'image',
    name: 'shot.png',
    media_type: 'image/png',
    byte_length: 12,
    artifact_id: PICTURE,
  },
  {
    kind: 'external_file',
    name: VIDEO,
    path: `C:\\Users\\Owner\\Downloads\\${VIDEO}`,
    byte_length: 6_400_000,
    external_grant_id: GRANT,
  },
  { kind: 'text', name: 'notes.md', byte_length: 9, artifact_id: TEXT },
  {
    kind: 'external_file',
    name: 'archive.zip',
    path: 'D:\\Inbox\\archive.zip',
    byte_length: 4096,
    external_grant_id: 'xgr_01M5NMVMVN9BY58RT7W730MPEZ',
    listing_artifact_id: LISTING,
  },
];

const message = () =>
  render(
    <FileLinkContext.Provider value={scope}>
      <MessageAttachments attachments={attachments} sessionId={session} />
    </FileLinkContext.Provider>,
  );
const shownFile = () => toolTabs.get(session).tabs.find((tab) => tab.kind === 'files')?.file;

test('a file card opens what it carried in the sidebar: the outside file, the text, the zip listing', async () => {
  const user = userEvent.setup();
  message();
  await user.click(screen.getByRole('button', { name: `打开 ${VIDEO}` }));
  expect(shownFile()).toBe(`attachment:${GRANT}/${VIDEO}`);
  await user.click(screen.getByRole('button', { name: '打开 notes.md' }));
  expect(shownFile()).toBe(`record:${TEXT}/notes.md`);
  await user.click(screen.getByRole('button', { name: '打开 archive.zip' }));
  expect(shownFile()).toBe(`record:${LISTING}/archive.zip`);
});

test("an outside file's menu opens it outside Aura, shows it in the file manager and copies its path", async () => {
  const user = userEvent.setup();
  message();
  const card = screen.getByRole('button', { name: `打开 ${VIDEO}` });
  fireEvent.contextMenu(card);
  expect(
    screen.getAllByRole('menuitem').map((item) => item.textContent?.replace(/\s+/gu, '')),
  ).toEqual(['在侧边栏打开', '用默认应用打开', '在文件资源管理器中显示', '复制路径']);
  await user.click(screen.getByRole('menuitem', { name: /在文件资源管理器中显示/u }));
  expect(attachmentFile).toHaveBeenLastCalledWith({
    action: 'file_manager',
    session_id: session,
    external_grant_id: GRANT,
  });
  fireEvent.contextMenu(card);
  await user.click(screen.getByRole('menuitem', { name: /复制路径/u }));
  expect(writeClipboard).toHaveBeenLastCalledWith({
    text: attachments[1]!.kind === 'external_file' ? attachments[1]!.path : '',
  });
});

test("a picture's menu previews, copies the picture and saves it where the user chooses", async () => {
  const user = userEvent.setup();
  message();
  const picture = await screen.findByRole('button', { name: '预览 shot.png' });
  fireEvent.contextMenu(picture);
  expect(
    screen.getAllByRole('menuitem').map((item) => item.textContent?.replace(/\s+/gu, '')),
  ).toEqual(['预览', '复制图片', '另存为…']);
  await user.click(screen.getByRole('menuitem', { name: /复制图片/u }));
  expect(messagePicture).toHaveBeenLastCalledWith({
    action: 'copy',
    session_id: session,
    artifact_id: PICTURE,
    name: 'shot.png',
  });
  fireEvent.contextMenu(picture);
  await user.click(screen.getByRole('menuitem', { name: /另存为/u }));
  expect(messagePicture).toHaveBeenLastCalledWith(
    expect.objectContaining({ action: 'save', artifact_id: PICTURE }),
  );
  fireEvent.contextMenu(picture);
  await user.click(screen.getByRole('menuitem', { name: /预览/u }));
  expect(screen.getByRole('dialog', { name: '图片预览' })).toBeTruthy();
});

test('a bare name the model wrote opens the outside file the user attached under that name', async () => {
  const named = carriedByName([{ attachments }]);
  openFile({ ...scope, attachmentNamed: (name) => named.get(name) }, VIDEO);
  await waitFor(() => expect(shownFile()).toBe(`attachment:${GRANT}/${VIDEO}`));
  // A workspace file of that name still wins.
  resetToolTabsForTests();
  statWorkspaceFile.mockResolvedValueOnce({ ok: true, value: {} });
  openFile({ ...scope, attachmentNamed: (name) => named.get(name) }, VIDEO);
  await waitFor(() => expect(shownFile()).toBe(VIDEO));
});
