import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { resetComposerDraftsForTests } from './composer-drafts';
import { useComposerSubmission } from './useComposerSubmission';

// The composer names no desktop: Core gives each turn the one Settings → 电脑操控 chooses when
// the turn starts.

const submit = vi.fn(),
  start = vi.fn(),
  init = vi.fn(),
  compact = vi.fn();
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  for (const method of [submit, start, init, compact])
    method.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        submitAgentMessage: submit,
        startAgent: start,
        startWorkspaceInit: init,
        startAgentCompaction: compact,
      },
    },
  });
});
afterEach(cleanup);

test('a message to a new conversation names no desktop', async () => {
  const { result } = renderHook(() =>
    useComposerSubmission('draft', undefined, 'model', false, vi.fn(), {
      prepare: async () => 'created',
      accepted: vi.fn(),
    }),
  );
  act(() => result.current.setDraft('fixture'));
  await act(() => result.current.send());
  expect(submit).toHaveBeenCalledWith(expect.objectContaining({ session_id: 'created' }));
  expect(submit.mock.calls[0]?.[0]).not.toHaveProperty('computer_mode');
});

test.each(['message', 'plan', 'init', 'compact'] as const)(
  'the %s entry names no desktop',
  async (kind) => {
    const { result } = renderHook(() =>
      useComposerSubmission('session', undefined, 'model', false, vi.fn()),
    );
    act(() => {
      result.current.setDraft('fixture');
      if (kind === 'plan') result.current.setMode('plan');
    });
    await act(() =>
      kind === 'init'
        ? result.current.sendInit()
        : kind === 'compact'
          ? result.current.sendCompaction()
          : result.current.send(),
    );
    const sent = { message: submit, plan: start, init, compact }[kind].mock.calls[0]?.[0];
    expect(sent).toMatchObject({ session_id: 'session' });
    expect(sent).not.toHaveProperty('computer_mode');
  },
);
