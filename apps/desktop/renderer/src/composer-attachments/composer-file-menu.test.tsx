import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { pickedEntry } from '../files/file-sources';
import { root, session, workspace } from '../files/files-fixture';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { ComposerAttachments } from './ComposerAttachments';
import type { ComposerAttachment } from './attachment-files';

// A file added to the
// message has a menu before it is sent.

const notify = vi.fn();
const scope = { session, workspace, root, notify };
const writeClipboard = vi.fn();
beforeEach(() => {
  resetToolTabsForTests();
  writeClipboard.mockReset().mockResolvedValue({ ok: true, value: { completed: true } });
  vi.stubGlobal('aura', { v1: { writeClipboard } });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const files: ComposerAttachment[] = [
  { id: 'a', kind: 'file', name: 'main.rs', path: `${root}\\src\\main.rs`, bytes: 10 },
  {
    id: 'b',
    kind: 'file',
    name: '课程设计.zip',
    path: 'C:\\Users\\me\\Downloads\\课程设计.zip',
    bytes: 7_000_000,
  },
  { id: 'c', kind: 'text', name: '粘贴的文本.txt', text: '第一行\n第二行', bytes: 18 },
  { id: 'd', kind: 'file', name: '日志.log', path: 'C:\\Users\\me\\日志.log', bytes: 900_000 },
];

const menuOf = (name: string) => {
  fireEvent.contextMenu(screen.getByTitle(name));
  return screen.getByRole('menu', { name: new RegExp(`的操作$`, 'u') });
};

test("a workspace file has a workspace path's menu, and can be taken out", async () => {
  const user = userEvent.setup();
  const remove = vi.fn();
  render(<ComposerAttachments attachments={files} onRemove={remove} scope={scope} />);
  const menu = menuOf(`${root}\\src\\main.rs`);
  const items = within(menu)
    .getAllByRole('menuitem')
    .map((item) => item.textContent);
  expect(items).toEqual(
    expect.arrayContaining(['在侧边栏打开', '在文件资源管理器中显示', '复制相对路径', '移除']),
  );
  await user.click(within(menu).getByRole('menuitem', { name: '移除' }));
  expect(remove).toHaveBeenCalledWith('a');
});

// A file from outside the
// workspace opens too, in the sidebar when it shows that kind of file.
test('a file from outside the workspace opens and copies its path; text copies itself', async () => {
  const user = userEvent.setup();
  render(<ComposerAttachments attachments={files} onRemove={vi.fn()} scope={scope} />);
  let menu = menuOf('C:\\Users\\me\\Downloads\\课程设计.zip');
  expect(
    within(menu)
      .getAllByRole('menuitem')
      .map((item) => item.textContent),
  ).toEqual(['用默认应用打开', '在文件资源管理器中显示', '复制路径', '移除']);
  await user.click(within(menu).getByRole('menuitem', { name: '复制路径' }));
  expect(writeClipboard).toHaveBeenLastCalledWith({
    text: 'C:\\Users\\me\\Downloads\\课程设计.zip',
  });
  menu = menuOf('粘贴的文本.txt');
  await user.click(within(menu).getByRole('menuitem', { name: '复制内容' }));
  expect(writeClipboard).toHaveBeenLastCalledWith({ text: '第一行\n第二行' });
});

test('before a conversation exists, a file still copies its path and can be taken out', () => {
  render(<ComposerAttachments attachments={files.slice(0, 1)} onRemove={vi.fn()} />);
  const menu = menuOf(`${root}\\src\\main.rs`);
  expect(
    within(menu)
      .getAllByRole('menuitem')
      .map((item) => item.textContent),
  ).toEqual(['复制路径', '移除']);
});

test('a text file from outside the workspace opens in the sidebar', async () => {
  const user = userEvent.setup();
  render(<ComposerAttachments attachments={files} onRemove={vi.fn()} scope={scope} />);
  const menu = menuOf('C:\\Users\\me\\日志.log');
  await user.click(within(menu).getByRole('menuitem', { name: '在侧边栏打开' }));
  expect(
    toolTabs.get(session).tabs.flatMap((tab) => (tab.kind === 'files' ? [tab.file] : [])),
  ).toEqual([pickedEntry('C:\\Users\\me\\日志.log')]);
});

test('a text file read into the message still opens from its card, and copies its text', async () => {
  const user = userEvent.setup();
  const note: ComposerAttachment = {
    id: 'e',
    kind: 'text',
    name: '笔记.md',
    text: '笔记正文',
    bytes: 12,
    path: 'C:\\Users\\me\\笔记.md',
  };
  render(<ComposerAttachments attachments={[note]} onRemove={vi.fn()} scope={scope} />);
  const menu = menuOf('笔记.md');
  expect(
    within(menu)
      .getAllByRole('menuitem')
      .map((item) => item.textContent),
  ).toEqual([
    '在侧边栏打开',
    '用默认应用打开',
    '在文件资源管理器中显示',
    '复制路径',
    '复制内容',
    '移除',
  ]);
  await user.click(within(menu).getByRole('menuitem', { name: '在侧边栏打开' }));
  expect(
    toolTabs.get(session).tabs.flatMap((tab) => (tab.kind === 'files' ? [tab.file] : [])),
  ).toEqual([pickedEntry('C:\\Users\\me\\笔记.md')]);
});
