import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { resetPictureReadsForTests } from '../composer-attachments/picture-reads';
import { FileLinkContext } from '../files/file-links';
import { root, workspace } from '../files/files-fixture';
import { ActivitySession } from './activity-session';
import { ToolActivity } from './ToolActivity';

// The screenshot a browser call took shows under its row in the
// conversation once the row is opened, read from the conversation's own record. Like a picture
// sent with a message, it opens in the viewer and has a right-click menu.

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const IMAGE = 'art_01ARZ3NDEKTSV4RRFFQ69G5FAV';

const shot = (tool = 'browser.screenshot'): AgentToolActivityEntry =>
  ({
    display: {
      details: [],
      internal: false,
      summary: '截图 1280×800 · 已发给模型',
      target: 'http://127.0.0.1:9/game',
      title: '网页截图',
      truncated: false,
      image: IMAGE,
    },
    summary: {
      can_cancel: false,
      completed_at: null,
      display_name: tool,
      ordinal: 1,
      outcome: { type: 'succeeded' },
      side_effect_state: null,
      started_at: null,
      status: 'finalized',
      tool_batch_id: 'tbt_1',
      tool_call_id: 'tcl_01HOSTTOOLCALL0000000001',
      tool_id: 'tol_01',
      tool_version: { definition_revision: '1', semantic_version: null },
    },
  }) as unknown as AgentToolActivityEntry;

const readScreenshot = vi.fn();
const messagePicture = vi.fn();
const notify = vi.fn();
beforeEach(() => {
  resetPictureReadsForTests();
  readScreenshot.mockReset();
  readScreenshot.mockResolvedValue({
    ok: true,
    value: { media_type: 'image/jpeg', data_base64: '/9j/4AAQSkZJRg==' },
  });
  messagePicture.mockReset().mockResolvedValue({ completed: true, message: '已复制图片' });
  notify.mockReset();
  Object.assign(window, { aura: { v1: { browser: { readScreenshot }, messagePicture } } });
});
afterEach(cleanup);

/** The row inside a conversation, opened. */
async function opened(entry: AgentToolActivityEntry) {
  const view = render(
    <FileLinkContext.Provider value={{ session: S, workspace, root, notify }}>
      <ActivitySession.Provider value={S}>
        <ToolActivity entry={entry} />
      </ActivitySession.Provider>
    </FileLinkContext.Provider>,
  );
  const details = view.container.querySelector('details') as HTMLDetailsElement;
  details.open = true;
  fireEvent(details, new Event('toggle'));
  return view;
}

test('a screenshot opens in the viewer, and its menu previews, copies and saves it', async () => {
  const user = userEvent.setup();
  await opened(shot());
  const picture = await screen.findByRole('button', { name: '预览 网页截图' });
  await user.click(picture);
  expect(screen.getByRole('dialog', { name: '图片预览' })).toBeTruthy();
  await user.click(screen.getByRole('button', { name: '关闭图片预览' }));
  expect(screen.queryByRole('dialog', { name: '图片预览' })).toBeNull();
  fireEvent.contextMenu(picture);
  expect(
    screen.getAllByRole('menuitem').map((item) => item.textContent?.replace(/\s+/gu, '')),
  ).toEqual(['预览', '复制图片', '另存为…']);
  await user.click(screen.getByRole('menuitem', { name: /复制图片/u }));
  expect(messagePicture).toHaveBeenLastCalledWith({
    action: 'copy',
    session_id: S,
    artifact_id: IMAGE,
    name: '网页截图',
  });
  fireEvent.contextMenu(picture);
  await user.click(screen.getByRole('menuitem', { name: /另存为/u }));
  expect(messagePicture).toHaveBeenLastCalledWith(
    expect.objectContaining({ action: 'save', artifact_id: IMAGE }),
  );
  fireEvent.contextMenu(picture);
  await user.click(screen.getByRole('menuitem', { name: /预览/u }));
  expect(screen.getByRole('dialog', { name: '图片预览' })).toBeTruthy();
});

test('a screenshot of the desktop or of Chrome is named for what it shows', async () => {
  const desktop = await opened(shot('computer.input'));
  expect(await screen.findByRole('button', { name: '预览 屏幕截图' })).toBeTruthy();
  desktop.unmount();
  await opened(shot('chrome.screenshot'));
  expect(await screen.findByRole('button', { name: '预览 Chrome 截图' })).toBeTruthy();
});

test('a copy the clipboard refused is said in the conversation', async () => {
  const user = userEvent.setup();
  messagePicture.mockResolvedValue({ completed: false, message: '剪贴板暂时不可用' });
  await opened(shot());
  fireEvent.contextMenu(await screen.findByRole('button', { name: '预览 网页截图' }));
  await user.click(screen.getByRole('menuitem', { name: /复制图片/u }));
  await waitFor(() => expect(notify).toHaveBeenCalledWith('剪贴板暂时不可用'));
});

test("a screenshot call's row opens to the picture, read only when opened", async () => {
  const { container } = render(
    <ActivitySession.Provider value={S}>
      <ToolActivity entry={shot()} />
    </ActivitySession.Provider>,
  );
  expect(screen.getByText('网页截图')).toBeTruthy();
  expect(screen.getByText('截图 1280×800 · 已发给模型')).toBeTruthy();
  expect(readScreenshot).not.toHaveBeenCalled();
  const details = container.querySelector('details') as HTMLDetailsElement;
  details.open = true;
  fireEvent(details, new Event('toggle'));
  const image = await waitFor(() => screen.getByRole('img', { name: '网页截图' }));
  expect(image.getAttribute('src')).toBe('data:image/jpeg;base64,/9j/4AAQSkZJRg==');
  expect(readScreenshot).toHaveBeenCalledWith({ session_id: S, artifact_id: IMAGE });
});

test('a screenshot read once opens again without another read', async () => {
  const open = async () => {
    const view = render(
      <ActivitySession.Provider value={S}>
        <ToolActivity entry={shot()} />
      </ActivitySession.Provider>,
    );
    const details = view.container.querySelector('details') as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await waitFor(() => screen.getByRole('img', { name: '网页截图' }));
    return view;
  };
  (await open()).unmount();
  await open();
  expect(readScreenshot).toHaveBeenCalledTimes(1);
});

test('a screenshot that can no longer be read says so', async () => {
  readScreenshot.mockResolvedValue({ ok: false, error: { message: 'gone' } });
  const { container } = render(
    <ActivitySession.Provider value={S}>
      <ToolActivity entry={shot()} />
    </ActivitySession.Provider>,
  );
  const details = container.querySelector('details') as HTMLDetailsElement;
  details.open = true;
  fireEvent(details, new Event('toggle'));
  await waitFor(() => expect(screen.getByRole('status').textContent).toBe('截图不可用'));
});
