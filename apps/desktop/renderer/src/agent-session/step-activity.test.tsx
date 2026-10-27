import type { AgentTurnView } from '@aura/contracts/generated';
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { StepActivity } from './StepActivity';
import { turnStatus } from './turn-state';

// "正文仍在生成，会持续补齐。" stayed for minutes with nothing moving, "让人感觉就像
// 卡住了一样". The running turn says what it is doing: which tool call it is writing and how much
// has arrived, nothing while its words are arriving, and how long it has been thinking otherwise.

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

type Message = Partial<AgentTurnView['assistant_messages'][number]>;

const turn = (state: string, messages: Message[]): AgentTurnView =>
  ({
    turn: { turn_task_id: 'turn', session_id: 'session', state: { state } },
    user_messages: [],
    assistant_messages: messages.map((message, index) => ({
      model_step_id: `step-${index}`,
      partial: false,
      text: '',
      text_total_bytes: 0,
      reasoning: '',
      reasoning_total_bytes: 0,
      tool_call_ids: [],
      ...message,
    })),
    todo: { items: [] },
  }) as unknown as AgentTurnView;

test('a step writing a tool call says which, for which file, and how much has arrived', () => {
  render(
    <StepActivity
      item={turn('running', [
        {
          partial: true,
          tool_input: [
            {
              title: '写入文件',
              target: 'web/pelican_bicycle.html',
              received_bytes: 18_637,
              complete: false,
            },
          ],
        },
      ])}
    />,
  );
  expect(screen.getByRole('status').textContent).toBe(
    '正在编写「写入文件」pelican_bicycle.html · 已生成 18.2 KB',
  );
});

test('words arriving need no line; a pause says the model is thinking, and for how long', () => {
  vi.useFakeTimers();
  const running = (text: number) =>
    turn('running', [{ partial: true, text_total_bytes: text, reasoning_total_bytes: 5 }]);
  const view = render(<StepActivity item={running(10)} />);
  expect(screen.queryByRole('status')).toBeNull();
  act(() => vi.advanceTimersByTime(2_000));
  expect(screen.getByRole('status').textContent).toBe('正在思考下一步 · 2 秒');
  act(() => vi.advanceTimersByTime(3_000));
  expect(screen.getByRole('status').textContent).toBe('正在思考下一步 · 5 秒');
  view.rerender(<StepActivity item={running(40)} />);
  expect(screen.queryByRole('status')).toBeNull();
});

test('a turn that is not running says nothing here', () => {
  render(
    <StepActivity
      item={turn('awaiting_approval', [
        {
          partial: true,
          tool_input: [{ title: '写入文件', received_bytes: 10, complete: false }],
        },
      ])}
    />,
  );
  expect(screen.queryByRole('status')).toBeNull();
});

// The count belongs
// to the step, not to the view showing it.
test('a step that has written nothing yet counts from when it started', () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-21T12:00:00Z'));
  const started = turn('running', [
    { model_step_id: 'step-fresh', partial: true, at: '2026-10-21T11:58:30Z' },
  ]);
  render(<StepActivity item={started} />);
  expect(screen.getByRole('status').textContent).toBe('正在思考下一步 · 90 秒');
});

test('switching away and back keeps counting instead of starting again', () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-21T12:00:00Z'));
  const thinking = turn('running', [
    { model_step_id: 'step-kept', partial: true, text_total_bytes: 12, at: '2026-10-21T11:59:00Z' },
  ]);
  const first = render(<StepActivity item={thinking} />);
  act(() => vi.advanceTimersByTime(7_000));
  expect(screen.getByRole('status').textContent).toBe('正在思考下一步 · 7 秒');
  first.unmount();
  act(() => vi.advanceTimersByTime(3_000));
  render(<StepActivity item={thinking} />);
  expect(screen.getByRole('status').textContent).toBe('正在思考下一步 · 10 秒');
  // And it goes on from there: the view that mounted again keeps the step's own start.
  act(() => vi.advanceTimersByTime(1_000));
  expect(screen.getByRole('status').textContent).toBe('正在思考下一步 · 11 秒');
});

// A long compaction is a wait of its own: it was shown as the model thinking for minutes.
test('a step waiting on a compaction says so, how long its part has run, and the parts done', () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-21T12:00:00Z'));
  const compacting = turn('running', [
    {
      model_step_id: 'step-compacting',
      partial: true,
      at: '2026-10-21T11:57:00Z',
      context_compacting: { since: '2026-10-21T11:58:11Z', finished_parts: 2 },
    },
  ]);
  render(<StepActivity item={compacting} />);
  expect(screen.getByRole('status').textContent).toBe('正在压缩上下文 · 109 秒 · 已完成 2 段');
  expect(turnStatus(compacting, new Map(), false)).toBe('正在压缩上下文');
});
