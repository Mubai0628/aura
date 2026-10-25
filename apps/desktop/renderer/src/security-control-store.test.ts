import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import type { CoreLifecycleState, SessionPermissionProjection } from '@aura/contracts/generated';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { CoreHealthProjectionChange, ResultEnvelope } from '../../shared/aura-api';
import { resetCoreHealthStoreForTests } from './core-health-store';
import { useSecurityControl } from './security-control-store';

beforeEach(() => {
  resetCoreHealthStoreForTests();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
const permission = (
  mode: SessionPermissionProjection['mode'] = 'workspace_read_write',
  revision = 1,
  session_id = 'session-a',
): SessionPermissionProjection => ({
  active_grant_count: 0,
  mode,
  revision,
  session_id,
  transition: 'settled',
});
const ok = <T>(value: T): ResultEnvelope<T> => ({ ok: true, value });
const CORE_RUNTIME_ID = 'run-health-a';
const healthProjection = (
  state: CoreLifecycleState,
  revision: number,
  runtime = CORE_RUNTIME_ID,
): CoreHealthProjectionChange => ({
  payload: {
    read_only: false,
    restart_attempt: 0,
    safe_mode: false,
    state,
    status_code: `core.${state}`,
    status_message: `fixture ${state}`,
  },
  projection_kind: 'core_health',
  projection_scope: { type: 'global' },
  revision: String(revision),
  runtime_instance_id: runtime,
});
const P04_UNSUPPORTED: ResultEnvelope<never> = {
  ok: false,
  error: {
    category: 'unavailable',
    code: 'main_request_failed',
    correlation_id: 'cor_00000000000000000000000000',
    details: {},
    message: 'Core does not support the P04 Security Control Plane interface',
    retryable: true,
    terminal: 'failed',
  },
};
function fixture(initialHealthState: CoreLifecycleState = 'ready') {
  let healthState = initialHealthState;
  let healthRevision = 1;
  let healthRuntime = CORE_RUNTIME_ID;
  let listener:
    | ((change: {
        projection_kind: 'session_permission';
        payload: SessionPermissionProjection;
      }) => void)
    | undefined;
  let healthListener: ((change: CoreHealthProjectionChange) => void) | undefined;
  const api = {
    listPendingApprovals: vi.fn(async () => ok([])),
    getHostPreparationStatus: vi.fn(async () => ok(undefined)),
    listQuarantine: vi.fn(async () => ok([])),
    listPermissionGrants: vi.fn(async () => ok([])),
    getSessionPermissionState: vi.fn(async ({ session_id }: { session_id: string }) =>
      ok(permission('workspace_read_write', 1, session_id)),
    ),
    getCoreHealth: vi.fn(async () =>
      ok(healthProjection(healthState, healthRevision, healthRuntime)),
    ),
    setSessionPermissionMode: vi.fn(
      async ({
        mode,
        expected_revision,
        session_id,
      }: {
        mode: SessionPermissionProjection['mode'];
        expected_revision: number;
        session_id: string;
      }) => ok(permission(mode, expected_revision + 1, session_id)),
    ),
    onCoreHealth: (next: typeof healthListener) => {
      healthListener = next;
      return () => {
        healthListener = undefined;
      };
    },
    onSecurityControl: (next: typeof listener) => {
      listener = next;
      return () => {
        listener = undefined;
      };
    },
  };
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
  return {
    api,
    emit: (payload: SessionPermissionProjection) =>
      listener?.({ projection_kind: 'session_permission', payload }),
    emitHealth: (state: CoreLifecycleState, runtime = healthRuntime) => {
      healthState = state;
      healthRuntime = runtime;
      healthRevision += 1;
      healthListener?.(healthProjection(state, healthRevision, runtime));
    },
    failAllP04: () => {
      api.listPendingApprovals.mockResolvedValue(P04_UNSUPPORTED);
      api.getHostPreparationStatus.mockResolvedValue(P04_UNSUPPORTED);
      api.listQuarantine.mockResolvedValue(P04_UNSUPPORTED);
      api.listPermissionGrants.mockResolvedValue(P04_UNSUPPORTED);
      api.getSessionPermissionState.mockResolvedValue(P04_UNSUPPORTED);
    },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

test('Full Access requires explicit confirmation bound to the displayed session revision', async () => {
  const { api } = fixture();
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  await waitFor(() => expect(result.current.permission?.revision).toBe(1));
  await act(async () => result.current.setMode('full_access'));
  expect(api.setSessionPermissionMode).not.toHaveBeenCalled();
  expect(result.current.fullAccessConfirmation).toBe(true);
  await act(async () => result.current.confirmFullAccess());
  expect(api.setSessionPermissionMode).toHaveBeenCalledExactlyOnceWith({
    session_id: 'session-a',
    expected_revision: 1,
    mode: 'full_access',
  });
  expect(result.current.permission?.mode).toBe('full_access');
  await act(async () => result.current.setMode('full_access'));
  expect(api.setSessionPermissionMode).toHaveBeenCalledTimes(1);
});

test('cancelled or stale-session confirmation cannot change the new session', async () => {
  const { api } = fixture();
  const { result, rerender } = renderHook(({ id }) => useSecurityControl('workspace', id), {
    initialProps: { id: 'session-a' },
  });
  await waitFor(() => expect(result.current.permission).toBeDefined());
  await act(async () => result.current.setMode('full_access'));
  act(() => result.current.cancelFullAccess());
  await act(async () => result.current.confirmFullAccess());
  expect(api.setSessionPermissionMode).not.toHaveBeenCalled();
  await act(async () => result.current.setMode('full_access'));
  rerender({ id: 'session-b' });
  await waitFor(() => expect(result.current.permission?.session_id).toBe('session-b'));
  await act(async () => result.current.confirmFullAccess());
  expect(result.current.fullAccessConfirmation).toBe(false);
  expect(api.setSessionPermissionMode).not.toHaveBeenCalled();
});

test('late state refresh and mode reply cannot overwrite a newer authority event', async () => {
  const { api, emit } = fixture();
  const reply = deferred<ReturnType<typeof ok<SessionPermissionProjection>>>();
  api.setSessionPermissionMode.mockImplementationOnce(() => reply.promise);
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  await waitFor(() => expect(result.current.permission).toBeDefined());
  let change!: Promise<void>;
  act(() => {
    change = result.current.setMode('read_only');
  });
  act(() => emit(permission('read_only', 3)));
  await act(async () => {
    reply.resolve(ok(permission('workspace_read_write', 2)));
    await change;
  });
  expect(result.current.permission?.mode).toBe('read_only');
  expect(result.current.permission?.revision).toBe(3);
  await act(async () => result.current.refresh());
  expect(result.current.permission?.revision).toBe(3);
});

test('uncertain stop reloads the already tightened mode and exposes retry without widening', async () => {
  const { api, emit } = fixture();
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  await waitFor(() => expect(result.current.permission).toBeDefined());
  const lower = { ...permission('read_only', 2), transition: 'stop_unconfirmed' as const };
  api.getSessionPermissionState.mockResolvedValue(ok(lower));
  // The bridge can lose a response after Core has durably applied the ceiling.
  api.setSessionPermissionMode.mockRejectedValueOnce(new Error('stop not confirmed'));
  await act(async () => result.current.setMode('read_only'));
  expect(result.current.permission).toEqual(lower);
  expect(result.current.error).toBe('stop not confirmed');
  await act(async () => result.current.setMode('read_only'));
  expect(api.setSessionPermissionMode).toHaveBeenCalledTimes(2);
  act(() => emit({ ...lower, revision: 3, transition: 'stopping' }));
  expect(result.current.permission?.transition).toBe('settled');
});

test('a pending refresh from another session is discarded', async () => {
  const { api } = fixture();
  const { result, rerender } = renderHook(({ id }) => useSecurityControl('workspace', id), {
    initialProps: { id: 'session-a' },
  });
  await waitFor(() => expect(result.current.permission).toBeDefined());
  const read = deferred<ReturnType<typeof ok<SessionPermissionProjection>>>();
  api.getSessionPermissionState.mockImplementationOnce(() => read.promise);
  let refresh!: Promise<void>;
  act(() => {
    refresh = result.current.refresh();
  });
  rerender({ id: 'session-b' });
  await waitFor(() => expect(result.current.permission?.session_id).toBe('session-b'));
  await act(async () => {
    read.resolve(ok(permission('full_access', 99)));
    await refresh;
  });
  expect(result.current.permission).toEqual(permission('workspace_read_write', 1, 'session-b'));
});

test('slow Core handshake keeps the security surface waiting instead of reporting unsupported', async () => {
  const { api, emitHealth, failAllP04 } = fixture('starting');
  failAllP04();
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  // The renderer mounted before the handshake finished: Main is still answering the
  // transient "interface unsupported", which must read as "waiting", never an error.
  await waitFor(() => expect(result.current.loading).toBe(true));
  expect(result.current.error).toBeUndefined();
  expect(result.current.permission).toBeUndefined();
  // Handshake completes; the same calls now succeed and the store settles by itself.
  api.listPendingApprovals.mockResolvedValue(ok([]));
  api.getHostPreparationStatus.mockResolvedValue(ok(undefined));
  api.listQuarantine.mockResolvedValue(ok([]));
  api.listPermissionGrants.mockResolvedValue(ok([]));
  api.getSessionPermissionState.mockImplementation(async ({ session_id }: { session_id: string }) =>
    ok(permission('workspace_read_write', 1, session_id)),
  );
  act(() => emitHealth('ready'));
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(result.current.error).toBeUndefined();
  expect(result.current.permission?.session_id).toBe('session-a');
});

test('Core recovery-in-progress waits without surfacing the transient rejection', async () => {
  const { api, emitHealth, failAllP04 } = fixture('ready');
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  await waitFor(() => expect(result.current.permission).toBeDefined());
  act(() => emitHealth('recovery_required'));
  failAllP04();
  await act(async () => result.current.refresh());
  expect(result.current.loading).toBe(true);
  expect(result.current.error).toBeUndefined();
  // Recovery ends; the retry against the recovered runtime must clear the wait.
  api.listPendingApprovals.mockResolvedValue(ok([]));
  api.getHostPreparationStatus.mockResolvedValue(ok(undefined));
  api.listQuarantine.mockResolvedValue(ok([]));
  api.listPermissionGrants.mockResolvedValue(ok([]));
  api.getSessionPermissionState.mockImplementation(async ({ session_id }: { session_id: string }) =>
    ok(permission('read_only', 2, session_id)),
  );
  act(() => emitHealth('ready'));
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(result.current.error).toBeUndefined();
  expect(result.current.permission?.mode).toBe('read_only');
});

test('a new runtime generation renegotiates without a false unsupported error', async () => {
  const { api, emitHealth, failAllP04 } = fixture('ready');
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  await waitFor(() => expect(result.current.permission).toBeDefined());
  // The Core restarts: a fresh runtime generation is negotiating, so P04 answers
  // "unsupported" again until the new handshake settles.
  act(() => emitHealth('starting', 'run-health-b'));
  failAllP04();
  await act(async () => result.current.refresh());
  expect(result.current.loading).toBe(true);
  expect(result.current.error).toBeUndefined();
  api.listPendingApprovals.mockResolvedValue(ok([]));
  api.getHostPreparationStatus.mockResolvedValue(ok(undefined));
  api.listQuarantine.mockResolvedValue(ok([]));
  api.listPermissionGrants.mockResolvedValue(ok([]));
  api.getSessionPermissionState.mockImplementation(async ({ session_id }: { session_id: string }) =>
    ok(permission('workspace_read_write', 3, session_id)),
  );
  act(() => emitHealth('ready'));
  await waitFor(() => expect(result.current.loading).toBe(false));
  expect(result.current.error).toBeUndefined();
  expect(result.current.permission?.revision).toBe(3);
});

test('a genuinely unsupported P04 interface is still reported once Core is ready', async () => {
  const { failAllP04 } = fixture('ready');
  failAllP04();
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  // A failing read is tried twice more before it is said.
  await waitFor(() => expect(result.current.loading).toBe(false), { timeout: 3_000 });
  expect(result.current.error).toBe(
    'Core does not support the P04 Security Control Plane interface',
  );
});

test('a real request failure clears once a later refresh succeeds', async () => {
  const { api } = fixture('ready');
  api.getSessionPermissionState.mockResolvedValue(P04_UNSUPPORTED);
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  await waitFor(() => expect(result.current.loading).toBe(false), { timeout: 3_000 });
  expect(result.current.error).toBe(
    'Core does not support the P04 Security Control Plane interface',
  );
  // A later successful refresh replaces the stale failure state.
  api.getSessionPermissionState.mockImplementation(async ({ session_id }: { session_id: string }) =>
    ok(permission('read_only', 4, session_id)),
  );
  await act(async () => result.current.refresh());
  expect(result.current.error).toBeUndefined();
  expect(result.current.permission?.mode).toBe('read_only');
});

// Switching conversations sometimes showed "权限与沙箱操作未完成" and the
// composer's "权限尚未就绪".
const UNREAD: ResultEnvelope<never> = {
  ok: false,
  error: {
    category: 'internal',
    code: 'persistence.internal',
    correlation_id: 'cor_00000000000000000000000001',
    details: {},
    message: 'read connection unavailable after 5000 ms',
    retryable: false,
    terminal: 'failed',
  },
};

/** The first load and the one that follows Core's handshake have both answered. */
async function loaded(api: ReturnType<typeof fixture>['api'], current: () => { loading: boolean }) {
  await waitFor(() => expect(api.getSessionPermissionState).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(current().loading).toBe(false));
  return api.getSessionPermissionState.mock.calls.length;
}

test('a read that fails while Core is busy is tried again before anything is said', async () => {
  const { api } = fixture('ready');
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  const before = await loaded(api, () => result.current);
  api.getSessionPermissionState
    .mockResolvedValueOnce(UNREAD)
    .mockImplementation(async ({ session_id }: { session_id: string }) =>
      ok(permission('read_only', 2, session_id)),
    );
  await act(async () => result.current.refresh());
  expect(result.current.error).toBeUndefined();
  expect(result.current.readFailed).toBe(false);
  expect(result.current.permission?.mode).toBe('read_only');
  expect(api.getSessionPermissionState.mock.calls.length - before).toBe(2);
});

test('a read that keeps failing is said as a read, keeps what was known, and reads again later', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    const { api } = fixture('ready');
    const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
    const before = await loaded(api, () => result.current);
    api.getSessionPermissionState.mockResolvedValue(UNREAD);
    api.listPendingApprovals.mockResolvedValue(UNREAD);
    await act(async () => result.current.refresh());
    // Tried three times in all, then said once, as a read.
    expect(api.getSessionPermissionState.mock.calls.length - before).toBe(3);
    expect(result.current.readFailed).toBe(true);
    expect(result.current.error).toBe('read connection unavailable after 5000 ms');
    // What was known stays: the conversation's mode is still shown.
    expect(result.current.permission?.mode).toBe('workspace_read_write');
    // Core answers again: the read in the background clears the notice.
    api.getSessionPermissionState.mockImplementation(
      async ({ session_id }: { session_id: string }) => ok(permission('read_only', 2, session_id)),
    );
    api.listPendingApprovals.mockResolvedValue(ok([]));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    await waitFor(() => expect(result.current.error).toBeUndefined());
    expect(result.current.readFailed).toBe(false);
    expect(result.current.permission?.mode).toBe('read_only');
  } finally {
    vi.useRealTimers();
  }
});

test('a read that keeps failing is read again less and less often, at most once a minute', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    const { api } = fixture('ready');
    const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
    await loaded(api, () => result.current);
    api.listQuarantine.mockResolvedValue(UNREAD);
    await act(async () => result.current.refresh());
    expect(result.current.readFailed).toBe(true);
    const tries = () => api.listQuarantine.mock.calls.length;
    // Five seconds, then ten: each background read tries three times before it fails again.
    let before = tries();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000 + 1_100);
    });
    expect(tries() - before).toBe(3);
    before = tries();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(tries() - before).toBe(0);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000 + 1_100);
    });
    expect(tries() - before).toBe(3);
  } finally {
    vi.useRealTimers();
  }
});

test("an operation's failure after a failed read is said as the operation's", async () => {
  const { api } = fixture('ready');
  const { result } = renderHook(() => useSecurityControl('workspace', 'session-a'));
  await loaded(api, () => result.current);
  api.listQuarantine.mockResolvedValue(UNREAD);
  await act(async () => result.current.refresh());
  expect(result.current.readFailed).toBe(true);
  api.setSessionPermissionMode.mockResolvedValueOnce({
    ok: false,
    error: { ...UNREAD.error, code: 'permission.mode_changed', message: '模式已在别处改变' },
  } as never);
  await act(async () => result.current.setMode('read_only'));
  expect(result.current.error).toBe('模式已在别处改变');
  expect(result.current.readFailed).toBe(false);
});
