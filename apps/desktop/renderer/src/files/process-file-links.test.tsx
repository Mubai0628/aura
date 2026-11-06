import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ToolActivity } from '../agent-session/ToolActivity';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { FileLinkContext } from './file-links';
import { root, session, workspace } from './files-fixture';

// A file the model's work names — a download, a video it read, a page it opened, what a lookup
// found or a search matched — opens from the process like the file a file tool names, and has the
// same right-click menu.

const scope = { session, workspace, root, notify: vi.fn() };
beforeEach(() => resetToolTabsForTests());
afterEach(cleanup);

const active = () => {
  const state = toolTabs.get(session);
  return state.tabs.find((tab) => tab.id === state.active);
};

const call = (name: string, display: Partial<AgentToolActivityEntry['display']>) =>
  ({
    display: {
      details: [],
      internal: false,
      summary: '',
      title: '工具',
      truncated: false,
      ...display,
    },
    summary: {
      can_cancel: false,
      completed_at: null,
      display_name: name,
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

const row = (entry: AgentToolActivityEntry) =>
  render(
    <FileLinkContext.Provider value={scope}>
      <ToolActivity entry={entry} />
    </FileLinkContext.Provider>,
  );

const expanded = (entry: AgentToolActivityEntry) => {
  const view = row(entry);
  const details = view.container.querySelector('details') as HTMLDetailsElement;
  details.open = true;
  fireEvent(details, new Event('toggle'));
  return view;
};

const menuItems = () =>
  screen.getAllByRole('menuitem').map((item) => item.textContent?.replace(/\s+/gu, ''));

test('the file a download, a video read or a page opening names opens from its row, with its menu', async () => {
  const user = userEvent.setup();
  for (const [tool, target, path] of [
    ['http.download', 'downloads/clip.mp4', 'downloads/clip.mp4'],
    ['media.frames', 'C:\\Dev\\Demo\\video\\demo.webm', 'video/demo.webm'],
    ['media.probe', 'video/demo.webm', 'video/demo.webm'],
    ['browser.open', 'dist/game.html', 'dist/game.html'],
  ] as const) {
    const shown = row(call(tool, { target }));
    const link = screen.getByRole('button', { name: `打开 ${path}` });
    await user.click(link);
    expect(active(), tool).toMatchObject({ kind: 'files', file: path });
    fireEvent.contextMenu(link);
    expect(menuItems(), tool).toContain('在文件资源管理器中显示');
    shown.unmount();
    resetToolTabsForTests();
  }
});

test('the workspace page a browser call shows, named by its file address, opens from its row', async () => {
  const user = userEvent.setup();
  for (const [tool, target] of [
    ['browser.open', 'file:///C:/Dev/Demo/dist/game.html'],
    ['browser.screenshot', 'file:///C:/Dev/Demo/dist/game.html#level-2'],
    ['browser.click', 'file:///c:/Dev/Demo/dist/game.html?seed=7'],
  ] as const) {
    const shown = row(call(tool, { target }));
    const link = screen.getByRole('button', { name: '打开 dist/game.html' });
    expect(link.textContent, tool).toBe(target);
    await user.click(link);
    expect(active(), tool).toMatchObject({ kind: 'files', file: 'dist/game.html' });
    fireEvent.contextMenu(link);
    expect(menuItems(), tool).toContain('在文件资源管理器中显示');
    shown.unmount();
    resetToolTabsForTests();
  }
});

test('an address, a deleted file, a path above the workspace and a name cut short stay text', () => {
  for (const [tool, target] of [
    ['browser.open', 'http://127.0.0.1:9/game'],
    ['browser.open', 'file:///C:/Dev/Demo/dist/a-very-long-name…'],
    ['browser.open', 'file:///D:/Games/a-very-long-name…'],
    ['media.frames', 'D:\\Videos\\a-very-long-name…'],
    ['fs.read', 'docs/a-very-long-name…'],
    ['fs.read', '../outside/notes.txt'],
    ['fs.delete', 'old/notes.txt'],
    ['fs.delete', 'D:\\old\\notes.txt'],
  ] as const) {
    const shown = row(call(tool, { target }));
    expect(screen.queryByRole('button'), tool).toBeNull();
    expect(screen.getByText(target), tool).toBeTruthy();
    shown.unmount();
  }
});

test('what a lookup found opens from the expanded row; a path cut short stays text', async () => {
  const user = userEvent.setup();
  expanded(
    call('fs.find', {
      target: '.',
      summary: '找到 2 个文件',
      details: ['src/main.rs', 'docs/a-very-long-name…'],
    }),
  );
  const found = screen.getByRole('button', { name: '打开 src/main.rs' });
  await user.click(found);
  expect(active()).toMatchObject({ kind: 'files', file: 'src/main.rs' });
  fireEvent.contextMenu(found);
  expect(menuItems()).toContain('复制相对路径');
  expect(screen.queryByRole('button', { name: /a-very-long-name/u })).toBeNull();
  expect(screen.getByText('docs/a-very-long-name…')).toBeTruthy();
});

test('where a search matched opens at its line from the expanded row', async () => {
  const user = userEvent.setup();
  expanded(
    call('fs.search', {
      target: '.',
      summary: '找到 1 处匹配',
      details: ['src/main.rs:2 · println!("hello"); · done'],
    }),
  );
  await user.click(screen.getByRole('button', { name: '打开 src/main.rs:2' }));
  expect(active()).toMatchObject({ kind: 'files', file: 'src/main.rs', line: 2 });
  expect(screen.getByText(/println!\("hello"\); · done/u)).toBeTruthy();
});
