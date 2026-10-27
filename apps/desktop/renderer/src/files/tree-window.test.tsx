import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileTree } from './FileTree';
import { fileTrees, resetFileTreesForTests } from './file-tree-store';
import { installFilesApi, root, session, workspace } from './files-fixture';

// However many entries a folder holds, the tree draws the lines near its view: a screen above,
// the view and a screen below, plus the lines being worked with — at most
// 3 × ⌈view height / line height⌉ + 10 lines. jsdom lays nothing out, so the list's height is
// given here and its resize reported by hand.

const scope = { session, workspace, root };
/** The tree's line height, as files.css draws it. */
const LINE = 24;
const bound = (height: number) => 3 * Math.ceil(height / LINE) + 10;

const file = (index: number) => ({
  path: `big/${String(index).padStart(5, '0')}.txt`,
  name: `${String(index).padStart(5, '0')}.txt`,
  kind: 'file' as const,
  byte_length: 1,
  ignored: true,
  generated: false,
  reparse_tag: null,
});

let api: ReturnType<typeof installFilesApi>;
let height = 640;
let resized: (() => void) | undefined;

/** `big/` read page by page until `loaded` entries are there. */
async function bigFolder(loaded: number) {
  const all = Array.from({ length: 10_001 }, (_, index) => file(index));
  api.listWorkspaceFiles.mockImplementation((async (request: {
    target: { path: string };
    after?: string;
  }) => {
    if (request.target.path === '.')
      return {
        ok: true,
        value: {
          entries: [{ ...file(0), path: 'big', name: 'big', kind: 'directory', byte_length: null }],
          catalog_revision: '1',
        },
      };
    const start = request.after ? all.findIndex((item) => item.path === request.after) + 1 : 0;
    const page = all.slice(start, start + 1000);
    return {
      ok: true,
      value: {
        entries: page,
        catalog_revision: '1',
        ...(start + 1000 < all.length ? { next_after: page.at(-1)!.path } : {}),
      },
    };
  }) as never);
  await fileTrees.load(scope, '.');
  fileTrees.toggle(scope, 'big');
  await vi.waitFor(() => expect(fileTrees.get(scope).folders.get('big')?.entries).toBeTruthy());
  while ((fileTrees.get(scope).folders.get('big')?.entries?.length ?? 0) < loaded)
    await fileTrees.more(scope, 'big');
}

async function shown(onPick = vi.fn(), onMenu?: (event: unknown, path: string) => void) {
  const view = render(<FileTree onMenu={onMenu} onPick={onPick} scope={scope} />);
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  await within(tree).findByRole('button', { name: 'big' });
  return { tree, view };
}

/** Every line drawn: entries and notes alike. */
const drawn = (tree: HTMLElement) => tree.querySelectorAll('li').length;
const list = (tree: HTMLElement) => tree.querySelector<HTMLElement>('.file-tree-list')!;

function scrollTo(tree: HTMLElement, line: number) {
  list(tree).scrollTop = line * LINE;
  fireEvent.scroll(list(tree));
}

/** Whether the line at `index` lies in the list's view. */
function inView(tree: HTMLElement, index: number) {
  const top = list(tree).scrollTop;
  return index * LINE >= top && (index + 1) * LINE <= top + height;
}

beforeEach(() => {
  resetFileTreesForTests();
  api = installFilesApi();
  height = 640;
  resized = undefined;
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.classList.contains('file-tree-list') ? height : 0;
  });
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resized = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test.each([1_000, 5_000, 10_001])(
  'with %i entries loaded the lines drawn stay within one bound, and every entry is kept once',
  async (loaded) => {
    await bigFolder(loaded);
    const { tree } = await shown();
    expect(drawn(tree)).toBeLessThanOrEqual(bound(640));
    scrollTo(tree, Math.floor(loaded / 2));
    expect(drawn(tree)).toBeLessThanOrEqual(bound(640));
    expect(
      within(tree).getByRole('button', { name: file(Math.floor(loaded / 2)).name }),
    ).toBeTruthy();
    const entries = fileTrees.get(scope).folders.get('big')?.entries ?? [];
    expect(entries).toHaveLength(loaded);
    expect(new Set(entries.map((entry) => entry.path)).size).toBe(loaded);
  },
);

test('scrolled to the end the last entry is drawn, and a click or menu on a line names its own path', async () => {
  await bigFolder(10_001);
  const onPick = vi.fn();
  const onMenu = vi.fn();
  const { tree } = await shown(onPick, onMenu);
  scrollTo(tree, 10_002);
  const last = await within(tree).findByRole('button', { name: '10000.txt' });
  expect(within(tree).queryByRole('button', { name: '00000.txt' })).toBeNull();
  const user = userEvent.setup();
  await user.click(last);
  expect(onPick).toHaveBeenLastCalledWith('big/10000.txt');
  scrollTo(tree, 5_000);
  const middle = await within(tree).findByRole('button', { name: '04999.txt' });
  await user.click(middle);
  expect(onPick).toHaveBeenLastCalledWith('big/04999.txt');
  fireEvent.contextMenu(middle);
  expect(onMenu).toHaveBeenLastCalledWith(expect.anything(), 'big/04999.txt');
  scrollTo(tree, 0);
  await user.click(await within(tree).findByRole('button', { name: '00000.txt' }));
  expect(onPick).toHaveBeenLastCalledWith('big/00000.txt');
});

