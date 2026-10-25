import type { AgentSessionProjection } from '@aura/contracts/generated';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from './Conversation';

// 新建分支 under a finished
// answer asks first, says what is carried, makes the branch, keeps the model and opens it; a turn a
// branch carried is history.

const selectors: Record<
  string,
  { selected_model_profile_id: string | null; binding_revision: string }
> = {
  session: { selected_model_profile_id: 'mpf_source', binding_revision: '2' },
  branch: { selected_model_profile_id: null, binding_revision: '0' },
};
vi.mock('../model-projection-store', () => ({
  readSessionModelSelector: vi.fn(async (id: string) => ({ payload: selectors[id] })),
}));

const made = {
  session_id: 'branch',
  workspace_id: 'workspace',
  title: '分支：雷霆战机',
  origin: {
    source_session_id: 'session',
    source_title: '雷霆战机',
    source_turn_task_id: 'turn',
    turn_number: 1,
    created_at: '2026-10-20T08:30:00Z',
  },
};
const branchSession = vi.fn();
const setSessionModelBinding = vi.fn();
beforeEach(() => {
  branchSession.mockReset().mockResolvedValue({ ok: true, value: made });
  setSessionModelBinding.mockReset().mockResolvedValue({ ok: true, value: { changed: true } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        branchSession,
        setSessionModelBinding,
        readAgentMessageText: vi.fn(async () => ({ ok: false, error: { message: 'unused' } })),
        readAgentToolActivity: vi.fn(async () => ({
          ok: true,
          value: { session_id: 'session', next_before_call_id: null, calls: [] },
        })),
      },
    },
  });
});
afterEach(cleanup);

const answer = '打开方式：双击 thunder-fighter.html。';
const bytes = (text: string) => new TextEncoder().encode(text).length;
const turn = (id: string, extra: Record<string, unknown> = {}) => ({
  turn: {
    mode: 'normal',
    state: { state: 'completed', final_report: answer },
    session_id: 'session',
    turn_task_id: id,
    revision: '3',
    root_user_event_id: `${id}-root`,
    created_at: '2026-10-20T00:00:00Z',
    updated_at: '2026-10-20T00:00:09Z',
  },
  final_text_source: { kind: 'assistant_step', model_step_id: `${id}-answer` },
  user_messages: [
    {
      event_id: `${id}-root`,
      text: '写一个雷霆战机小游戏',
      total_bytes: bytes('写一个雷霆战机小游戏'),
      origin: { origin: 'submit' },
    },
  ],
  assistant_messages: [
    {
      model_step_id: `${id}-answer`,
      model_profile_id: null,
      context_snapshot_id: 'mex_1',
      text: answer,
      reasoning: '',
      text_total_bytes: bytes(answer),
      reasoning_total_bytes: 0,
      partial: false,
      stream_sequence: null,
      tool_call_ids: [],
      at: '2026-10-20T00:00:00Z',
    },
  ],
  goal: null,
  ...extra,
});
const page = (turns: unknown[], extra: Record<string, unknown> = {}) =>
  ({
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: null,
    turns,
    ...extra,
  }) as unknown as AgentSessionProjection;

function show(
  projection: AgentSessionProjection,
  onBranched = vi.fn(),
  onOpenSession = vi.fn(() => true),
) {
  render(
    <Conversation
      projection={projection}
      viewportRef={{ current: null }}
      onChanged={vi.fn()}
      loading={false}
      error={undefined}
      loadOlder={vi.fn()}
      loadTurnOlder={vi.fn()}
      title="雷霆战机"
      onBranched={onBranched}
      onOpenSession={onOpenSession}
    />,
  );
  return { onBranched, onOpenSession };
}

