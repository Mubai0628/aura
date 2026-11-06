import type { AgentTurnView } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { StoppingRunContext } from './stopping';
import { TurnProcess } from './TurnProcess';
import { turnStatus } from './turn-state';

// Core answers a 停止 only once the run has really stopped; until then the turn read as still
// running, so a stop looked as if it had not been taken.

afterEach(cleanup);

const turn = (state: string, run = 'run-stopped'): AgentTurnView =>
  ({
    turn: {
      session_id: 'session',
      turn_task_id: `turn-${run}`,
      agent_run_id: run,
      root_user_event_id: 'root',
      state: { state },
      created_at: '2026-11-04T09:00:00Z',
      updated_at: '2026-11-04T09:00:01Z',
    },
    user_messages: [],
    assistant_messages: [],
  }) as unknown as AgentTurnView;

test.each(['running', 'pending', 'awaiting_approval', 'awaiting_query', 'tools_running'])(
  'a turn whose stop is under way says 正在停止 (%s)',
  (state) => {
    expect(turnStatus(turn(state), new Map(), false, true)).toBe('正在停止');
    expect(turnStatus(turn(state), new Map(), true, true)).toBe('正在停止');
  },
);

test('a turn that has ended says how, whether or not a stop was asked for', () => {
  for (const stopping of [false, true]) {
    expect(turnStatus(turn('cancelled'), new Map(), false, stopping)).toBe('已停止');
    expect(turnStatus(turn('completed'), new Map(), false, stopping)).toBe('已完成');
    expect(turnStatus(turn('failed'), new Map(), false, stopping)).toBe('任务中断');
  }
  expect(turnStatus(turn('running'), new Map(), false)).toBe('正在思考');
});

// With no stop asked for, the context holds nothing; a turn view that does not name its run must
// not match that nothing.
test('with no stop asked for, no turn says 正在停止, not even one that does not name its run', () => {
  const item = turn('running');
  delete (item.turn as { agent_run_id?: string }).agent_run_id;
  render(<TurnProcess item={item} calls={new Map()} awaitingContext={false} />);
  expect(screen.queryByText('正在停止')).toBeNull();
  expect(screen.getByText('正在思考')).toBeTruthy();
});

test('the status line of the stopping run alone says 正在停止', () => {
  const page = (item: AgentTurnView, stopping: string | undefined) => (
    <StoppingRunContext.Provider value={stopping}>
      <TurnProcess item={item} calls={new Map()} awaitingContext={false} />
    </StoppingRunContext.Provider>
  );
  const host = render(page(turn('running'), 'run-stopped'));
  expect(screen.getByText('正在停止')).toBeTruthy();
  host.rerender(page(turn('running'), 'run-other'));
  expect(screen.queryByText('正在停止')).toBeNull();
  expect(screen.getByText('正在思考')).toBeTruthy();
  host.rerender(page(turn('cancelled'), 'run-stopped'));
  expect(screen.queryByText('正在停止')).toBeNull();
  expect(screen.getByText('已停止')).toBeTruthy();
});
