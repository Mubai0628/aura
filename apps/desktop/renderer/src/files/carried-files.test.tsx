import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { FileView } from './FileView';
import { fileSource } from './file-sources';
import { root, session, workspace } from './files-fixture';

// The 文件 tab shows what a message carried, and its
// pictures open the full-window viewer.

const GRANT = 'xgr_01M5NMVMVN9BY58RT7W730MPES';
const TEXT = 'art_01M5NMX47G84NTN17N06XFEG7Z';
const scope = { session, workspace, root };
const attachmentFile = vi.fn();
const readAttachmentText = vi.fn();
const workspaceMediaUrl = vi.fn();

beforeEach(() => {
  attachmentFile.mockReset();
  readAttachmentText.mockReset();
  workspaceMediaUrl.mockReset();
  vi.stubGlobal('aura', { v1: { attachmentFile, readAttachmentText, workspaceMediaUrl } });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test('entries keep workspace paths apart from what a message carried', () => {
  expect(fileSource('src/main.rs')).toEqual({ kind: 'workspace', path: 'src/main.rs' });
  expect(fileSource(`attachment:${GRANT}/clip.mp4`)).toEqual({
    kind: 'attachment',
    grant: GRANT,
    name: 'clip.mp4',
  });
  expect(fileSource(`record:${TEXT}/notes.md`)).toEqual({
    kind: 'record',
    artifact: TEXT,
    name: 'notes.md',
  });
  // Anything else stays a workspace path, which Core judges.
  expect(fileSource('attachment:grant/clip.mp4').kind).toBe('workspace');
  expect(fileSource(`attachment:${GRANT}/a/b.mp4`).kind).toBe('workspace');
});

test('an outside video plays from the address Main gives, under its own name', async () => {
  attachmentFile.mockResolvedValue({
    ok: true,
    kind: 'media',
    url: 'aura://app/workspace-media/7d9c4f0e-2f0e-4c39-9d6c-2f0f6b0a1c2d/clip.mp4',
  });
  render(<FileView path={`attachment:${GRANT}/clip.mp4`} scope={scope} visible />);
  await waitFor(() =>
    expect(document.querySelector('video')?.getAttribute('src')).toBe(
      'aura://app/workspace-media/7d9c4f0e-2f0e-4c39-9d6c-2f0f6b0a1c2d/clip.mp4',
    ),
  );
  expect(attachmentFile).toHaveBeenCalledWith({
    action: 'media',
    session_id: session,
    external_grant_id: GRANT,
  });
  expect(screen.getByTitle('clip.mp4').textContent).toBe('clip.mp4');
});

test('an outside file that is gone says so in the user words Main gives', async () => {
  attachmentFile.mockResolvedValue({
    ok: false,
    message: '找不到这个附件，它可能已被移动、删除或替换。',
  });
  render(<FileView path={`attachment:${GRANT}/clip.mp4`} scope={scope} visible />);
  expect(await screen.findByText('找不到这个附件，它可能已被移动、删除或替换。')).toBeTruthy();
});

test('outside text shows its beginning, and says when it was cut', async () => {
  attachmentFile.mockResolvedValue({
    ok: true,
    kind: 'text',
    text: 'line one\nline two',
    truncated: true,
    size: 900_000,
  });
  render(<FileView path={`attachment:${GRANT}/server.log`} scope={scope} visible />);
  expect(await screen.findByText('line two')).toBeTruthy();
  expect(screen.getByText('只显示了开头的 256 KB。')).toBeTruthy();
  expect(attachmentFile).toHaveBeenCalledWith(expect.objectContaining({ action: 'text' }));
});

test('a text file a message kept reads back from the conversation', async () => {
  readAttachmentText.mockResolvedValue({ ok: true, value: { text: '第一行\n第二行' } });
  render(<FileView path={`record:${TEXT}/todo.txt`} scope={scope} visible />);
  expect(await screen.findByText('第二行')).toBeTruthy();
  expect(readAttachmentText).toHaveBeenCalledWith({ session_id: session, artifact_id: TEXT });
});

test("a picture in the sidebar opens the composer's full-window viewer", async () => {
  const user = userEvent.setup();
  workspaceMediaUrl.mockResolvedValue({
    ok: true,
    url: 'aura://app/workspace-media/7d9c4f0e-2f0e-4c39-9d6c-2f0f6b0a1c2d/logo.png',
  });
  render(<FileView path="logo.png" scope={scope} visible />);
  await user.click(await screen.findByRole('button', { name: '预览 logo.png' }));
  expect(screen.getByRole('dialog', { name: '图片预览' })).toBeTruthy();
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('dialog', { name: '图片预览' })).toBeNull();
});
