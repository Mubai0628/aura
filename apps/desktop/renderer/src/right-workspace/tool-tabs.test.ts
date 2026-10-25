import { beforeEach, expect, test, vi } from 'vitest';
import { availableTools, resetToolTabsForTests, tabLabels, toolTabs } from './tool-tabs';

// The right sidebar holds tabs. 终端、文件 open as many times as asked;
// 审查、进程、浏览器（its pages are its own tabs）、侧边对话、提示词监测 open once, and once
// open they leave the "+" menu. A terminal the
// model opened becomes a tab when the user asks to see it. Each conversation keeps its own tabs.

const A = 'ses_a';
const B = 'ses_b';
beforeEach(() => resetToolTabsForTests());

const kinds = (session: string) => toolTabs.get(session).tabs.map((tab) => tab.kind);
const active = (session: string) => {
  const state = toolTabs.get(session);
  return state.tabs.find((tab) => tab.id === state.active);
};

test('a single tool opens once; asking again shows the tab that is open', () => {
  const first = toolTabs.open(A, 'review');
  toolTabs.open(A, 'terminal');
  expect(toolTabs.open(A, 'review')).toBe(first);
  expect(kinds(A)).toEqual(['review', 'terminal']);
  expect(active(A)?.id).toBe(first);
  expect(availableTools(toolTabs.get(A))).not.toContain('review');
  expect(availableTools(toolTabs.get(A))).toContain('terminal');
});

test('terminals and files open as new tabs, each with its own label; the browser opens once', () => {
  toolTabs.open(A, 'terminal');
  toolTabs.open(A, 'terminal');
  toolTabs.open(A, 'files');
  toolTabs.open(A, 'files');
  const browser = toolTabs.open(A, 'browser');
  expect(toolTabs.open(A, 'browser')).toBe(browser);
  expect(kinds(A)).toEqual(['terminal', 'terminal', 'files', 'files', 'browser']);
  expect(tabLabels(toolTabs.get(A)).map((tab) => tab.label)).toEqual([
    '终端 1',
    '终端 2',
    '文件 1',
    '文件 2',
    '浏览器',
  ]);
  for (const kind of ['terminal', 'files'] as const)
    expect(availableTools(toolTabs.get(A))).toContain(kind);
  expect(availableTools(toolTabs.get(A))).not.toContain('browser');
});

test('a terminal keeps its number when an earlier one closes', () => {
  const one = toolTabs.open(A, 'terminal');
  toolTabs.open(A, 'terminal');
  toolTabs.close(A, one);
  toolTabs.open(A, 'terminal');
  expect(tabLabels(toolTabs.get(A)).map((tab) => tab.label)).toEqual(['终端 2', '终端 3']);
});

test('closing the open tab shows its neighbour; closing the last leaves the sidebar empty', () => {
  const review = toolTabs.open(A, 'review');
  const terminal = toolTabs.open(A, 'terminal');
  const files = toolTabs.open(A, 'files');
  toolTabs.activate(A, terminal);
  toolTabs.close(A, terminal);
  expect(active(A)?.id).toBe(files);
  toolTabs.close(A, files);
  expect(active(A)?.id).toBe(review);
  toolTabs.close(A, review);
  expect(toolTabs.get(A)).toEqual({ tabs: [], active: undefined });
});

test('show brings back the latest tab of a kind, or opens one', () => {
  const first = toolTabs.open(A, 'terminal');
  const second = toolTabs.open(A, 'terminal');
  toolTabs.open(A, 'review');
  expect(toolTabs.show(A, 'terminal')).toBe(second);
  toolTabs.close(A, second);
  expect(toolTabs.show(A, 'terminal')).toBe(first);
  const files = toolTabs.show(A, 'files');
  expect(active(A)?.kind).toBe('files');
  expect(active(A)?.id).toBe(files);
});

test('a terminal opened elsewhere becomes a tab once, bound to that terminal', () => {
  const tab = toolTabs.showTerminal(A, 'trm_model');
  expect(toolTabs.showTerminal(A, 'trm_model')).toBe(tab);
  expect(toolTabs.get(A).tabs).toEqual([{ id: tab, kind: 'terminal', terminalId: 'trm_model' }]);
  const own = toolTabs.open(A, 'terminal');
  toolTabs.bindTerminal(A, own, 'trm_own');
  expect(toolTabs.showTerminal(A, 'trm_own')).toBe(own);
});

test('each conversation keeps its own tabs', () => {
  toolTabs.open(A, 'review');
  toolTabs.open(B, 'terminal');
  expect(kinds(A)).toEqual(['review']);
  expect(kinds(B)).toEqual(['terminal']);
  expect(availableTools(toolTabs.get(B))).toContain('review');
});

test('a running terminal of the user’s own that has no tab gets one back, without being shown', () => {
  const review = toolTabs.open(A, 'review');
  toolTabs.adoptTerminal(A, 'trm_left');
  toolTabs.adoptTerminal(A, 'trm_left');
  expect(toolTabs.get(A).tabs.filter((tab) => tab.terminalId === 'trm_left')).toHaveLength(1);
  expect(toolTabs.get(A).active).toBe(review);
  expect(tabLabels(toolTabs.get(A)).map((tab) => tab.label)).toEqual(['审查', '终端 1']);
});

test('a link elsewhere shows the terminal and asks the window for the sidebar, until it stops listening', () => {
  const revealed = vi.fn();
  const stop = toolTabs.onReveal(revealed);
  toolTabs.open(A, 'review');
  toolTabs.revealTerminal(A, 'trm_model');
  expect(active(A)).toMatchObject({ kind: 'terminal', terminalId: 'trm_model' });
  expect(revealed).toHaveBeenCalledExactlyOnceWith(A);
  stop();
  toolTabs.revealTerminal(A, 'trm_model');
  expect(revealed).toHaveBeenCalledTimes(1);
  expect(kinds(A)).toEqual(['review', 'terminal']);
});
