import type { RuntimeReadiness } from '@aura/contracts/generated';
import { afterEach, expect, it, vi } from 'vitest';
import { RuntimeReadinessStore } from './runtime-readiness-store';
import type { ResultEnvelope } from '../../shared/aura-api';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../shared/ipc-request-failure';
const value: RuntimeReadiness = {
  schema_version: 1,
  revision: '1',
  core_health: 'ready',
  capabilities: [],
  backends: [],
  extensions: [],
};
afterEach(() => vi.useRealTimers());
it('shares an in-flight read across navigation and reuses its result until the refresh interval', async () => {
  vi.useFakeTimers();
  let resolve!: (v: ResultEnvelope<RuntimeReadiness>) => void;
  const read = vi.fn(
    () =>
      new Promise<ResultEnvelope<RuntimeReadiness>>((done) => {
        resolve = done;
      }),
  );
  const store = new RuntimeReadinessStore(read, 1000);
  store.subscribe(vi.fn())();
  const stop = store.subscribe(vi.fn());
  expect(read).toHaveBeenCalledTimes(1);
  resolve({ ok: true, value });
  await vi.advanceTimersByTimeAsync(0);
  stop();
  const stopAgain = store.subscribe(vi.fn());
  expect(store.getSnapshot().value).toEqual(value);
  await vi.advanceTimersByTimeAsync(900);
  expect(read).toHaveBeenCalledTimes(1);
  stopAgain();
});
it('keeps the last snapshot with an explicit stale error when a refresh fails', async () => {
  vi.useFakeTimers();
  const read = vi
    .fn()
    .mockResolvedValueOnce({ ok: true, value })
    .mockRejectedValueOnce(new Error('offline'));
  const store = new RuntimeReadinessStore(read, 1000);
  const stop = store.subscribe(vi.fn());
  await vi.advanceTimersByTimeAsync(1000);
  expect(store.getSnapshot().value).toEqual(value);
  expect(store.getSnapshot().error).toContain('暂时无法更新');
  stop();
});
it('reports a request-local timeout as that read failing, not as a lost connection', async () => {
  vi.useFakeTimers();
  const read = vi
    .fn()
    .mockResolvedValueOnce({ ok: true, value })
    .mockResolvedValueOnce({
      error: {
        category: 'cancelled',
        code: REQUEST_TIMEOUT_CODE,
        correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        details: {
          command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
          outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
        },
        message: 'Core request timed out: runtime.readiness',
        retryable: false,
        terminal: 'cancelled',
      },
      ok: false,
    });
  const store = new RuntimeReadinessStore(read, 1000);
  const stop = store.subscribe(vi.fn());
  await vi.advanceTimersByTimeAsync(1000);
  // The last snapshot stays visible, and the reason names the timed-out read rather than the link.
  expect(store.getSnapshot().value).toEqual(value);
  expect(store.getSnapshot().error).toContain('超时');
  expect(store.getSnapshot().error).not.toContain('检查连接');
  expect(store.getSnapshot().errorEnvelope?.code).toBe(REQUEST_TIMEOUT_CODE);
  expect(store.getSnapshot().errorEnvelope?.details.outcome_recovery).toBe(
    OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  );
  stop();
});
it('rejects an old runtime response and reads the new runtime without overlapping calls', async () => {
  vi.useFakeTimers();
  let resolve!: (v: ResultEnvelope<RuntimeReadiness>) => void;
  const read = vi.fn(
    () =>
      new Promise<ResultEnvelope<RuntimeReadiness>>((done) => {
        resolve = done;
      }),
  );
  const store = new RuntimeReadinessStore(read);
  store.setConnection('old');
  const stop = store.subscribe(vi.fn());
  store.setConnection('new');
  resolve({ ok: true, value });
  await vi.advanceTimersByTimeAsync(0);
  expect(store.getSnapshot()).toEqual({ loading: true });
  expect(read).toHaveBeenCalledTimes(2);
  resolve({ ok: true, value: { ...value, revision: '0' } });
  await vi.advanceTimersByTimeAsync(0);
  expect(store.getSnapshot().value?.revision).toBe('0');
  stop();
});
