import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { AgentTurnView, HookInvocationView } from '@aura/contracts/generated';
import { HookTurnSummary } from './HookTurnSummary';

const turn = (state: AgentTurnView['turn']['state'], updated = '2026-10-17T08:00:00Z') =>
  ({
    turn: {
      turn_task_id: 'turn_1',
      session_id: 'session_1',
      state,
      created_at: '2026-10-17T07:59:00Z',
      updated_at: updated,
    },
  }) as unknown as AgentTurnView;
const blocked: HookInvocationView = {
  invocation_id: 'hki_1',
  path: 'C:\\hooks\\gate',
  hook_name: 'gate',
  hook_version: '1.0.0',
  fingerprint: 'f',
  event: 'turn.before_start',
  started_at: '2026-10-17T07:59:01Z',
  duration_ms: 12,
  outcome: 'blocked',
  effect: 'action_blocked',
  reason: '今天不开工',
};

const list = vi.fn();
beforeEach(() => {
  list.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { listHookInvocations: list } },
  });
});
afterEach(cleanup);

test('an ended turn names the Hook that stopped it and its totals', async () => {
  list.mockResolvedValue({
    ok: true,
    value: {
      invocations: [blocked],
      statistics: { calls: 3, succeeded: 1, failed: 1, blocked: 1 },
    },
  });
  render(
    <HookTurnSummary item={turn({ state: 'recovery_required', reason: 'hooks.turn_blocked' })} />,
  );
  expect(await screen.findByText('Hook：调用 3 次，成功 1，失败 1，阻止 1')).toBeTruthy();
  expect(screen.getByRole('status').textContent).toBe(
    '被 Hook gate 阻止（轮次开始前）：今天不开工',
  );
  expect(list).toHaveBeenCalledWith({ path: null, turn_task_id: 'turn_1', limit: 20 });
});

test('a running turn asks nothing, and a turn without Hook calls shows nothing', async () => {
  const { container, rerender } = render(<HookTurnSummary item={turn({ state: 'running' })} />);
  expect(list).not.toHaveBeenCalled();
  list.mockResolvedValue({
    ok: true,
    value: { invocations: [], statistics: { calls: 0, succeeded: 0, failed: 0, blocked: 0 } },
  });
  rerender(<HookTurnSummary item={turn({ state: 'completed', final_report: '完成' })} />);
  await vi.waitFor(() => expect(list).toHaveBeenCalledTimes(1));
  expect(container.textContent).toBe('');
});

test('a bridge without Hooks leaves the turn untouched', () => {
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: {} } });
  const { container } = render(
    <HookTurnSummary item={turn({ state: 'completed', final_report: '完成' })} />,
  );
  expect(container.textContent).toBe('');
});