test('a focused line stays drawn and focused while the list scrolls away, and Tab still reaches its neighbours', async () => {
  await bigFolder(5_000);
  const { tree } = await shown();
  const first = within(tree).getByRole('button', { name: '00003.txt' });
  act(() => first.focus());
  scrollTo(tree, 4_000);
  expect(within(tree).getByRole('button', { name: '00003.txt' })).toBe(first);
  expect(document.activeElement).toBe(first);
  expect(drawn(tree)).toBeLessThanOrEqual(bound(640));
  const user = userEvent.setup();
  await user.tab({ shift: true });
  expect(document.activeElement?.getAttribute('title')).toBe('big/00002.txt');
  await user.tab();
  await user.tab();
  expect(document.activeElement?.getAttribute('title')).toBe('big/00004.txt');
  expect(drawn(tree)).toBeLessThanOrEqual(bound(640));
});

test('a located entry far down is drawn and brought into view, again when asked again', async () => {
  await bigFolder(10_001);
  const { tree } = await shown();
  act(() => fileTrees.locate(scope, 'big/07777.txt'));
  const located = await within(tree).findByRole('button', { name: '07777.txt' });
  expect(located.getAttribute('aria-current')).toBe('true');
  // Line 0 is `big`, so the entry 07777 is line 7,778.
  await vi.waitFor(() => expect(inView(tree, 7_778)).toBe(true));
  scrollTo(tree, 0);
  expect(inView(tree, 7_778)).toBe(false);
  act(() => fileTrees.locate(scope, 'big/07777.txt'));
  await vi.waitFor(() => expect(inView(tree, 7_778)).toBe(true));
  expect(drawn(tree)).toBeLessThanOrEqual(bound(640));
});

test('the last entry is located while only the first page is read, then drawn, marked and opened', async () => {
  await bigFolder(1_000);
  api.statWorkspaceFile.mockImplementation((async (request: { target: { path: string } }) => ({
    ok: true,
    value: {
      path: request.target.path,
      kind: 'file',
      byte_length: 1,
      modified_at: null,
      content_hash: null,
      ignored: false,
      generated: false,
      reparse_tag: null,
      active_editor_session: null,
    },
  })) as never);
  const onPick = vi.fn();
  const { tree } = await shown(onPick);
  act(() => fileTrees.locate(scope, 'big/10000.txt'));
  const last = await within(tree).findByRole('button', { name: '10000.txt' });
  expect(last.getAttribute('aria-current')).toBe('true');
  // Line 0 is `big`, its first page lines 1–1,000, and the located entry comes next.
  await vi.waitFor(() => expect(inView(tree, 1_001)).toBe(true));
  expect(fileTrees.get(scope).folders.get('big')?.entries).toHaveLength(1_001);
  expect(
    api.listWorkspaceFiles.mock.calls.filter(([request]) => (request as { after?: string }).after),
  ).toHaveLength(0);
  await userEvent.setup().click(last);
  expect(onPick).toHaveBeenLastCalledWith('big/10000.txt');
});

test('the open file far down is drawn, marked and brought into view', async () => {
  await bigFolder(10_001);
  const { tree, view } = await shown();
  view.rerender(<FileTree current="big/09000.txt" onPick={vi.fn()} scope={scope} />);
  const open = await within(tree).findByRole('button', { name: '09000.txt' });
  expect(open.getAttribute('data-open')).toBe('true');
  await vi.waitFor(() => expect(inView(tree, 9_001)).toBe(true));
  expect(drawn(tree)).toBeLessThanOrEqual(bound(640));
});

test('the lines drawn follow the list as it is resized', async () => {
  await bigFolder(10_001);
  const { tree } = await shown();
  height = 240;
  act(() => resized?.());
  expect(drawn(tree)).toBeLessThanOrEqual(bound(240));
  height = 1_200;
  act(() => resized?.());
  const tall = drawn(tree);
  expect(tall).toBeGreaterThan(bound(240));
  expect(tall).toBeLessThanOrEqual(bound(1_200));
});

test('closing the folder scrolled into draws what is left, and opening it again draws its lines', async () => {
  await bigFolder(10_001);
  const { tree } = await shown();
  scrollTo(tree, 6_000);
  act(() => fileTrees.toggle(scope, 'big'));
  expect(await within(tree).findByRole('button', { name: 'big' })).toBeTruthy();
  scrollTo(tree, 0);
  act(() => fileTrees.toggle(scope, 'big'));
  expect(await within(tree).findByRole('button', { name: '00000.txt' })).toBeTruthy();
  expect(drawn(tree)).toBeLessThanOrEqual(bound(640));
});
