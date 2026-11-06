import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { validateAgentRequest } from '../../../shared/agent-validation';
import { chooseSessionSpeed, forgetSessionSpeed, sessionSpeed } from '../speed-mode';
import { useComposerSubmission } from './useComposerSubmission';

// A message goes with its conversation's speed mode for the model, which Core sends when the
// model's settings allow it; a new conversation takes the mode its draft chose; with none, the
// message names no mode and goes in the standard mode.

const submit = vi.fn();
const start = vi.fn();
beforeEach(() => {
  localStorage.clear();
  forgetSessionSpeed();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  start.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { submitAgentMessage: submit, startAgent: start } },
  });
});
afterEach(cleanup);

const SESSION = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const MODEL = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';

test("a message carries its conversation's mode for the model, and none when it chose none", async () => {
  chooseSessionSpeed(SESSION, MODEL, 'fast');
  const { result } = renderHook(() =>
    useComposerSubmission(SESSION, undefined, MODEL, false, vi.fn()),
  );
  act(() => result.current.setDraft('你好[test]'));
  await act(() => result.current.send());
  const sent = submit.mock.lastCall![0];
  expect(sent).toEqual({
    session_id: SESSION,
    text: '你好[test]',
    destination: { lane: 'next_turn' },
    speed_mode: 'fast',
  });
  expect(validateAgentRequest({ command: 'agent.submit', value: sent })).toBe(true);

  chooseSessionSpeed(SESSION, MODEL, null);
  act(() => result.current.setDraft('你好'));
  await act(() => result.current.send());
  expect(submit.mock.lastCall![0]).not.toHaveProperty('speed_mode');
});

test('a new conversation takes the mode its draft chose', async () => {
  chooseSessionSpeed('draft_1', MODEL, 'ultrafast');
  const prepare = vi.fn(async () => SESSION);
  const { result } = renderHook(() =>
    useComposerSubmission('draft_1', undefined, MODEL, false, vi.fn(), {
      prepare,
      accepted: vi.fn(),
    }),
  );
  act(() => result.current.setDraft('从一个想法开始'));
  await act(() => result.current.send());
  expect(submit).toHaveBeenLastCalledWith(
    expect.objectContaining({ session_id: SESSION, speed_mode: 'ultrafast' }),
  );
  expect(sessionSpeed(SESSION, MODEL)).toBe('ultrafast');
});

test('the main process refuses a mode that is not a plain word', () => {
  const value = {
    session_id: SESSION,
    text: '你好',
    destination: { lane: 'next_turn' },
    speed_mode: 'fast mode',
  };
  expect(validateAgentRequest({ command: 'agent.submit', value })).toBe(false);
  expect(
    validateAgentRequest({ command: 'agent.submit', value: { ...value, speed_mode: 'fast' } }),
  ).toBe(true);
});
