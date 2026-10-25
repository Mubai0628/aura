import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FilesPanel } from './FilesPanel';
import { installFilesApi, root, session, workspace } from './files-fixture';
import { fileTrees, resetFileTreesForTests } from './file-tree-store';
import { resetToolTabsForTests, toolTabs, useToolTabs } from '../right-workspace/tool-tabs';

// The 文件 tab, read only. The workspace tree opens folders as asked, lists
// folders first and dims what Git ignores; a filter finds files by name; a file picked in an
// empty tab opens there, and the tab takes the file's name.

function Harness() {
  const state = useToolTabs(session);
  const tab = state.tabs.find((item) => item.id === state.active && item.kind === 'files');
  return tab ? (
    <FilesPanel
      key={tab.id}
      scope={{ session, workspace, root, notify: vi.fn() }}
      tab={tab}
      visible
    />
  ) : null;
}

let api: ReturnType<typeof installFilesApi>;
beforeEach(() => {
  resetToolTabsForTests();
  resetFileTreesForTests();
  api = installFilesApi();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const rows = (tree: HTMLElement) =>
  [...tree.querySelectorAll('.file-tree-row')].map((row) => row.textContent);

test('an empty 文件 tab shows the tree, folders first, and opens the file picked in place', async () => {
  const user = userEvent.setup();
  const tab = toolTabs.open(session, 'files');
  const view = render(<Harness />);
  // With no file open the tree has the whole page.
  expect(view.container.querySelector('.files-layout')?.getAttribute('data-view')).toBe('none');
  expect(view.container.querySelector('.file-view')).toBeNull();
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  await within(tree).findByRole('button', { name: 'src' });
  expect(rows(tree)).toEqual(['src', 'target', 'data.bin', 'logo.png', 'README.md']);
  expect(api.listWorkspaceFiles).toHaveBeenCalledWith({
    session_id: session,
    workspace_id: workspace,
    target: { path: '.' },
  });
  expect(within(tree).getByRole('button', { name: 'target' }).dataset.ignored).toBe('true');
  const folder = within(tree).getByRole('button', { name: 'src' });
  expect(folder.getAttribute('aria-expanded')).toBe('false');
  await user.click(folder);
  expect(folder.getAttribute('aria-expanded')).toBe('true');
  await user.click(await within(tree).findByRole('button', { name: 'main.rs' }));
  expect(toolTabs.get(session).tabs).toEqual([
    expect.objectContaining({
      id: tab,
      kind: 'files',
      file: 'src/main.rs',
      files: ['src/main.rs'],
    }),
  ]);
});

// Files open beside the tree as the page's own small tabs, each closed on its
// own; "在文件树中打开的文件要在文件树体现出来我打开的是什么文件" — the tree marks the file shown.
test('open files are small tabs beside the tree, each closed on its own, the shown one marked in the tree', async () => {
  const user = userEvent.setup();
  act(() => {
    toolTabs.openFile(session, 'README.md');
    toolTabs.openFile(session, 'src/main.rs');
  });
  const view = render(<Harness />);
  const opened = screen.getByRole('tablist', { name: '打开的文件' });
  const names = () =>
    within(opened)
      .getAllByRole('tab')
      .map((item) => item.textContent);
  expect(names()).toEqual(['README.md', 'main.rs']);
  expect(within(opened).getByRole('tab', { name: 'main.rs' }).getAttribute('aria-selected')).toBe(
    'true',
  );
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  // The shown file's folders open, and its row is marked.
  const shown = await within(tree).findByRole('button', { name: 'main.rs' });
  expect(shown.dataset.open).toBe('true');
  expect(within(tree).getByRole('button', { name: 'README.md' }).dataset.open).toBeUndefined();
  await user.click(within(opened).getByRole('tab', { name: 'README.md' }));
  expect(within(tree).getByRole('button', { name: 'README.md' }).dataset.open).toBe('true');
  // A small tab's menu is the file's.
  fireEvent.contextMenu(within(opened).getByRole('tab', { name: 'README.md' }));
  expect(screen.getByRole('menuitem', { name: '在文件树中显示' })).toBeTruthy();
  await user.keyboard('{Escape}');
  await user.click(within(opened).getByRole('button', { name: '关闭 README.md' }));
  expect(names()).toEqual(['main.rs']);
  await user.click(within(opened).getByRole('button', { name: '关闭 main.rs' }));
  expect(screen.queryByRole('tablist', { name: '打开的文件' })).toBeNull();
  expect(view.container.querySelector('.files-layout')?.getAttribute('data-view')).toBe('none');
});

test('the tree hides and shows with one button, and stays as it was', async () => {
  const user = userEvent.setup();
  // The button is beside an open file; with none open the tree is the whole page.
  act(() => {
    toolTabs.openFile(session, 'README.md');
  });
  render(<Harness />);
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  await user.click(await within(tree).findByRole('button', { name: 'src' }));
  await user.click(screen.getByRole('button', { name: '隐藏文件树' }));
  expect(screen.queryByRole('navigation', { name: '文件树' })).toBeNull();
  await user.click(screen.getByRole('button', { name: '显示文件树' }));
  const again = screen.getByRole('navigation', { name: '文件树' });
  expect(within(again).getByRole('button', { name: 'src' }).getAttribute('aria-expanded')).toBe(
    'true',
  );
  expect(api.listWorkspaceFiles).toHaveBeenCalledTimes(2);
});

test('the filter finds files by name and opens the one picked', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
  toolTabs.open(session, 'files');
  render(<Harness />);
  await user.type(screen.getByRole('searchbox', { name: '筛选文件' }), 'main');
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300);
  });
  expect(api.findWorkspacePaths).toHaveBeenLastCalledWith({
    session_id: session,
    workspace_id: workspace,
    query: 'main',
    limit: 100,
  });
  await user.click(await screen.findByRole('button', { name: 'src/main.rs' }));
  expect(toolTabs.get(session).tabs[0]).toMatchObject({ file: 'src/main.rs' });
});