test('新建分支 asks first, says what is carried, then makes the branch, keeps the model and opens it', async () => {
  const user = userEvent.setup();
  const { onBranched } = show(page([turn('turn')]));
  const footer = document.querySelector('.agent-turn-footer') as HTMLElement;
  await user.click(within(footer).getByRole('button', { name: '新建分支' }));
  const dialog = screen.getByRole('dialog', { name: '从这一轮新建分支？' });
  expect(dialog.textContent).toContain('会新建会话“分支：雷霆战机”');
  expect(dialog.textContent).toContain('文件不会分支');
  expect(dialog.textContent).toContain('仍会先问你');
  expect(dialog.textContent).toContain('分支从普通模式开始');
  expect(branchSession).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole('button', { name: '新建分支' }));
  await waitFor(() => expect(onBranched).toHaveBeenCalledWith(made));
  expect(branchSession).toHaveBeenCalledExactlyOnceWith({
    session_id: 'session',
    turn_task_id: 'turn',
  });
  // The branch answers with the model the conversation used, bound as a new conversation is.
  expect(setSessionModelBinding).toHaveBeenCalledExactlyOnceWith({
    session_id: 'branch',
    expected_revision: '0',
    model_profile_id: 'mpf_source',
  });
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('a branch that was not made is said in the dialog and can be tried again', async () => {
  const user = userEvent.setup();
  branchSession.mockResolvedValueOnce({
    ok: false,
    error: {
      code: 'ipc.failed',
      message: 'agent.branch_turn_unknown: 这一轮不在这个会话里，请刷新后再试。',
    },
  });
  const { onBranched } = show(page([turn('turn')]));
  await user.click(screen.getByRole('button', { name: '新建分支' }));
  const dialog = screen.getByRole('dialog');
  await user.click(within(dialog).getByRole('button', { name: '新建分支' }));
  expect(await within(dialog).findByRole('alert')).toHaveProperty(
    'textContent',
    '这一轮不在这个会话里，请刷新后再试。',
  );
  expect(onBranched).not.toHaveBeenCalled();
  expect(setSessionModelBinding).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole('button', { name: '重试' }));
  await waitFor(() => expect(onBranched).toHaveBeenCalledWith(made));
});

test('while the conversation is copied the dialog says so and what comes next', async () => {
  const user = userEvent.setup();
  let answer: (value: unknown) => void = () => {};
  branchSession.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        answer = resolve;
      }),
  );
  const { onBranched } = show(page([turn('turn')]));
  await user.click(screen.getByRole('button', { name: '新建分支' }));
  const dialog = screen.getByRole('dialog');
  expect(within(dialog).queryByRole('status')).toBeNull();
  await user.click(within(dialog).getByRole('button', { name: '新建分支' }));
  // The branch opens with its newest turns; the older ones follow inside it.
  expect(within(dialog).getByRole('status').textContent).toBe(
    '正在打开分支：先带上最近的几轮，较早的消息会在分支里继续复制。',
  );
  expect(within(dialog).getByRole('button', { name: '正在新建分支…' })).toHaveProperty(
    'disabled',
    true,
  );
  answer({ ok: true, value: made });
  await waitFor(() => expect(onBranched).toHaveBeenCalledWith(made));
});

test('a carried turn is history: its plan is not implemented here, and a line names the source', async () => {
  const user = userEvent.setup();
  const plan = {
    revision: '1',
    original_user_goal: '做一个小游戏',
    plan: { requirements: [], steps: [], current_project_state: '' },
  };
  const carried = turn('carried', {
    imported: true,
    plan,
    turn: { ...turn('carried').turn, mode: 'plan' },
  });
  const own = turn('own');
  const { onOpenSession } = show(
    page([carried, own], { branch_origin: made.origin }),
    vi.fn(),
    vi.fn(() => false),
  );
  expect(screen.queryByRole('button', { name: /实施计划/ })).toBeNull();
  expect(screen.getByText('这是分支前的历史，不能在这里实施。')).toBeTruthy();
  const line = screen.getByRole('note', { name: '分支起点' });
  expect(line.textContent).toContain('从「雷霆战机」第 1 轮分支');
  // The line sits between the carried turn and the branch's own.
  const turns = [...document.querySelectorAll('.agent-turn')];
  expect(turns[0]!.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(line.compareDocumentPosition(turns[1]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  await user.click(within(line).getByRole('button', { name: '打开原会话' }));
  expect(onOpenSession).toHaveBeenCalledWith('session');
  expect(line.textContent).toContain('原会话已不存在');
});
