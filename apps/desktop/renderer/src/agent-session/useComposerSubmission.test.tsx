import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../../shared/ipc-request-failure';
import { useComposerSubmission } from './useComposerSubmission';

const submit = vi.fn();
const start = vi.fn();
beforeEach(() => {
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  start.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { submitAgentMessage: submit, startAgent: start } },
  });
});
afterEach(cleanup);

test('an idle send without a chosen model preserves the draft without submitting or pausing a queue', async () => {
  const { result } = renderHook(() =>
    useComposerSubmission('unbound', undefined, undefined, false, vi.fn()),
  );
  act(() => result.current.setDraft('先选择模型再发送'));
  await act(() => result.current.send());
  expect(submit).not.toHaveBeenCalled();
  expect(start).not.toHaveBeenCalled();
  expect(result.current.draft).toBe('先选择模型再发送');
  expect(result.current.error).toBe('请先选择已配置的模型。');
  expect(result.current.needsReview).toBe(false);
});

test('slash text stays queueable and an explicit mode switch does not enter the queue', async () => {
  const { result } = renderHook(() =>
    useComposerSubmission('routing', 'active-plan', 'model', false, vi.fn()),
  );
  act(() => result.current.setDraft('/goal a literal message'));
  await act(() => result.current.send());
  expect(submit).toHaveBeenCalledWith({
    session_id: 'routing',
    text: '/goal a literal message',
    destination: { lane: 'next_turn' },
  });
  expect(start).not.toHaveBeenCalled();
  act(() => {
    result.current.setDraft('switch to goal now');
    result.current.setMode('goal');
  });
  await act(() => result.current.send());
  expect(start).not.toHaveBeenCalled();
  expect(submit).toHaveBeenCalledTimes(1);
  expect(result.current.error).toContain('不会进入队列或追加');
  expect(result.current.draft).toBe('switch to goal now');
  act(() => {
    result.current.setMode('normal');
    result.current.setDraft('/plan literal supplement');
  });
  // While a task runs every message waits in the queue; the queue appends it at once.
  await act(() => result.current.send());
  expect(submit).toHaveBeenLastCalledWith({
    session_id: 'routing',
    text: '/plan literal supplement',
    destination: { lane: 'next_turn' },
  });
  expect(start).not.toHaveBeenCalled();
});

test('special entry requires an idle session without queued input', async () => {
  const { result, rerender } = renderHook(
    ({ pending }) =>
      useComposerSubmission('mode-entry', undefined, 'chosen-model', pending, vi.fn()),
    { initialProps: { pending: true } },
  );
  act(() => {
    result.current.setDraft('make a plan');
    result.current.setMode('plan');
  });
  await act(() => result.current.send());
  expect(start).not.toHaveBeenCalled();
  expect(submit).not.toHaveBeenCalled();
  expect(result.current.error).toContain('不会进入队列或追加');
  expect(result.current.draft).toBe('make a plan');
  rerender({ pending: false });
  act(() => {
    result.current.setDraft('explicit plan');
    result.current.setMode('plan');
  });
  await act(() => result.current.send());
  expect(start).toHaveBeenCalledWith({
    session_id: 'mode-entry',
    model_profile_id: 'chosen-model',
    text: 'explicit plan',
    mode: 'plan',
    goal_budget: null,
  });
});

test('unknown submission keeps the draft and resend block across a session remount', async () => {
  submit.mockRejectedValueOnce(new Error('transport lost'));
  const first = renderHook(() =>
    useComposerSubmission('uncertain', undefined, 'chosen-model', false, vi.fn()),
  );
  act(() => first.result.current.setDraft('keep my input'));
  await act(() => first.result.current.send());
  first.unmount();
  const second = renderHook(() =>
    useComposerSubmission('uncertain', undefined, 'chosen-model', false, vi.fn()),
  );
  expect(second.result.current.draft).toBe('keep my input');
  expect(second.result.current.needsReview).toBe(true);
  await act(() => second.result.current.send());
  expect(submit).toHaveBeenCalledTimes(1);
  act(() => second.result.current.acknowledge());
  await act(() => second.result.current.send());
  expect(submit).toHaveBeenCalledTimes(2);
});

test('a request-local timeout blocks the resend until the result is acknowledged', async () => {
  submit.mockResolvedValueOnce({
    error: {
      category: 'cancelled',
      code: REQUEST_TIMEOUT_CODE,
      correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      details: {
        command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
      },
      message: 'Core request timed out: agent.submit',
      retryable: false,
      terminal: 'cancelled',
    },
    ok: false,
  });
  const { result } = renderHook(() =>
    useComposerSubmission('timeout', undefined, 'chosen-model', false, vi.fn()),
  );
  act(() => result.current.setDraft('do not resend yet'));
  await act(() => result.current.send());
  // The message may already have been accepted: another send is blocked until the user confirms.
  expect(result.current.needsReview).toBe(true);
  expect(result.current.error).toBeDefined();
  await act(() => result.current.send());
  expect(submit).toHaveBeenCalledTimes(1);
  act(() => result.current.acknowledge());
  await act(() => result.current.send());
  expect(submit).toHaveBeenCalledTimes(2);
});

test('two submissions before React renders issue only one command', async () => {
  let resolve: (value: unknown) => void = () => undefined;
  submit.mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const { result } = renderHook(() =>
    useComposerSubmission('one-command', undefined, 'chosen-model', false, vi.fn()),
  );
  act(() => result.current.setDraft('once'));
  await act(async () => {
    const pending = result.current.send();
    await result.current.send();
    expect(submit).toHaveBeenCalledTimes(1);
    resolve({ ok: true, value: {} });
    await pending;
  });
});
