import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileTree } from './FileTree';
import { fileTrees, resetFileTreesForTests } from './file-tree-store';
import { installFilesApi, root, session, workspace } from './files-fixture';

// A folder or file in a page of its folder not listed yet is brought into the tree at once: the
// rows on its path are read on their own, not by paging through every entry before it.

const scope = { session, workspace, root };
const other = { session: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW', workspace, root };

const entry = (path: string, kind: 'file' | 'directory', ignored = false) => ({
  path,
  name: path.split('/').at(-1)!,
  kind,
  byte_length: kind === 'file' ? 1 : null,
  ignored,
  generated: false,
  reparse_tag: null,
});

let api: ReturnType<typeof installFilesApi>;
let gone = new Set<string>();
let statDelay: Promise<void> | undefined;

/**
 * `paged/` holds `before` files, a page of 1,000 at a time, then the folder `z-target`; an
 * ignored `paged/` stands for a folder the index leaves out.
 */
function workspaceWith(before: number, ignored = false) {
  const files = Array.from({ length: before }, (_, index) =>
    entry(`paged/${String(index).padStart(5, '0')}.txt`, 'file', ignored),
  );
  const ordered = [...files, entry('paged/z-target', 'directory', ignored)];
  api.listWorkspaceFiles.mockImplementation((async (request: {
    target: { path: string };
    after?: string;
  }) => {
    const path = request.target.path;
    if (path === '.')
      return {
        ok: true,
        value: { entries: [entry('paged', 'directory', ignored)], catalog_revision: '1' },
      };
    if (path === 'paged/z-target')
      return {
        ok: true,
        value: {
          entries: [entry('paged/z-target/inside.txt', 'file', ignored)],
          catalog_revision: '1',
        },
      };
    const start = request.after ? ordered.findIndex((item) => item.path === request.after) + 1 : 0;
    const page = ordered.slice(start, start + 1000);
    const more = start + 1000 < ordered.length;
    return {
      ok: true,
      value: {
        entries: page,
        catalog_revision: '1',
        ...(more ? { next_after: page.at(-1)!.path } : {}),
      },
    };
  }) as never);
  api.statWorkspaceFile.mockImplementation((async (request: { target: { path: string } }) => {
    await statDelay;
    const path = request.target.path;
    if (gone.has(path)) return { ok: false, error: { code: 'file.not_found', message: 'gone' } };
    return {
      ok: true,
      value: {
        path,
        kind: path.endsWith('.txt') ? 'file' : 'directory',
        byte_length: null,
        modified_at: null,
        content_hash: null,
        ignored: false,
        generated: false,
        reparse_tag: null,
        active_editor_session: null,
      },
    };
  }) as never);
  api.findWorkspacePaths.mockImplementation((async () => ({
    ok: true,
    value: {
      catalog_revision: '1',
      matches: [{ path: 'paged/z-target', kind: 'directory', score: 1, ignored, generated: false }],
    },
  })) as never);
}

const listed = (path: string, after = false) =>
  api.listWorkspaceFiles.mock.calls.filter(
    ([request]) =>
      (request as { target: { path: string } }).target.path === path &&
      Boolean((request as { after?: string }).after) === after,
  ).length;

const pinned = (where = scope) =>
  fileTrees
    .get(where)
    .folders.get('paged')
    ?.entries?.some((item) => item.path === 'paged/z-target') === true;

/** Lets every answer already given reach the tree. */
const settled = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

beforeEach(() => {
  resetFileTreesForTests();
  api = installFilesApi();
  gone = new Set();
  statDelay = undefined;
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

async function searchAndPick(user: ReturnType<typeof userEvent.setup>) {
  render(<FileTree scope={scope} onPick={vi.fn()} />);
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  await user.type(within(tree).getByRole('searchbox', { name: '筛选文件' }), 'z-target');
  await user.click(await within(tree).findByRole('button', { name: 'paged/z-target' }));
  return tree;
}

/** The first `stat` waits until the returned function is called. */
function holdStat() {
  let answer: () => void = () => undefined;
  statDelay = new Promise((resolve) => (answer = resolve));
  return () => answer();
}

test.each([1_000, 10_000])(
  'a folder after %i entries is shown and opened from the filter, reading only its path',
  async (before) => {
    workspaceWith(before);
    const user = userEvent.setup();
    const tree = await searchAndPick(user);
    const target = await within(tree).findByRole('button', { name: 'z-target' });
    expect(target.getAttribute('aria-expanded')).toBe('true');
    expect(await within(tree).findByRole('button', { name: 'inside.txt' })).toBeTruthy();
    expect((within(tree).getByRole('searchbox') as HTMLInputElement).value).toBe('');
    // One first page of `paged`, no paging through the entries before the target.
    expect(listed('paged')).toBe(1);
    expect(listed('paged', true)).toBe(0);
    expect(api.statWorkspaceFile).toHaveBeenCalledTimes(1);
  },
);

test.each([false, true])(
  '在文件树中显示 brings a file in a later page into the tree and marks it (ignored folder: %s)',
  async (ignored) => {
    workspaceWith(1_000, ignored);
    render(<FileTree scope={scope} onPick={vi.fn()} />);
    const tree = await screen.findByRole('navigation', { name: '文件树' });
    act(() => fileTrees.locate(scope, 'paged/z-target/inside.txt'));
    const file = await within(tree).findByRole('button', { name: 'inside.txt' });
    expect(file.getAttribute('aria-current')).toBe('true');
    const folder = within(tree).getByRole('button', { name: 'z-target' });
    expect(folder.getAttribute('aria-expanded')).toBe('true');
    // Below an ignored folder the row read on its own is ignored too.
    expect(folder.getAttribute('data-ignored')).toBe(ignored ? 'true' : null);
    await vi.waitFor(() => expect(fileTrees.get(scope).locating).toBeUndefined());
    expect(listed('paged', true)).toBe(0);
  },
);

test('the next page after a located folder lists it once and goes on from where it was', async () => {
  workspaceWith(1_000);
  const user = userEvent.setup();
  const tree = await searchAndPick(user);
  await within(tree).findByRole('button', { name: 'inside.txt' });
  // The next-page line is at the end of the folder's thousand lines: scroll there first.
  const list = tree.querySelector<HTMLElement>('.file-tree-list')!;
  list.scrollTop = 1_010 * 24;
  fireEvent.scroll(list);
  await user.click(await within(tree).findByRole('button', { name: '加载更多' }));
  expect(listed('paged', true)).toBe(1);
  await vi.waitFor(() =>
    expect(within(tree).queryByRole('button', { name: '加载更多' })).toBeNull(),
  );
  expect(within(tree).getAllByRole('button', { name: 'z-target' })).toHaveLength(1);
  const rows = fileTrees.get(scope).folders.get('paged')?.entries ?? [];
  expect(rows).toHaveLength(1_001);
  expect(new Set(rows.map((item) => item.path)).size).toBe(1_001);
});

test('a slow locate says so; one whose path is gone says why, and a retry reaches it once it is back', async () => {
  workspaceWith(1_000);
  const answer = holdStat();
  const user = userEvent.setup();
  const tree = await searchAndPick(user);
  expect((await within(tree).findByRole('status')).textContent).toBe('正在定位 z-target…');
  gone.add('paged/z-target');
  await act(async () => answer());
  const alert = await within(tree).findByRole('alert');
  expect(alert.textContent).toContain('没有找到 paged/z-target');
  gone.clear();
  await user.click(within(alert).getByRole('button', { name: '重试' }));
  expect(await within(tree).findByRole('button', { name: 'inside.txt' })).toBeTruthy();
  expect(within(tree).queryByRole('alert')).toBeNull();
});

test('a failed locate offers to pick again from the same filter, or to put the reason away', async () => {
  workspaceWith(1_000);
  gone.add('paged/z-target');
  const user = userEvent.setup();
  const tree = await searchAndPick(user);
  const alert = await within(tree).findByRole('alert');
  await user.click(within(alert).getByRole('button', { name: '重新选择' }));
  expect((within(tree).getByRole('searchbox') as HTMLInputElement).value).toBe('z-target');
  expect(within(tree).queryByRole('alert')).toBeNull();
  await user.click(await within(tree).findByRole('button', { name: 'paged/z-target' }));
  await user.click(
    within(await within(tree).findByRole('alert')).getByRole('button', { name: '关闭' }),
  );
  expect(within(tree).queryByRole('alert')).toBeNull();
  expect(fileTrees.get(scope).located).toBeUndefined();
});

test('a newer locate wins over an older one that answers later', async () => {
  workspaceWith(1_000);
  const answer = holdStat();
  await searchAndPick(userEvent.setup());
  await vi.waitFor(() => expect(api.statWorkspaceFile).toHaveBeenCalledTimes(1));
  statDelay = undefined;
  act(() => fileTrees.locate(scope, 'paged/00000.txt'));
  await act(async () => answer());
  await settled();
  const state = fileTrees.get(scope);
  expect(state.located).toBe('paged/00000.txt');
  expect(state.locating).toBeUndefined();
  expect(pinned()).toBe(false);
});

test('a refresh during a locate stops it: the path neither comes back nor is marked later', async () => {
  workspaceWith(1_000);
  const answer = holdStat();
  const user = userEvent.setup();
  const tree = await searchAndPick(user);
  await within(tree).findByRole('status');
  await user.click(within(tree).getByRole('button', { name: '刷新文件列表' }));
  await act(async () => answer());
  await settled();
  expect(within(tree).queryByRole('status')).toBeNull();
  expect(within(tree).queryByRole('alert')).toBeNull();
  expect(pinned()).toBe(false);
  expect(fileTrees.get(scope).located).toBeUndefined();
  // The folder's next page brings the entry in as an ordinary row.
  await act(() => fileTrees.more(scope, 'paged'));
  expect(pinned()).toBe(true);
  expect(fileTrees.get(scope).located).toBeUndefined();
});

test('a locate answered after the tab moved to another conversation stays in its own tree', async () => {
  workspaceWith(1_000);
  const answer = holdStat();
  const view = render(<FileTree scope={scope} onPick={vi.fn()} />);
  const user = userEvent.setup();
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  await user.type(within(tree).getByRole('searchbox', { name: '筛选文件' }), 'z-target');
  await user.click(await within(tree).findByRole('button', { name: 'paged/z-target' }));
  await vi.waitFor(() => expect(api.statWorkspaceFile).toHaveBeenCalledTimes(1));
  view.rerender(<FileTree scope={other} onPick={vi.fn()} />);
  await within(tree).findByRole('button', { name: 'paged' });
  await act(async () => answer());
  await settled();
  const moved = fileTrees.get(other);
  expect(moved.located).toBeUndefined();
  expect(moved.locating).toBeUndefined();
  expect(moved.expanded.has('paged')).toBe(false);
  expect(within(tree).queryByRole('status')).toBeNull();
  expect(pinned()).toBe(true);
  expect(fileTrees.get(scope).located).toBe('paged/z-target');
});

test('a locate replaced by a newer one still leaves the folders it opened listed', async () => {
  let rootAnswer: () => void = () => undefined;
  const rootHeld = new Promise<void>((resolve) => (rootAnswer = resolve));
  const folders: Record<string, ReturnType<typeof entry>[]> = {
    '.': [entry('deep', 'directory'), entry('paged', 'directory')],
    deep: [entry('deep/inner', 'directory')],
    'deep/inner': [entry('deep/inner/file.txt', 'file')],
    paged: [entry('paged/00000.txt', 'file')],
  };
  api.listWorkspaceFiles.mockImplementation((async (request: { target: { path: string } }) => {
    if (request.target.path === '.') await rootHeld;
    return {
      ok: true,
      value: { entries: folders[request.target.path] ?? [], catalog_revision: '1' },
    };
  }) as never);
  act(() => fileTrees.locate(scope, 'deep/inner/file.txt'));
  act(() => fileTrees.locate(scope, 'paged/00000.txt'));
  render(<FileTree scope={scope} onPick={vi.fn()} />);
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  await act(async () => rootAnswer());
  expect(await within(tree).findByRole('button', { name: 'file.txt' })).toBeTruthy();
  expect(within(tree).queryByText('正在读取…')).toBeNull();
  expect(
    (await within(tree).findByRole('button', { name: '00000.txt' })).getAttribute('aria-current'),
  ).toBe('true');
});
