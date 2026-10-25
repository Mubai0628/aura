import type { AgentTurnView } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ContextCleared, ContextCompacted } from './ContextCompacted';
import { turnTimeline } from './turn-timeline';

afterEach(cleanup);

const step = (
  id: string,
  at: string,
  compacted?: { manual: boolean; retained_recent_tokens: number },
  cleared?: {
    cleared_tool_results: number;
    input_tokens_before: number;
    input_tokens_after: number;
  },
) =>
  ({
    model_step_id: id,
    at,
    text: '',
    reasoning: '想一想',
    tool_call_ids: [],
    ...(compacted ? { context_compacted: compacted } : {}),
    ...(cleared ? { context_cleared: cleared } : {}),
  }) as unknown as AgentTurnView['assistant_messages'][number];

test('a step prepared after a compaction is preceded by a line that says so', () => {
  const item = {
    turn: { root_user_event_id: 'evt_root' },
    user_messages: [],
    assistant_messages: [
      step('stp_1', '2026-10-20T10:00:00Z'),
      step('stp_2', '2026-10-20T10:01:00Z', { manual: false, retained_recent_tokens: 48_000 }),
    ],
  } as unknown as AgentTurnView;
  const blocks = turnTimeline(item, new Map(), undefined);
  expect(blocks.map((block) => block.kind)).toEqual(['process', 'compacted', 'process']);
});

test('the line names who compacted and how much recent history stayed', () => {
  const { unmount } = render(
    <ContextCompacted compacted={{ manual: false, retained_recent_tokens: 48_000 }} />,
  );
  expect(screen.getByRole('note').textContent).toBe(
    '上下文接近上限，已自动压缩：较早的内容换成了摘要，最近约 4.8万 tokens 原样保留。',
  );
  unmount();
  render(<ContextCompacted compacted={{ manual: true, retained_recent_tokens: 0 }} />);
  expect(screen.getByRole('note').textContent).toBe(
    '已按你的要求压缩上下文：较早的内容换成了摘要。',
  );
});

// The line says how large the request was before and after, and a step whose
// request only cleared older tool results says that.
test('the line gives the sizes before and after, and the results cleared first', () => {
  const { unmount } = render(
    <ContextCompacted
      compacted={{
        manual: false,
        retained_recent_tokens: 26_000,
        input_tokens_before: 83_000,
        input_tokens_after: 48_000,
        cleared_tool_results: 12,
      }}
    />,
  );
  expect(screen.getByRole('note').textContent).toBe(
    '上下文接近上限，已自动压缩（约 8.3万 → 4.8万 tokens）：12 个较早的工具结果已清理，较早的内容换成了摘要，最近约 2.6万 tokens 原样保留。',
  );
  unmount();
  render(
    <ContextCleared
      cleared={{ cleared_tool_results: 4, input_tokens_before: 83_000, input_tokens_after: 61_000 }}
    />,
  );
  expect(screen.getByRole('note').textContent).toBe(
    '上下文接近上限，已清理 4 个较早的工具结果（约 8.3万 → 6.1万 tokens）。',
  );
});

test('a step whose request cleared older results is preceded by its line', () => {
  const item = {
    turn: { root_user_event_id: 'evt_root' },
    user_messages: [],
    assistant_messages: [
      step('stp_1', '2026-10-22T10:00:00Z'),
      step('stp_2', '2026-10-22T10:01:00Z', undefined, {
        cleared_tool_results: 4,
        input_tokens_before: 83_000,
        input_tokens_after: 61_000,
      }),
    ],
  } as unknown as AgentTurnView;
  const blocks = turnTimeline(item, new Map(), undefined);
  expect(blocks.map((block) => block.kind)).toEqual(['process', 'compacted', 'process']);
});
