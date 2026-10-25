import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import type { AgentToolSettingsView, IpcErrorEnvelope } from '@aura/contracts/generated';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../../shared/ipc-request-failure';
import { useAgentConfiguration } from './useAgentConfiguration';

/**
 * What the EXISTING write consumer does with each answer Main can now hand it.
 *
 * This is the Renderer half of M2 and it is deliberately written against the envelopes the
 * supervisor really produces, not against a helper: a write whose caller timed out is answered by
 * Main either with the confirmed success (the write ran and Core confirmed it), with the Core's own
 * confirmed failure, or with the original pending-confirmation failure. The three cases below are
 * those three, in that order, and each asserts the state this store already owns.
 */
const committed: AgentToolSettingsView = {
  default_timeout_ms: 120_000,
  fixed_tool_ids: [],
  max_concurrent_tool_calls: 4,
  max_dynamic_tool_schemas: 50,
  max_retries: 3,
  revision: '4',
  tools: [],
};

const unconfirmed: IpcErrorEnvelope = {
  category: 'cancelled',
  code: REQUEST_TIMEOUT_CODE,
  correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  details: {
    command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  },
  message: 'Core request timed out: agent.tools.configure',
  retryable: false,
  terminal: 'cancelled',
};

/** Core's own stored failure for the invocation, as `runtime.command_outcome` reports it. */
const confirmedConflict: IpcErrorEnvelope = {
  category: 'conflict',
  code: 'tools.configuration_revision_conflict',
  correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  details: { actual_revision: '9' },
  message: '工具配置已被其他窗口修改。',
  retryable: false,
  terminal: 'failed',
};

const configure = vi.fn();
const windowWith = (configureAgentTools: typeof configure): void => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        configureAgentTools,
        readAgentToolSettings: vi.fn().mockResolvedValue({
          ok: true,
          value: { ...committed, revision: '3' },
        }),
      },
    },
  });
};

afterEach(() => {
  cleanup();
  configure.mockReset();
});

const submit = async (): Promise<
  ReturnType<typeof renderHook<ReturnType<typeof useAgentConfiguration>, unknown>>
> => {
  const rendered = renderHook(() => useAgentConfiguration());
  await waitFor(() => expect(rendered.result.current.view).toBeDefined());
  return rendered;
};

test('a confirmed success updates the existing snapshot instead of asking the user to verify', async () => {
  windowWith(configure);
  configure.mockResolvedValue({ ok: true, value: committed });
  const rendered = await submit();
  const before = rendered.result.current.view;
  expect(before?.revision).toBe('3');
  await act(async () => {
    await expect(rendered.result.current.commit({ max_concurrent_tool_calls: 4 })).resolves.toEqual(
      { view: committed },
    );
  });
  // The store's own state after: the confirmed configuration, an advanced revision, no error. This is
  // the same consumption a live reply has always had.
  expect(rendered.result.current.view).toEqual(committed);
  expect(rendered.result.current.loadError).toBeUndefined();
  expect(rendered.result.current.busy).toBe(false);
  expect(configure).toHaveBeenCalledTimes(1);
});

test('a confirmed failure shows Core’s real error and is not downgraded to pending', async () => {
  windowWith(configure);
  configure.mockResolvedValue({ error: confirmedConflict, ok: false });
  const rendered = await submit();
  const rejection = await act(async () => rendered.result.current.commit({}));
  // The confirmed terminal is definite, so the real message survives; the pending wording would tell
  // the user the outcome is unknown when Core has already said what it was.
  expect(rejection).toEqual({ error: confirmedConflict.message });
  expect(rendered.result.current.view?.revision).toBe('3');
});

test('an unconfirmed write keeps the pending state and sends nothing again', async () => {
  windowWith(configure);
  configure.mockResolvedValue({ error: unconfirmed, ok: false });
  const rendered = await submit();
  const rejection = await act(async () => rendered.result.current.commit({}));
  expect(rejection).toEqual({ error: expect.stringContaining('保存结果未确认') });
  // One submit, one write: the store never retries the command to learn its result.
  expect(configure).toHaveBeenCalledTimes(1);
});