test('while a new workspace is still indexed, its listing is asked again', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const indexing = { ok: false, error: { code: 'workspace.catalog_unavailable', message: 'x' } };
  api.listWorkspaceFiles
    .mockResolvedValueOnce(indexing as never)
    .mockResolvedValueOnce(indexing as never);
  toolTabs.open(session, 'files');
  render(<Harness />);
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2500);
  });
  expect(await within(tree).findByRole('button', { name: 'src' })).toBeTruthy();
  expect(screen.queryByRole('alert')).toBeNull();
  expect(api.listWorkspaceFiles).toHaveBeenCalledTimes(3);
});

test('a folder that cannot be listed says so and can be listed again', async () => {
  const user = userEvent.setup();
  api.listWorkspaceFiles.mockResolvedValueOnce({
    ok: false,
    error: { code: 'file.not_found', message: 'gone' },
  } as never);
  toolTabs.open(session, 'files');
  render(<Harness />);
  expect((await screen.findByRole('alert')).textContent).toContain('文件列表暂时读不出来');
  await user.click(screen.getByRole('button', { name: '重新读取文件列表' }));
  const tree = screen.getByRole('navigation', { name: '文件树' });
  expect(await within(tree).findByRole('button', { name: 'src' })).toBeTruthy();
});

// A file's right-click menu; 在文件树中显示 opens its folders and marks it.
test('a file row’s menu copies its paths, and a located file is marked in the tree', async () => {
  const user = userEvent.setup();
  const { writeClipboard } = window.aura.v1 as unknown as {
    writeClipboard: ReturnType<typeof vi.fn>;
  };
  act(() => toolTabs.open(session, 'files'));
  render(<Harness />);
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  fireEvent.contextMenu(await within(tree).findByRole('button', { name: 'README.md' }));
  await user.click(screen.getByRole('menuitem', { name: '复制路径' }));
  expect(writeClipboard).toHaveBeenLastCalledWith({ text: 'C:\\Dev\\Demo\\README.md' });
  fireEvent.contextMenu(within(tree).getByRole('button', { name: 'README.md' }));
  await user.click(screen.getByRole('menuitem', { name: '复制相对路径' }));
  expect(writeClipboard).toHaveBeenLastCalledWith({ text: 'README.md' });
  act(() => fileTrees.locate({ session, workspace, root }, 'src/main.rs'));
  const located = await within(tree).findByRole('button', { name: 'main.rs' });
  expect(located.getAttribute('aria-current')).toBe('true');
});

// "文件树与文件预览页中间的分页栏也能适度控制调整两侧区域占用的比例".
test('the line between the tree and the open file moves both ways and is remembered', async () => {
  const user = userEvent.setup();
  window.localStorage.removeItem('aura.ui.v1.files-tree-width');
  act(() => {
    toolTabs.openFile(session, 'README.md');
  });
  const view = render(<Harness />);
  await screen.findByRole('navigation', { name: '文件树' });
  const line = screen.getByRole('separator', { name: '调整文件树宽度' });
  const before = Number(line.getAttribute('aria-valuenow'));
  line.focus();
  await user.keyboard('{ArrowRight}');
  expect(Number(line.getAttribute('aria-valuenow'))).toBe(before + 16);
  await user.keyboard('{ArrowLeft}{ArrowLeft}');
  expect(Number(line.getAttribute('aria-valuenow'))).toBe(before - 16);
  expect(window.localStorage.getItem('aura.ui.v1.files-tree-width')).toBe(String(before - 16));
  const layout = view.container.querySelector('.files-layout') as HTMLElement;
  expect(layout.style.getPropertyValue('--files-tree-width')).toBe(`${before - 16}px`);
  // The next page opens with the width it was left at.
  view.unmount();
  render(<Harness />);
  expect(
    Number(screen.getByRole('separator', { name: '调整文件树宽度' }).getAttribute('aria-valuenow')),
  ).toBe(before - 16);
});

test('with no file open there is no line to move', async () => {
  act(() => {
    toolTabs.open(session, 'files');
  });
  render(<Harness />);
  await screen.findByRole('navigation', { name: '文件树' });
  expect(screen.queryByRole('separator', { name: '调整文件树宽度' })).toBeNull();
});
