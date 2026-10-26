import { act, cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FilesPanel } from './FilesPanel';
import { directories, installFilesApi, root, session, workspace } from './files-fixture';
import { fileTrees, resetFileTreesForTests } from './file-tree-store';
import { resetToolTabsForTests, toolTabs, useToolTabs } from '../right-workspace/tool-tabs';

// The tree follows what the model changes: a finished tool call lists the open folders again,
// once per burst, and only while the tree is in view.

const scope = { session, workspace, root };

function Harness({ refreshKey, visible = true }: { refreshKey: string; visible?: boolean }) {
  const state = useToolTabs(session);
  const tab = state.tabs.find((item) => item.id === state.active && item.kind === 'files');
  return tab ? (
    <FilesPanel
      key={tab.id}
      refreshKey={refreshKey}
      scope={{ session, workspace, root, notify: vi.fn() }}
      tab={tab}
      visible={visible}
    />
  ) : null;
}

let api: ReturnType<typeof installFilesApi>;
const original = structuredClone(directories);
beforeEach(() => {
  resetToolTabsForTests();
  resetFileTreesForTests();
  api = installFilesApi();
  toolTabs.open(session, 'files');
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  for (const key of Object.keys(directories)) delete directories[key];
  Object.assign(directories, structuredClone(original));
});

const listed = (path: string) =>
  api.listWorkspaceFiles.mock.calls.filter(([request]) => request.target.path === path).length;

/** The tree shown, with `src` open. */
async function openTree(refreshKey = 'a') {
  const user = userEvent.setup();
  const view = render(<Harness refreshKey={refreshKey} />);
  const tree = await screen.findByRole('navigation', { name: '文件树' });
  await user.click(await within(tree).findByRole('button', { name: 'src' }));
  await within(tree).findByRole('button', { name: 'main.rs' });
  return { view, tree };
}

const fileEntry = (path: string) => ({
  path,
  name: path.split('/').at(-1)!,
  kind: 'file' as const,
  byte_length: 10,
  ignored: false,
  generated: false,
  reparse_tag: null,
});

const wait = async (ms: number) => {
  await act(async () => {
    vi.advanceTimersByTime(ms);
    await Promise.resolve();
  });
};

test('a finished tool call lists the root and the open folders again after 250 ms', async () => {
  const { view, tree } = await openTree();
  directories['.']!.push(fileEntry('NEW.md'));
  directories.src!.push(fileEntry('src/lib.rs'));
  const before = { root: listed('.'), src: listed('src') };
  vi.useFakeTimers();
  view.rerender(<Harness refreshKey="b" />);
  await wait(249);
  expect(listed('.')).toBe(before.root);
  await wait(1);
  expect(listed('.')).toBe(before.root + 1);
  expect(listed('src')).toBe(before.src + 1);
  vi.useRealTimers();
  expect(await within(tree).findByRole('button', { name: 'NEW.md' })).toBeTruthy();
  expect(await within(tree).findByRole('button', { name: 'lib.rs' })).toBeTruthy();
});

test('a burst of tool calls lists each folder once', async () => {
  const { view } = await openTree();
  const before = { root: listed('.'), src: listed('src') };
  vi.useFakeTimers();
  for (const key of ['b', 'c', 'd']) {
    view.rerender(<Harness refreshKey={key} />);
    await wait(100);
  }
  await wait(250);
  expect(listed('.')).toBe(before.root + 1);
  expect(listed('src')).toBe(before.src + 1);
});

test('a hidden page reads nothing until it shows again, then reads once', async () => {
  const { view } = await openTree();
  const before = listed('.');
  vi.useFakeTimers();
  view.rerender(<Harness refreshKey="b" visible={false} />);
  await wait(1000);
  expect(listed('.')).toBe(before);
  view.rerender(<Harness refreshKey="b" visible />);
  await wait(250);
  expect(listed('.')).toBe(before + 1);
});

test('an open folder the model deleted leaves the tree without an error row', async () => {
  const { view, tree } = await openTree();
  directories['.'] = directories['.']!.filter((item) => item.path !== 'src');
  delete directories.src;
  vi.useFakeTimers();
  view.rerender(<Harness refreshKey="b" />);
  await wait(250);
  vi.useRealTimers();
  await act(async () => {
    await Promise.resolve();
  });
  expect(within(tree).queryByRole('button', { name: 'src' })).toBeNull();
  expect(within(tree).queryByRole('button', { name: '重试' })).toBeNull();
  expect(fileTrees.get(scope).expanded.has('src')).toBe(false);
});
