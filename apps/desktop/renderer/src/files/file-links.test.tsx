import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileLinkContext } from './file-links';
import { root, session, workspace } from './files-fixture';
import { MarkdownBody } from '../markdown-body';
import { ToolActivity } from '../agent-session/ToolActivity';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';

// "Click a file path in the conversation to open it" — a path in a message's
// inline code, or the file a file tool names, opens in a 文件 tab of the right sidebar.

const scope = { session, workspace, root, notify: vi.fn() };
beforeEach(() => resetToolTabsForTests());
afterEach(cleanup);

const active = () => {
  const state = toolTabs.get(session);
  return state.tabs.find((tab) => tab.id === state.active);
};

test('inline code naming a workspace file opens it at its line; other code stays code', async () => {
  const user = userEvent.setup();
  const revealed = vi.fn();
  const stop = toolTabs.onReveal(revealed);
  render(
    <FileLinkContext.Provider value={scope}>
      <MarkdownBody text="先看 `src/main.rs:3`，再看 `foo.bar()`。" />
    </FileLinkContext.Provider>,
  );
  await user.click(screen.getByRole('button', { name: '打开 src/main.rs:3' }));
  expect(active()).toMatchObject({ kind: 'files', file: 'src/main.rs', line: 3 });
  expect(revealed).toHaveBeenCalledWith(session);
  expect(screen.queryByRole('button', { name: /foo\.bar/u })).toBeNull();
  expect(screen.getByText('foo.bar()').tagName).toBe('CODE');
  stop();
});

test('a path in a table cell is a link too', async () => {
  const user = userEvent.setup();
  render(
    <FileLinkContext.Provider value={scope}>
      <MarkdownBody text={'| 模块 | 职责 |\n| --- | --- |\n| `src/main.rs` | 入口 |\n'} />
    </FileLinkContext.Provider>,
  );
  await user.click(screen.getByRole('button', { name: '打开 src/main.rs' }));
  expect(active()).toMatchObject({ kind: 'files', file: 'src/main.rs' });
});

test('outside a conversation, inline code is never a link', () => {
  render(<MarkdownBody text="见 `src/main.rs`" />);
  expect(screen.queryByRole('button')).toBeNull();
  expect(screen.getByText('src/main.rs').tagName).toBe('CODE');
});

const call = (name: string, target: string) =>
  ({
    display: {
      details: [],
      internal: false,
      summary: '',
      target,
      title: '读取文件',
      truncated: false,
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

test('the file a file tool names opens from its row; a Shell command does not', async () => {
  const user = userEvent.setup();
  const shown = render(
    <FileLinkContext.Provider value={scope}>
      <ToolActivity entry={call('fs.read', 'C:\\Dev\\Demo\\scripts\\build')} />
    </FileLinkContext.Provider>,
  );
  await user.click(screen.getByRole('button', { name: '打开 scripts/build' }));
  expect(active()).toMatchObject({ kind: 'files', file: 'scripts/build' });
  shown.unmount();
  render(
    <FileLinkContext.Provider value={scope}>
      <ToolActivity entry={call('shell.powershell', 'README.md')} />
    </FileLinkContext.Provider>,
  );
  expect(screen.queryByRole('button')).toBeNull();
});
