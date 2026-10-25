import { beforeEach, expect, test, vi } from 'vitest';
import { resetToolTabsForTests, tabLabels, toolTabs } from './tool-tabs';

// Files open in 文件 tabs. "打开的文件在文件页内的文件树
// 右侧半栏展示，控制关闭文件查看标签页，而不是每开一个文件就多一个侧边栏标签页" — a file opens in the
// 文件 page in use, beside its tree, as one of that page's own small tabs; the sidebar's tab stays
// 文件 however many files are open.

const A = 'ses_a';
beforeEach(() => resetToolTabsForTests());

const active = () => {
  const state = toolTabs.get(A);
  return state.tabs.find((tab) => tab.id === state.active);
};
const labels = () => tabLabels(toolTabs.get(A)).map((item) => item.label);

test('files open in one 文件 page, which keeps its name however many are open', () => {
  const page = toolTabs.openFile(A, 'src/main.rs');
  toolTabs.open(A, 'review');
  expect(toolTabs.openFile(A, 'README.md')).toBe(page);
  expect(toolTabs.openFile(A, 'src/main.rs', 42)).toBe(page);
  expect(active()).toMatchObject({
    id: page,
    kind: 'files',
    files: ['src/main.rs', 'README.md'],
    file: 'src/main.rs',
    line: 42,
  });
  expect(labels()).toEqual(['文件', '审查']);
});

test('asking for the same line again is a new request to show it', () => {
  toolTabs.openFile(A, 'README.md', 3);
  const first = active()?.shown;
  toolTabs.openFile(A, 'README.md', 3);
  expect(active()?.shown).toBeGreaterThan(first ?? 0);
});

test('a file opens in the 文件 page on screen, else in the last one opened', () => {
  const first = toolTabs.open(A, 'files');
  const second = toolTabs.open(A, 'files');
  toolTabs.activate(A, first);
  expect(toolTabs.openFile(A, 'a.txt')).toBe(first);
  toolTabs.open(A, 'review');
  expect(toolTabs.openFile(A, 'b.txt')).toBe(second);
  expect(labels()).toEqual(['文件 1', '文件 2', '审查']);
});

test('a file closes on its own: its neighbour is shown, and the last leaves the tree alone', () => {
  const page = toolTabs.openFile(A, 'a.txt');
  toolTabs.openFile(A, 'b.txt');
  toolTabs.openFile(A, 'c.txt');
  toolTabs.showFile(A, page, 'b.txt');
  expect(active()?.file).toBe('b.txt');
  toolTabs.closeFile(A, page, 'b.txt');
  expect(active()).toMatchObject({ files: ['a.txt', 'c.txt'], file: 'c.txt' });
  toolTabs.closeFile(A, page, 'c.txt');
  expect(active()?.file).toBe('a.txt');
  toolTabs.closeFile(A, page, 'a.txt');
  expect(active()?.files).toEqual([]);
  expect(active()?.file).toBeUndefined();
  expect(labels()).toEqual(['文件']);
});

test('a link elsewhere shows the file and asks the window for the sidebar', () => {
  const revealed = vi.fn();
  const stop = toolTabs.onReveal(revealed);
  toolTabs.revealFile(A, 'src/lib.rs', 7);
  expect(active()).toMatchObject({ kind: 'files', file: 'src/lib.rs', line: 7 });
  expect(revealed).toHaveBeenCalledExactlyOnceWith(A);
  stop();
});
