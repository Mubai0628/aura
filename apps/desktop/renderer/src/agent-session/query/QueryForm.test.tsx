import type { UserQuery } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { QueryForm } from './QueryForm';

// Every question takes the user's own words, the user may decline, and a
// question left alone for 30 s shows a 30 s countdown that any touch starts again.

const query: UserQuery = {
  query_id: 'qry_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  turn_task_id: 'trn_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  agent_run_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  tool_call_id: 'tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  revision: '1',
  created_at: '2026-10-20T10:00:00Z',
  state: { state: 'pending' },
  input: {
    questions: [
      {
        id: 'empty_workspace',
        text: '工作区是空的，你希望我怎么处理？',
        answer_type: {
          type: 'single_choice',
          options: [
            { id: 'minimal', label: '创建最小 AGENTS.md' },
            { id: 'skip', label: '跳过' },
          ],
        },
      },
      { id: 'proceed', text: '现在继续吗？', answer_type: { type: 'confirm' } },
    ],
  },
} as unknown as UserQuery;

const answer = vi.fn();
beforeEach(() => {
  answer.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { answerAgentQuery: answer } },
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const sent = () => answer.mock.calls[0]![0].answers;

test('every question offers a custom reply, and the user’s words are what is sent', async () => {
  const user = userEvent.setup();
  render(<QueryForm query={query} sessionId="ses_1" onChanged={vi.fn()} />);
  await user.click(screen.getByRole('radio', { name: '自定义回复' }));
  await user.type(
    screen.getByRole('textbox', { name: '工作区是空的，你希望我怎么处理？（自定义回复）' }),
    '先列出目录再决定',
  );
  await user.click(screen.getByRole('button', { name: '下一题' }));
  expect(screen.getByRole('radio', { name: '自定义回复' })).toBeTruthy();
  await user.click(screen.getByRole('radio', { name: '确认' }));
  await user.click(screen.getByRole('button', { name: '提交回答' }));
  expect(sent()).toEqual([
    { question_id: 'empty_workspace', value: { answer: 'custom', text: '先列出目录再决定' } },
    { question_id: 'proceed', value: { answer: 'confirm', accepted: true } },
  ]);
});

test('one question shows at a time, and moving between them keeps each answer', async () => {
  const user = userEvent.setup();
  render(<QueryForm query={query} sessionId="ses_1" onChanged={vi.fn()} />);
  const form = screen.getByRole('form', { name: '回答当前问题' });
  expect(form.textContent).toContain('第 1 / 2 题');
  expect(form.textContent).toContain('工作区是空的');
  expect(form.textContent).not.toContain('现在继续吗？');
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '上一题' }).disabled).toBe(true);
  await user.click(screen.getByRole('radio', { name: '跳过' }));
  await user.click(screen.getByRole('button', { name: '下一题' }));
  expect(form.textContent).toContain('第 2 / 2 题');
  expect(form.textContent).toContain('现在继续吗？');
  expect(form.textContent).not.toContain('工作区是空的');
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '下一题' }).disabled).toBe(true);
  await user.click(screen.getByRole('button', { name: '上一题' }));
  expect(screen.getByRole<HTMLInputElement>('radio', { name: '跳过' }).checked).toBe(true);
  // A dot goes straight to its question.
  await user.click(screen.getByRole('button', { name: '第 2 题' }));
  expect(form.textContent).toContain('现在继续吗？');
});

test('提交回答 waits until every question is answered; the dots show which are', async () => {
  const user = userEvent.setup();
  render(<QueryForm query={query} sessionId="ses_1" onChanged={vi.fn()} />);
  const submit = screen.getByRole<HTMLButtonElement>('button', { name: '提交回答' });
  expect(submit.disabled).toBe(true);
  await user.click(screen.getByRole('radio', { name: '跳过' }));
  expect(screen.getByRole('button', { name: '第 1 题（已答）' })).toBeTruthy();
  expect(screen.getByRole('button', { name: '第 2 题' })).toBeTruthy();
  expect(submit.disabled).toBe(true);
  // A custom reply with no words is not an answer yet.
  await user.click(screen.getByRole('button', { name: '下一题' }));
  await user.click(screen.getByRole('radio', { name: '自定义回复' }));
  expect(submit.disabled).toBe(true);
  await user.type(screen.getByRole('textbox', { name: '现在继续吗？（自定义回复）' }), '等我看完');
  expect(submit.disabled).toBe(false);
  await user.click(submit);
  expect(sent()).toEqual([
    { question_id: 'empty_workspace', value: { answer: 'choices', ids: ['skip'] } },
    { question_id: 'proceed', value: { answer: 'custom', text: '等我看完' } },
  ]);
});

test('a single question has no pager', () => {
  const one = { ...query, input: { questions: [query.input.questions[1]!] } } as UserQuery;
  render(<QueryForm query={one} sessionId="ses_1" onChanged={vi.fn()} />);
  expect(screen.queryByRole('group', { name: '切换问题' })).toBeNull();
  expect(screen.getByRole('form', { name: '回答当前问题' }).textContent).toContain(
    'Aura 在等你回复',
  );
});

test('declining answers every question as declined', async () => {
  const user = userEvent.setup();
  render(<QueryForm query={query} sessionId="ses_1" onChanged={vi.fn()} />);
  await user.click(screen.getByRole('button', { name: '拒绝回答' }));
  expect(sent()).toEqual([
    { question_id: 'empty_workspace', value: { answer: 'declined' } },
    { question_id: 'proceed', value: { answer: 'declined' } },
  ]);
});

