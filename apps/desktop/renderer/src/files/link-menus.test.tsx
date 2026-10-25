import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileLinkContext } from './file-links';
import { root, session, workspace } from './files-fixture';
import { MarkdownBody } from '../markdown-body';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';

// A path in the conversation opens by its kind — text, Markdown and pictures
// in the sidebar, a program or a script never run (shown in the file manager), anything else with
// its default application — and a right-click offers the rest. A web link opens in the browser.

const notify = vi.fn();
const scope = { session, workspace, root, notify };
const openOutside = vi.fn();
const openLink = vi.fn();
// Copies go to Main's clipboard; the page's own is refused by Main.
const writeClipboard = vi.fn();
beforeEach(() => {
  resetToolTabsForTests();
  notify.mockReset();
  openOutside.mockReset().mockResolvedValue({ ok: true, value: { completed: true, message: '' } });
  openLink.mockReset().mockResolvedValue({ ok: true, value: { completed: true, message: '' } });
  writeClipboard
    .mockReset()
    .mockResolvedValue({ ok: true, value: { completed: true, message: '已复制' } });
  vi.stubGlobal('aura', {
    v1: { openWorkspaceFileOutside: openOutside, openExternalLink: openLink, writeClipboard },
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const message = (text: string) =>
  render(
    <FileLinkContext.Provider value={scope}>
      <MarkdownBody text={text} />
    </FileLinkContext.Provider>,
  );
const asked = (path: string, opener: string) => ({
  workspace_id: workspace,
  session_id: session,
  path,
  opener,
});

test('a click opens a path by its kind', async () => {
  const user = userEvent.setup();
  message('看 `src/main.rs`、`docs/plan.pdf` 和 `tools/setup.exe`。');
  await user.click(screen.getByRole('button', { name: '打开 src/main.rs' }));
  expect(toolTabs.get(session).tabs).toEqual([
    expect.objectContaining({ kind: 'files', file: 'src/main.rs' }),
  ]);
  expect(openOutside).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: '打开 docs/plan.pdf' }));
  expect(openOutside).toHaveBeenLastCalledWith(asked('docs/plan.pdf', 'default_application'));
  await user.click(screen.getByRole('button', { name: '打开 tools/setup.exe' }));
  expect(openOutside).toHaveBeenLastCalledWith(asked('tools/setup.exe', 'file_manager'));
});

test('a path’s menu opens it outside Aura, shows it, finds it in the tree and copies it', async () => {
  const user = userEvent.setup();
  message('改了 `src/main.rs`。');
  const open = () => {
    fireEvent.contextMenu(screen.getByRole('button', { name: '打开 src/main.rs' }));
    return screen.getByRole('menu', { name: 'src/main.rs 的操作' });
  };
  expect(
    within(open())
      .getAllByRole('menuitem')
      .map((item) => item.textContent),
  ).toEqual([
    '在侧边栏打开',
    '用默认应用打开',
    '在文件资源管理器中显示',
    '在文件树中显示',
    '复制路径',
    '复制相对路径',
  ]);
  await user.click(
    within(screen.getByRole('menu')).getByRole('menuitem', { name: '用默认应用打开' }),
  );
  expect(openOutside).toHaveBeenLastCalledWith(asked('src/main.rs', 'default_application'));
  await user.click(within(open()).getByRole('menuitem', { name: '在文件资源管理器中显示' }));
  expect(openOutside).toHaveBeenLastCalledWith(asked('src/main.rs', 'file_manager'));
  await user.click(within(open()).getByRole('menuitem', { name: '复制路径' }));
  expect(writeClipboard).toHaveBeenLastCalledWith({ text: 'C:\\Dev\\Demo\\src\\main.rs' });
});

test('a script is never run: its menu says so, and it still opens in the sidebar to read', async () => {
  const user = userEvent.setup();
  message('运行 `scripts/build.bat`。');
  fireEvent.contextMenu(screen.getByRole('button', { name: '打开 scripts/build.bat' }));
  const run = screen.getByRole('menuitem', { name: /用默认应用打开/u });
  expect(run.getAttribute('aria-disabled')).toBe('true');
  expect(run.textContent).toContain('Aura 不直接运行可执行文件或脚本');
  await user.click(run);
  expect(openOutside).not.toHaveBeenCalled();
  await user.click(screen.getByRole('menuitem', { name: '在侧边栏打开' }));
  expect(toolTabs.get(session).tabs).toEqual([
    expect.objectContaining({ kind: 'files', file: 'scripts/build.bat' }),
  ]);
});

test('what Main refuses is said', async () => {
  const user = userEvent.setup();
  openOutside.mockResolvedValue({
    ok: true,
    value: { completed: false, message: '找不到这个文件，它可能已被移动或删除。' },
  });
  message('见 `docs/plan.pdf`。');
  await user.click(screen.getByRole('button', { name: '打开 docs/plan.pdf' }));
  await vi.waitFor(() =>
    expect(notify).toHaveBeenCalledWith('找不到这个文件，它可能已被移动或删除。'),
  );
});

test('a web link opens in the browser and copies; other addresses stay text', async () => {
  const user = userEvent.setup();
  message('参见[文档](https://example.com/docs)和[信箱](mailto:someone@example.com)。');
  const link = screen.getByRole('link', { name: '文档' });
  await user.click(link);
  expect(openLink).toHaveBeenCalledWith({ url: 'https://example.com/docs' });
  fireEvent.contextMenu(link);
  await user.click(screen.getByRole('menuitem', { name: '复制链接' }));
  expect(writeClipboard).toHaveBeenLastCalledWith({ text: 'https://example.com/docs' });
  expect(screen.queryByRole('link', { name: '信箱' })).toBeNull();
  cleanup();
  // Outside a conversation a link stays text.
  render(<MarkdownBody text="参见[文档](https://example.com/docs)。" />);
  expect(screen.queryByRole('link')).toBeNull();
});
