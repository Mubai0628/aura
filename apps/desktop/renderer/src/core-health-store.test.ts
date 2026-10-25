import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import type { CoreHealthProjectionChange } from '../../shared/aura-api';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../shared/ipc-request-failure';
import { resetCoreHealthStoreForTests, useCoreHealth } from './core-health-store';

const RUNTIME = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const OTHER_RUNTIME = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW';

const projection = (revision: number, statusMessage = 'Ready'): CoreHealthProjectionChange => ({
  payload: {
    read_only: false,
    restart_attempt: 0,
    safe_mode: false,
    state: 'ready',
    status_code: 'core.ready',
    status_message: statusMessage,
  },
  projection_kind: 'core_health',
  projection_scope: { type: 'global' },
  revision: String(revision),
  runtime_instance_id: RUNTIME,
});

/** What the transport settles a timed-out Core Health read with: one request, one reason. */
const requestTimeout = {
  category: 'cancelled',
  code: REQUEST_TIMEOUT_CODE,
  correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  details: {
    command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  },
  message: 'Core request timed out: projection.snapshot',
  retryable: false,
  terminal: 'cancelled',
};

const getCoreHealth = vi.fn();
let deliver: ((change: CoreHealthProjectionChange) => void) | undefined;

beforeEach(() => {
  getCoreHealth.mockReset();
  deliver = undefined;
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getCoreHealth,
        onCoreHealth: (listener: (change: CoreHealthProjectionChange) => void) => {
          deliver = listener;
          return () => {
            deliver = undefined;
          };
        },
      },
    },
  });
  resetCoreHealthStoreForTests();
});
afterEach(cleanup);

test('a request-local timeout is surfaced as that request and never as a Ready Core', async () => {
  getCoreHealth.mockResolvedValueOnce({ ok: true, value: projection(1) });
  const { result } = renderHook(() => useCoreHealth());
  await waitFor(() => expect(result.current.projection?.revision).toBe('1'));

  // A new Core generation makes the store read again instead of trusting the increment.
  getCoreHealth.mockResolvedValueOnce({ error: requestTimeout, ok: false });
  act(() => {
    deliver?.({ ...projection(1), runtime_instance_id: OTHER_RUNTIME });
  });
  await waitFor(() => expect(result.current.error).toBeDefined());

  // The precise reason and the queryable flag reach the view unchanged.
  expect(result.current.loading).toBe(false);
  expect(result.current.error?.code).toBe(REQUEST_TIMEOUT_CODE);
  expect(result.current.error?.terminal).toBe('cancelled');
  expect(result.current.error?.retryable).toBe(false);
  expect(result.current.error?.details.outcome_recovery).toBe(
    OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  );
  // A failed read is not an answer about Core: nothing may be presented as a fresh Ready state.
  expect(result.current.projection).toBeUndefined();
});

test('a newer increment replaces the projection and a stale one is ignored', async () => {
  getCoreHealth.mockResolvedValue({ ok: true, value: projection(1) });
  const { result } = renderHook(() => useCoreHealth());
  await waitFor(() => expect(result.current.projection?.revision).toBe('1'));

  act(() => {
    deliver?.(projection(1));
  });
  // The revision did not move forward, so this is not a new projection and not a reason to re-read.
  expect(result.current.projection?.revision).toBe('1');
  expect(getCoreHealth).toHaveBeenCalledTimes(1);

  act(() => {
    deliver?.(projection(2, 'Ready 2'));
  });
  expect(result.current.projection?.revision).toBe('2');
  expect(result.current.projection?.payload.status_message).toBe('Ready 2');
});

test('a snapshot from a new Core generation replaces the previous projection', async () => {
  getCoreHealth.mockResolvedValueOnce({ ok: true, value: projection(1) });
  const { result } = renderHook(() => useCoreHealth());
  await waitFor(() => expect(result.current.projection?.revision).toBe('1'));

  getCoreHealth.mockResolvedValueOnce({
    ok: true,
    value: { ...projection(4, 'Ready B'), runtime_instance_id: OTHER_RUNTIME },
  });
  act(() => {
    deliver?.({ ...projection(2), runtime_instance_id: OTHER_RUNTIME });
  });
  await waitFor(() => expect(result.current.projection?.runtime_instance_id).toBe(OTHER_RUNTIME));
  // The new generation's own revision is what the view shows, not the old one plus one.
  expect(result.current.projection?.revision).toBe('4');
  expect(result.current.projection?.payload.status_message).toBe('Ready B');
});

test('a read that produced no envelope is reported as a bridge failure', async () => {
  getCoreHealth.mockRejectedValue(new Error('bridge gone'));
  const { result } = renderHook(() => useCoreHealth());
  await waitFor(() => expect(result.current.error).toBeDefined());
  // Only the case with no typed answer is described in connection terms.
  expect(result.current.error).toMatchObject({
    category: 'transport',
    code: 'renderer.snapshot_failed',
    terminal: 'transport_lost',
  });
  expect(result.current.projection).toBeUndefined();
});
