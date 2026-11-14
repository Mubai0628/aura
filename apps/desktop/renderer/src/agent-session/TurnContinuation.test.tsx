import type { AgentSessionProjection } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from './Conversation';
import { restartTime } from './turn-continuation';

const continueAgent = vi.fn();
beforeEach(() => {
  continueAgent.mockReset().mockResolvedValue({ ok: true, value: {} });
  class QuietObserver {
    observe(): void {
      /* nothing is ever in view */
    }
    disconnect(): void {
      /* nothing to release */
    }
  }
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: QuietObserver,
  });
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: { continueAgent } } });
});
afterEach(cleanup);

const turn = (id: string, state: Record<string, unknown>, extra: Record<string, unknown> = {}) => ({
  turn: { mode: 'normal', state, session_id: 'session', turn_task_id: id, revision: '3' },
  user_messages: [],
  assistant_messages: [],
  plan: null,
  ...extra,
});

function show(turns: unknown[], active: unknown = null) {
  const projection = {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active,
    turns,
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
}

const cutOff = { state: 'recovery_required', reason: 'agent.core_restarted' };

test('the last turn a restart cut off has 继续, which asks Core to continue that turn', async () => {
  show([turn('cut', cutOff)]);
  fireEvent.click(screen.getByRole('button', { name: '继续' }));
  await waitFor(() => expect(continueAgent).toHaveBeenCalledTimes(1));
  expect(continueAgent).toHaveBeenCalledWith({ session_id: 'session', turn_task_id: 'cut' });
});

test('a 继续 Core refused says why, that the turn stays, and what to do', async () => {
  continueAgent.mockResolvedValueOnce({
    ok: false,
    error: {
      code: 'agent.continuation_requires_checkpoint',
      message: 'agent.continuation_requires_checkpoint',
    },
  });
  show([turn('cut', cutOff)]);
  fireEvent.click(screen.getByRole('button', { name: '继续' }));
  await waitFor(() =>
    expect(document.querySelector('.agent-continue [role="alert"]')?.textContent).toBe(
      '这一轮没有可以接着做的位置，请在输入框说明下一步。',
    ),
  );
  cleanup();
  continueAgent.mockResolvedValueOnce({
    ok: false,
    error: { code: 'agent.store_busy', message: 'agent.store_busy' },
  });
  show([turn('cut', cutOff)]);
  fireEvent.click(screen.getByRole('button', { name: '继续' }));
  await waitFor(() =>
    expect(document.querySelector('.agent-continue [role="alert"]')?.textContent).toBe(
      '没能继续，任务仍停在这里；可以再点“继续”，或在输入框说明下一步。',
    ),
  );
});

test('继续 is only on the last cut-off turn, and only while nothing runs', () => {
  show([turn('cut', cutOff), turn('later', { state: 'completed', final_report: '好了' })]);
  expect(screen.queryByRole('button', { name: '继续' })).toBeNull();
  cleanup();
  show([turn('cut', cutOff)], { run: { state: { state: 'model_running' } } });
  expect(screen.queryByRole('button', { name: '继续' })).toBeNull();
  cleanup();
  show([turn('failed', { state: 'recovery_required', reason: 'agent.transport_interrupted' })]);
  expect(screen.queryByRole('button', { name: '继续' })).toBeNull();
});

test('a turn Aura continued by itself says when Core restarted and why', () => {
  const at = new Date();
  at.setHours(21, 17, 0, 0);
  show([
    turn(
      'continued',
      { state: 'completed', final_report: '好了' },
      { auto_continued: { cause: 'unresponsive', restarted_at: at.toISOString() } },
    ),
  ]);
  expect(screen.getByRole('note').textContent).toBe(
    'Aura Core 在 21:17 重启（长时间无响应），已自动继续',
  );
});

test('a restart on another day names the day', () => {
  const now = new Date(2026, 9, 29, 9, 5);
  expect(restartTime(new Date(2026, 9, 28, 21, 7).toISOString(), now)).toBe('10月28日 21:07');
  expect(restartTime(new Date(2026, 9, 29, 8, 3).toISOString(), now)).toBe('08:03');
});
