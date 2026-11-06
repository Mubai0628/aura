import type { AgentSessionProjection } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from '../agent-session/Conversation';
import { resetSendingForTests } from '../agent-session/sending';
import { forgetToolCallDirectories } from '../agent-session/useToolCallDirectory';

/**
 * A turn the user took the desktop over in: nothing under it offers 继续, whether the turn ended
 * taken over or went on after the desktop was given back; 近期活动 gives the desktop back. A turn
 * a restart cut off keeps its own 继续.
 */
const activity = vi.fn();
const submit = vi.fn();
const continueAgent = vi.fn();
const CALL = 'tcl_01HOSTTOOLCALL0000000001';

const entry = (offer?: string) => ({
  model_step_id: 'step-1',
  display: {
    title: '用鼠标键盘操作程序',
    target: '记事本',
    summary: '未执行：你已接管电脑',
    details: [],
    truncated: false,
    internal: false,
    ...(offer ? { offer } : {}),
  },
  summary: {
    tool_call_id: CALL,
    tool_batch_id: 'tbt_1',
    ordinal: 1,
    tool_id: 'tol_01',
    tool_version: { definition_revision: '1', semantic_version: null },
    display_name: 'computer.input',
    status: 'finalized',
    outcome: { type: 'operation_rejected' },
    side_effect_state: null,
    started_at: null,
    completed_at: null,
    can_cancel: false,
  },
  shell_query_available: false,
  permission_mode: 'workspace_read_write',
  permission_revision: 1,
});

const answer = (offer?: string) =>
  activity.mockResolvedValue({
    ok: true,
    value: { session_id: 'session', next_before_call_id: null, calls: [entry(offer)] },
  });

beforeEach(() => {
  localStorage.clear();
  forgetToolCallDirectories();
  resetSendingForTests();
  answer();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  continueAgent.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentToolActivity: activity,
        submitAgentMessage: submit,
        continueAgent,
        readAgentResult: vi
          .fn()
          .mockResolvedValue({ ok: true, value: { text: '', next_offset: null } }),
      },
    },
  });
});
afterEach(cleanup);

const completed = { state: 'completed', final_report: '小猫画好了，就在画图里开着。' };

/** The message 交还 in 近期活动 adds to a running turn. */
const givenBack = {
  event_id: 'evt_given_back',
  at: '2026-11-04T00:00:05Z',
  origin: { origin: 'steer' },
  text: '我已交还电脑的操作权，继续。',
  total_bytes: 42,
};

/** A turn with a call the user took the desktop over from. */
const turn = (
  id: string,
  state: Record<string, unknown> = completed,
  extra: Record<string, unknown> = {},
) => ({
  turn: {
    mode: 'normal',
    state,
    session_id: 'session',
    turn_task_id: id,
    revision: '3',
    root_user_event_id: 'root',
    created_at: '2026-11-04T00:00:00Z',
    updated_at: '2026-11-04T00:00:10Z',
  },
  final_text_source: null,
  user_messages: [],
  assistant_messages: [
    {
      model_step_id: `${id}-step`,
      model_profile_id: null,
      context_snapshot_id: `mex_${id}`,
      text: '',
      reasoning: '',
      text_total_bytes: 0,
      reasoning_total_bytes: 0,
      partial: false,
      stream_sequence: null,
      tool_call_ids: [CALL],
      at: '2026-11-04T00:00:01Z',
    },
  ],
  plan: null,
  goal: null,
  ...extra,
});

function show(turns: unknown[]) {
  const projection = {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: null,
    turns,
  } as unknown as AgentSessionProjection;
  return render(
    <Conversation
      sessionId="session"
      projection={projection}
      viewportRef={{ current: null }}
      onChanged={vi.fn()}
      loading={false}
      error={undefined}
      loadOlder={vi.fn()}
      loadTurnOlder={vi.fn()}
    />,
  );
}

/** The calls are read: the section of the opened process names the refused one. */
async function callsRead() {
  for (const status of screen.queryAllByRole('button', { name: /^已完成/u }))
    if (status.getAttribute('aria-expanded') === 'false') fireEvent.click(status);
  await screen.findByRole('button', { name: /用鼠标键盘操作程序/u });
}

test('a turn the user took the desktop over in offers no 继续, whatever its row carries', async () => {
  for (const [offer, extra] of [
    ['continue_control', {}],
    [undefined, {}],
    ['continue_control', { computer_mode: 'child_session' }],
    ['continue_control', { user_messages: [givenBack] }],
  ] as const) {
    answer(offer);
    show([turn('taken', completed, extra)]);
    await callsRead();
    expect(screen.queryByRole('button', { name: '继续' })).toBeNull();
    cleanup();
    forgetToolCallDirectories();
  }
  expect(submit).not.toHaveBeenCalled();
});

test('a turn a restart cut off keeps its own 继续, which continues that turn', async () => {
  answer('continue_control');
  show([turn('cut', { state: 'recovery_required', reason: 'agent.core_restarted' })]);
  await screen.findByRole('button', { name: /用鼠标键盘操作程序/u });
  expect(screen.getAllByRole('button', { name: '继续' })).toHaveLength(1);
  fireEvent.click(screen.getByRole('button', { name: '继续' }));
  await waitFor(() => expect(continueAgent).toHaveBeenCalledTimes(1));
  expect(submit).not.toHaveBeenCalled();
});
