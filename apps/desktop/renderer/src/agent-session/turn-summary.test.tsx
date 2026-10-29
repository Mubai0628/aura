import type { AgentSessionProjection } from '@aura/contracts/generated';
import { cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from './Conversation';

//

const answer = '扫雷游戏已完成。';
const bytes = (text: string) => new TextEncoder().encode(text).length;

beforeEach(() => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentMessageText: vi.fn(async (request: { source: unknown }) => ({
          ok: true,
          value: {
            source: request.source,
            text: answer,
            total_bytes: bytes(answer),
            from_offset: 0,
            next_offset: null,
            body_id: null,
          },
        })),
        readAgentToolActivity: vi.fn(async () => ({
          ok: true,
          value: { session_id: 'session', next_before_call_id: null, calls: [] },
        })),
        listHookInvocations: vi.fn(async () => ({
          ok: true,
          value: { invocations: [], statistics: { calls: 0, succeeded: 0, failed: 0, blocked: 0 } },
        })),
      },
    },
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

/** A finished turn's time says how long ago it ended: 2 days 13 hours after this turn did. */
function twoDaysLater() {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(new Date('2026-10-25T13:38:50Z'));
}

const goal = (completed: boolean) => ({
  budget: { max_tokens: null, max_turns: null },
  completed,
  consumed_tokens: 1_877_962,
  consumed_turns: 9,
  latest_check_model_step: null,
  revision: '2',
  turn_task_id: 'turn',
});

function show(running: boolean, withGoal: boolean) {
  const projection = {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: null,
    turns: [
      {
        turn: {
          mode: withGoal ? 'goal' : 'normal',
          state: running ? { state: 'running' } : { state: 'completed', final_report: answer },
          session_id: 'session',
          turn_task_id: 'turn',
          revision: '3',
          root_user_event_id: 'root',
          created_at: '2026-10-23T00:00:00Z',
          updated_at: '2026-10-23T00:03:50Z',
        },
        final_text_source: running ? null : { kind: 'assistant_step', model_step_id: 'answer' },
        user_messages: [
          {
            event_id: 'root',
            text: '写一个扫雷小游戏，素材使用SVG',
            total_bytes: bytes('写一个扫雷小游戏，素材使用SVG'),
            origin: { origin: 'submit' },
          },
        ],
        assistant_messages: [
          {
            model_step_id: 'answer',
            model_profile_id: null,
            context_snapshot_id: 'mex_1',
            text: answer,
            reasoning: '',
            text_total_bytes: bytes(answer),
            reasoning_total_bytes: 0,
            partial: running,
            stream_sequence: null,
            tool_call_ids: [],
            at: '2026-10-23T00:03:49Z',
          },
        ],
        goal: withGoal ? goal(!running) : null,
      },
    ],
  } as unknown as AgentSessionProjection;
  render(
    <Conversation
      projection={projection}
      viewportRef={{ current: null }}
      onChanged={vi.fn()}
      loading={false}
      error={undefined}
      loadOlder={vi.fn()}
      loadTurnOlder={vi.fn()}
    />,
  );
  return document.querySelector('.agent-turn') as HTMLElement;
}

test('a finished turn’s time sits at the right of the line under its answer, not on its first line', async () => {
  twoDaysLater();
  const turn = show(false, false);
  const summary = await waitFor(() => {
    const found = turn.querySelector(':scope > .agent-turn-footer .agent-turn-summary');
    expect(found).not.toBeNull();
    return found as HTMLElement;
  });
  expect([...summary.children].map((part) => part.textContent)).toEqual([
    '2 天 13 小时之前',
    '3 分 50 秒',
  ]);
  expect(turn.querySelector('.agent-turn-status')?.textContent).not.toContain('3 分 50 秒');
});

test('a completed Goal sits just left of that time, and not above the answer', async () => {
  twoDaysLater();
  const turn = show(false, true);
  const summary = await waitFor(() => {
    const found = turn.querySelector(':scope > .agent-turn-footer .agent-turn-summary');
    expect(found).not.toBeNull();
    return found as HTMLElement;
  });
  const parts = [...summary.children].map((part) => part.textContent);
  expect(parts).toEqual([
    'Goal 已完成 · Token 1,877,962 / 未设预算 · 模型步数 9 / 未设预算',
    '2 天 13 小时之前',
    '3 分 50 秒',
  ]);
  expect(turn.querySelector(':scope > .agent-goal')).toBeNull();
});

test('a running turn keeps its time on its first line and leaves its Goal to the activity card', () => {
  const turn = show(true, true);
  expect(turn.querySelector('.agent-turn-footer')).toBeNull();
  expect(turn.querySelector('.agent-turn-status .agent-elapsed')).not.toBeNull();
  expect(turn.querySelector('.agent-goal')).toBeNull();
  expect(turn.textContent).not.toContain('Goal 进行中');
});