test('an answer to a question that already ended says it was not delivered and what to do', async () => {
  answer.mockResolvedValueOnce({
    ok: false,
    error: { code: 'agent.query_missing', message: 'agent.query_missing' },
  });
  const user = userEvent.setup();
  render(<QueryForm query={query} sessionId="ses_1" onChanged={vi.fn()} />);
  await user.click(screen.getByRole('button', { name: '拒绝回答' }));
  expect((await screen.findByRole('alert')).textContent).toBe(
    '这个问题已经结束（超时、任务已停止或问题已更新），你的回答没有送达；请看最新的问题，或在输入框说明。',
  );
});

test('left alone for 30 s the countdown shows, and a touch starts the time again', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: false });
  vi.setSystemTime(new Date('2026-10-20T10:00:00Z'));
  let deadline = Date.now() + 60_000;
  const activity = vi.fn(async ({ activity }: { activity: string }) => {
    if (activity === 'touch') deadline = Date.now() + 60_000;
    return { ok: true, value: { deadline_at: new Date(deadline).toISOString() } };
  });
  Object.assign(window.aura.v1, { updateAgentWait: activity });
  render(
    <QueryForm
      query={query}
      sessionId="ses_1"
      deadlineAt="2026-10-20T10:01:00Z"
      onChanged={vi.fn()}
    />,
  );
  expect(screen.queryByRole('timer')).toBeNull();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(31_000);
  });
  expect(screen.getByRole('timer').textContent).toBe('29 秒后视为暂时没有回应');
  // Any touch: here a click on the card itself.
  await act(async () => {
    fireEvent.pointerDown(screen.getByRole('form', { name: '回答当前问题' }));
  });
  expect(screen.queryByRole('timer')).toBeNull();
  expect(activity.mock.calls.at(-1)?.[0]).toMatchObject({
    session_id: 'ses_1',
    target: { kind: 'query', agent_run_id: query.agent_run_id, query_id: query.query_id },
    activity: 'touch',
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(59_000);
  });
  expect(screen.getByRole('timer').textContent).toBe('1 秒后视为暂时没有回应');
});

test('a mounted question in a hidden document does not start the timer', async () => {
  const state = vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden');
  const activity = vi.fn(async () => ({
    ok: true,
    value: { deadline_at: new Date().toISOString() },
  }));
  Object.assign(window.aura.v1, { updateAgentWait: activity });
  try {
    render(<QueryForm query={query} sessionId="ses_1" onChanged={vi.fn()} />);
    expect(activity).not.toHaveBeenCalled();
    await act(async () => {
      state.mockReturnValue('visible');
      document.dispatchEvent(new Event('visibilitychange'));
    });
    expect(activity.mock.calls).toHaveLength(1);
    expect(activity).toHaveBeenCalledWith(expect.objectContaining({ activity: 'present' }));
  } finally {
    state.mockRestore();
  }
});

test('a question below the scroll area waits until the user scrolls it into view', async () => {
  let observe!: IntersectionObserverCallback;
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      constructor(callback: IntersectionObserverCallback) {
        observe = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  const activity = vi.fn(async () => ({
    ok: true,
    value: { deadline_at: new Date().toISOString() },
  }));
  Object.assign(window.aura.v1, { updateAgentWait: activity });
  try {
    render(<QueryForm query={query} sessionId="ses_1" onChanged={vi.fn()} />);
    expect(activity).not.toHaveBeenCalled();
    const target = screen.getByRole('form', { name: '回答当前问题' });
    const scroll = (visible: boolean) =>
      act(async () => {
        const intersectionRect = new DOMRect(0, 0, visible ? 100 : 0, visible ? 20 : 0);
        observe(
          [
            {
              target,
              isIntersecting: visible,
              intersectionRect,
              boundingClientRect: new DOMRect(0, 0, 100, 20),
              intersectionRatio: visible ? 1 : 0,
              rootBounds: null,
              time: performance.now(),
            },
          ],
          {} as IntersectionObserver,
        );
      });
    await scroll(false);
    expect(activity).not.toHaveBeenCalled();
    await scroll(true);
    expect(activity).toHaveBeenCalledWith(expect.objectContaining({ activity: 'present' }));
  } finally {
    vi.unstubAllGlobals();
  }
});

test('a keyboard-focused question pauses and releases after focus leaves the form', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-20T10:00:00Z'));
  let deadline = Date.now() + 60_000;
  const activity = vi.fn().mockImplementation(async (input) => {
    if (input.activity !== 'present') deadline = Date.now() + 60_000;
    return { ok: true, value: { deadline_at: new Date(deadline).toISOString() } };
  });
  Object.assign(window.aura.v1, { updateAgentWait: activity });
  render(
    <>
      <QueryForm
        query={query}
        sessionId="ses_1"
        deadlineAt="2026-10-20T10:01:00Z"
        onChanged={vi.fn()}
      />
      <button type="button">离开提问</button>
    </>,
  );
  await act(async () => {
    screen.getByRole('radio', { name: '跳过' }).focus();
  });
  expect(activity.mock.calls.some(([input]) => input.activity === 'focus')).toBe(true);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(70_000);
  });
  expect(screen.queryByRole('timer')).toBeNull();
  await act(async () => {
    screen.getByText('离开提问').focus();
  });
  expect(activity.mock.calls.at(-1)?.[0].activity).toBe('blur');
  await act(async () => {
    await vi.advanceTimersByTimeAsync(31_000);
  });
  expect(screen.getByRole('timer').textContent).toBe('29 秒后视为暂时没有回应');
});
