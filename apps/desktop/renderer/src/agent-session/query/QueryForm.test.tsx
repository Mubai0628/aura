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
const touch = vi.fn();
beforeEach(() => {
  answer.mockReset().mockResolvedValue({ ok: true, value: {} });
  touch.mockReset().mockImplementation(async () => ({
    ok: true,
    value: { query_id: query.query_id, deadline_at: new Date(Date.now() + 60_000).toISOString() },
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { answerAgentQuery: answer, touchAgentQuery: touch } },
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
  const customs = screen.getAllByRole('radio', { name: '自定义回复' });
  expect(customs).toHaveLength(2);
  await user.click(customs[0]!);
  await user.type(
    screen.getByRole('textbox', { name: '工作区是空的，你希望我怎么处理？（自定义回复）' }),
    '先列出目录再决定',
  );
  await user.click(screen.getByRole('radio', { name: '确认' }));
  await user.click(screen.getByRole('button', { name: '提交回答' }));
  expect(sent()).toEqual([
    { question_id: 'empty_workspace', value: { answer: 'custom', text: '先列出目录再决定' } },
    { question_id: 'proceed', value: { answer: 'confirm', accepted: true } },
  ]);
});

test('a question still unanswered is named, and nothing is sent', async () => {
  const user = userEvent.setup();
  render(<QueryForm query={query} sessionId="ses_1" onChanged={vi.fn()} />);
  await user.click(screen.getByRole('radio', { name: '跳过' }));
  await user.click(screen.getByRole('button', { name: '提交回答' }));
  expect(screen.getByRole('alert').textContent).toBe('第 2 个问题：选择一个回复。');
  expect(answer).not.toHaveBeenCalled();
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

test('left alone for 30 s the countdown shows, and a touch starts the time again', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: false });
  vi.setSystemTime(new Date('2026-10-20T10:00:00Z'));
  render(
    <QueryForm
      query={query}
      sessionId="ses_1"
      deadlineAt="2026-10-20T10:01:00Z"
      onChanged={vi.fn()}
    />,
  );
  expect(screen.queryByRole('timer')).toBeNull();
  act(() => {
    vi.advanceTimersByTime(31_000);
  });
  expect(screen.getByRole('timer').textContent).toBe('29 秒后视为暂时没有回应');
  // Any touch: here a click on the card itself.
  fireEvent.pointerDown(screen.getByRole('form', { name: '回答当前问题' }));
  expect(screen.queryByRole('timer')).toBeNull();
  expect(touch).toHaveBeenCalledTimes(1);
  expect(touch.mock.calls[0]![0]).toEqual({
    session_id: 'ses_1',
    agent_run_id: query.agent_run_id,
    query_id: query.query_id,
  });
  // A burst of touches reaches Core once more, after it settles.
  fireEvent.keyDown(screen.getByRole('radio', { name: '跳过' }));
  fireEvent.keyDown(screen.getByRole('radio', { name: '跳过' }));
  expect(touch).toHaveBeenCalledTimes(1);
  await act(async () => {
    vi.advanceTimersByTime(2_000);
  });
  expect(touch).toHaveBeenCalledTimes(2);
  act(() => {
    vi.advanceTimersByTime(59_000);
  });
  expect(screen.getByRole('timer').textContent).toBe('1 秒后视为暂时没有回应');
});
