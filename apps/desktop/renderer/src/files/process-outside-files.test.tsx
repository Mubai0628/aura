import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ToolActivity } from '../agent-session/ToolActivity';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { FileLinkContext } from './file-links';
import { fileSource, mentionedEntry, outsideTarget } from './file-sources';
import { root, session, workspace } from './files-fixture';
import { forgetOutsideConsentsForTests } from './outside-consent';

// A file outside the workspace that a call's row names opens from the row as well, once the user
// says to open it. The answer holds for that file in that conversation while the window is open.

const CALL = 'tcl_01M5PF3QWJN5S3P0JFPVBSFY3R';
const from = { kind: 'tool_call' as const, tool_call_id: CALL };
const attachmentFile = vi.fn();
const writeClipboard = vi.fn();
beforeEach(() => {
  resetToolTabsForTests();
  forgetOutsideConsentsForTests();
  attachmentFile.mockReset().mockResolvedValue({ ok: true, kind: 'opened', message: '已打开' });
  writeClipboard.mockReset().mockResolvedValue({ ok: true, value: { completed: true } });
  vi.stubGlobal('aura', { v1: { attachmentFile, writeClipboard } });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const call = (name: string, target: string, details: string[] = []) =>
  ({
    display: { details, internal: false, summary: '', title: '工具', truncated: false, target },
    summary: {
      can_cancel: false,
      completed_at: null,
      display_name: name,
      ordinal: 1,
      outcome: { type: 'succeeded' },
      side_effect_state: null,
      started_at: null,
      status: 'finalized',
      tool_batch_id: 'tbt_01M5PF3QWJN5S3P0JFPVBSFY3R',
      tool_call_id: CALL,
      tool_id: 'tol_01',
      tool_version: { definition_revision: '1', semantic_version: null },
    },
  }) as unknown as AgentToolActivityEntry;

const row = (entry: AgentToolActivityEntry, conversation = session) =>
  render(
    <FileLinkContext.Provider value={{ session: conversation, workspace, root, notify: vi.fn() }}>
      <ToolActivity entry={entry} />
    </FileLinkContext.Provider>,
  );

const opened = (conversation = session) =>
  toolTabs.get(conversation).tabs.flatMap((tab) => (tab.kind === 'files' ? [tab.file] : []));
const asked = () => screen.queryByRole('dialog', { name: '打开工作区外的文件？' });
const answer = async (user: ReturnType<typeof userEvent.setup>, choice: '打开' | '取消') =>
  user.click(within(asked()!).getByRole('button', { name: choice }));
const menuItem = (label: string) =>
  screen.getAllByRole('menuitem').find((item) => item.textContent?.includes(label))!;

test('a file outside the workspace that a call names asks once, then opens in the 文件 tab', async () => {
  const user = userEvent.setup();
  row(call('media.frames', 'D:\\Videos\\demo.mp4'));
  const link = screen.getByRole('button', { name: '打开 D:\\Videos\\demo.mp4' });
  await user.click(link);
  expect(within(asked()!).getByText('D:\\Videos\\demo.mp4')).toBeTruthy();
  expect(opened()).toEqual([]);
  await answer(user, '打开');
  const entry = mentionedEntry(from, 'D:\\Videos\\demo.mp4');
  expect(asked()).toBeNull();
  expect(opened()).toEqual([entry]);
  // The tab reads it through Main, which asks Core by the call.
  expect(outsideTarget(fileSource(entry), session)).toEqual({
    session_id: session,
    mentioned_path: 'D:\\Videos\\demo.mp4',
    mentioned_in: from,
  });

  resetToolTabsForTests();
  await user.click(link);
  expect(asked()).toBeNull();
  expect(opened()).toEqual([entry]);
});

test('cancelling opens nothing, and the next click asks again', async () => {
  const user = userEvent.setup();
  row(call('fs.read', 'E:\\notes\\plan.md'));
  const link = screen.getByRole('button', { name: '打开 E:\\notes\\plan.md' });
  await user.click(link);
  await answer(user, '取消');
  expect(asked()).toBeNull();
  expect(opened()).toEqual([]);
  await user.click(link);
  expect(asked()).not.toBeNull();
  await user.keyboard('{Escape}');
  expect(asked()).toBeNull();
  expect(opened()).toEqual([]);
});

test('another conversation asks for the same file again', async () => {
  const user = userEvent.setup();
  const first = row(call('http.download', 'D:\\Downloads\\clip.mp4'));
  await user.click(screen.getByRole('button', { name: '打开 D:\\Downloads\\clip.mp4' }));
  await answer(user, '打开');
  first.unmount();
  const other = 'ses_01M5PF3QWJN5S3P0JFPVBSFY3R';
  row(call('http.download', 'D:\\Downloads\\clip.mp4'), other);
  await user.click(screen.getByRole('button', { name: '打开 D:\\Downloads\\clip.mp4' }));
  expect(asked()).not.toBeNull();
  expect(opened(other)).toEqual([]);
});

test('the menu’s opening actions ask first; copying the path does not', async () => {
  const user = userEvent.setup();
  row(call('http.download', 'D:\\Downloads\\setup.zip'));
  const link = screen.getByRole('button', { name: '打开 D:\\Downloads\\setup.zip' });
  fireEvent.contextMenu(link);
  await user.click(menuItem('复制路径'));
  expect(writeClipboard).toHaveBeenCalledWith({ text: 'D:\\Downloads\\setup.zip' });
  expect(asked()).toBeNull();

  fireEvent.contextMenu(link);
  await user.click(menuItem('在文件资源管理器中显示'));
  expect(attachmentFile).not.toHaveBeenCalled();
  await answer(user, '打开');
  const target = {
    session_id: session,
    mentioned_path: 'D:\\Downloads\\setup.zip',
    mentioned_in: from,
  };
  expect(attachmentFile).toHaveBeenLastCalledWith({ ...target, action: 'file_manager' });

  fireEvent.contextMenu(link);
  await user.click(menuItem('用默认应用打开'));
  expect(asked()).toBeNull();
  expect(attachmentFile).toHaveBeenLastCalledWith({ ...target, action: 'default_application' });
});

test('a page outside the workspace that a browser call shows by its file address opens from its row', async () => {
  const user = userEvent.setup();
  for (const [tool, target, path] of [
    ['browser.screenshot', 'file:///D:/Games/game.html', 'D:\\Games\\game.html'],
    ['browser.open', 'file:///D:/%E6%B8%B8%E6%88%8F/index.html?seed=7', 'D:\\游戏\\index.html'],
  ] as const) {
    const shown = row(call(tool, target));
    const link = screen.getByRole('button', { name: `打开 ${path}` });
    expect(link.textContent, tool).toBe(target);
    await user.click(link);
    await answer(user, '打开');
    expect(opened(), tool).toEqual([mentionedEntry(from, path)]);
    shown.unmount();
    resetToolTabsForTests();
  }
});

test('what a lookup or a search found outside the workspace opens from the expanded row once agreed', async () => {
  const user = userEvent.setup();
  for (const [tool, line, path] of [
    ['fs.find', 'D:\\notes\\plan.md', 'D:\\notes\\plan.md'],
    ['fs.search', 'D:\\notes\\todo.md:3 · 第三行', 'D:\\notes\\todo.md'],
  ] as const) {
    const shown = row(call(tool, 'D:\\notes', [line]));
    const details = shown.container.querySelector('details') as HTMLDetailsElement;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await user.click(screen.getByRole('button', { name: `打开 ${path}` }));
    await answer(user, '打开');
    expect(opened(), tool).toEqual([mentionedEntry(from, path)]);
    shown.unmount();
    resetToolTabsForTests();
  }
});
