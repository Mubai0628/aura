import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  chooseSessionReasoning,
  forgetSessionReasoning,
  sessionReasoning,
} from '../reasoning-effort';
import { useComposerSubmission } from './useComposerSubmission';

// A message goes with its conversation's
// thinking level for the model; a new conversation takes the level its draft chose; with none, the
// message names no level and the model's default applies.

const submit = vi.fn();
beforeEach(() => {
  localStorage.clear();
  forgetSessionReasoning();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { submitAgentMessage: submit, startAgent: vi.fn() } },
  });
});
afterEach(cleanup);

test("a message carries its conversation's level for the model, and none when it chose none", async () => {
  chooseSessionReasoning('ses_a', 'mdl_sol', 'max');
  const { result } = renderHook(() =>
    useComposerSubmission('ses_a', undefined, 'mdl_sol', false, vi.fn()),
  );
  act(() => result.current.setDraft('写一个走迷宫小游戏'));
  await act(() => result.current.send());
  expect(submit).toHaveBeenLastCalledWith({
    session_id: 'ses_a',
    text: '写一个走迷宫小游戏',
    destination: { lane: 'next_turn' },
    reasoning_effort: 'max',
  });

  const other = renderHook(() =>
    useComposerSubmission('ses_b', undefined, 'mdl_sol', false, vi.fn()),
  );
  act(() => other.result.current.setDraft('你好'));
  await act(() => other.result.current.send());
  expect(submit.mock.lastCall![0]).not.toHaveProperty('reasoning_effort');
});

test('a new conversation takes the level its draft chose', async () => {
  chooseSessionReasoning('draft_1', 'mdl_sol', 'high');
  const prepare = vi.fn(async () => 'ses_new');
  const { result } = renderHook(() =>
    useComposerSubmission('draft_1', undefined, 'mdl_sol', false, vi.fn(), {
      prepare,
      accepted: vi.fn(),
    }),
  );
  act(() => result.current.setDraft('从一个想法开始'));
  await act(() => result.current.send());
  expect(submit).toHaveBeenLastCalledWith(
    expect.objectContaining({ session_id: 'ses_new', reasoning_effort: 'high' }),
  );
  expect(sessionReasoning('ses_new', 'mdl_sol')).toBe('high');
});
