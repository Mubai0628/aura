import type { AgentToolActivityEntry, TerminalViewV1 } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ToolActivity } from './ToolActivity';
import { addTerminal, resetTerminalStoreForTests } from '../terminal/store';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import type { TerminalApiV1 } from '../../../shared/terminal-api';

// The model's terminal becomes a tab of the right sidebar only when the user
// asks to see it — from 进程, or from the call in the conversation that opened it.

const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const call = 'tcl_01HOSTTOOLCALL0000000001';

const view = (owner: 'agent' | 'user', toolCallId: string | null, terminalId: string) =>
  ({
    session_id: session,
    terminal_id: terminalId,
    shell: 'power_shell',
    process_id: 1,
    started_at: '2026-10-19T00:00:00Z',
    end: null,
    owner,
    controller: owner,
    title: '构建',
    tool_call_id: toolCallId,
    revision: '1',
  }) as unknown as TerminalViewV1;

const api = {
  subscribe: vi.fn(async (request: { terminal_id: string }) => ({
    ok: true,
    value: {
      runtime_instance_id: 'run_fixture',
      session_id: session,
      terminal_id: request.terminal_id,
      start_offset: 0,
      next_offset: 0,
      data_base64: '',
      truncated: false,
      end: null,
    },
  })),
} as unknown as TerminalApiV1;

const entry = (name: string, callId: string, details: string[] = []) =>
  ({
    display: {
      details,
      internal: false,
      summary: '',
      target: '',
      title: '打开终端',
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
      tool_call_id: callId,
      tool_id: 'tol_01',
      tool_version: { definition_revision: '1', semantic_version: null },
    },
  }) as unknown as AgentToolActivityEntry;

beforeEach(() => {
  resetTerminalStoreForTests();
  resetToolTabsForTests();
});
afterEach(cleanup);

test("the call that opened the model's terminal shows it as a tab and asks for the sidebar", async () => {
  await act(async () => {
    await addTerminal(view('agent', call, 'trm_model'), api);
  });
  const revealed = vi.fn();
  const stop = toolTabs.onReveal(revealed);
  for (const details of [[], ['PowerShell 7']]) {
    const shown = render(<ToolActivity entry={entry('terminal.open', call, details)} />);
    fireEvent.click(screen.getByRole('button', { name: '查看终端' }));
    shown.unmount();
  }
  const state = toolTabs.get(session);
  // Asking twice shows the one tab again.
  expect(state.tabs).toEqual([{ id: state.active, kind: 'terminal', terminalId: 'trm_model' }]);
  expect(revealed).toHaveBeenCalledTimes(2);
  expect(revealed).toHaveBeenLastCalledWith(session);
  stop();
});

test('the link appears once the terminal is known, and only for the model terminal of that call', async () => {
  render(<ToolActivity entry={entry('terminal.open', call)} />);
  expect(screen.queryByRole('button', { name: '查看终端' })).toBeNull();
  await act(async () => {
    await addTerminal(view('agent', 'tcl_01HOSTTOOLCALL0000000002', 'trm_other_call'), api);
    await addTerminal(view('user', call, 'trm_user'), api);
  });
  expect(screen.queryByRole('button', { name: '查看终端' })).toBeNull();
  await act(async () => {
    await addTerminal(view('agent', call, 'trm_model'), api);
  });
  expect(screen.getByRole('button', { name: '查看终端' })).toBeTruthy();
});
