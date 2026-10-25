// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type { RuntimeHandshakeRequest, RuntimeHandshakeResponse } from '@aura/contracts/generated';
import { CoreTransport } from './core-transport.js';
import {
  createFixture,
  fixture,
  pipeClosed,
  waitUntil,
  delay,
  loggedFields,
  type Kind,
} from './core-supervisor-tests/fixture.js';
import {
  requestTimeout,
  rejectionWith,
  budgetsOfCalls,
  projectionCalls,
} from './core-supervisor-tests/support.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

/** A connect that lands after the budget must be closed, never activated. */
test('a connect that succeeds after the recovery budget is closed and not activated', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();

  const connect = vi.mocked(CoreTransport.connect);
  const original = connect.getMockImplementation();
  if (original === undefined) throw new Error('connect fixture is missing');
  // A distinct channel object for the late connect: asserting that the *fixture's*
  // transport was not handshaken would be vacuous, because the startup already
  // handshook it.
  const lateTransport = {
    peerIdentity: f.transport.peerIdentity,
    close: vi.fn(),
    finishHandshake: vi.fn(),
    beginHandshake: vi.fn(async () => new Promise<never>(() => {})),
    request: vi.fn(async () => new Promise<never>(() => {})),
  };
  const attempted = Promise.withResolvers<void>();
  connect.mockImplementation(async () => {
    attempted.resolve();
    // Longer than the whole recovery budget, so the success lands after it ended.
    await delay(600);
    return lateTransport as unknown as CoreTransport;
  });

  try {
    f.callbacks().onClosed(pipeClosed);
    await attempted.promise;
    // Wait for the outcome, not for an empty list of a missing record: the late
    // connect lands 600ms in, after the 250ms budget, and its close has to happen
    // before this test asserts anything about it.
    await waitUntil(
      () => f.supervisor.snapshot.payload.status_code === 'core.reconnect_budget_exhausted',
    );
    await waitUntil(() => lateTransport.close.mock.calls.length > 0);
    // The late connect is a live channel object; it must be closed rather than handed
    // to a handshake the recovery no longer owns.
    expect(lateTransport.beginHandshake).not.toHaveBeenCalled();
    expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
  } finally {
    connect.mockImplementation(original);
  }
});

/**
 * Two handshake halves that each fit inside the budget still cannot exceed it.
 *
 * The counterexample: a 250 ms recovery budget with both halves 180 ms. Each half is
 * inside whatever it was handed, and the recovery still finished at about 401 ms —
 * because "each half gets the remaining budget" is not the same bound as "one absolute
 * deadline runs through both". The deadline is now re-measured for each half and checked
 * again before activation.
 */
test('two handshake halves inside their own windows cannot exceed the recovery budget', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();

  // Each half takes 180 ms — inside the 250 ms budget it is handed — but together they
  // are 360 ms, so a recovery that only asks "is this half inside its own window" ends
  // past the deadline it was given. The second half is also handed strictly less than the
  // first, which is what one absolute deadline across both looks like from the outside.
  const half = 180;
  const realBegin = f.transport.beginHandshake.getMockImplementation();
  if (realBegin === undefined) throw new Error('beginHandshake fixture is missing');
  f.transport.beginHandshake.mockImplementation(
    async (request: RuntimeHandshakeRequest): Promise<RuntimeHandshakeResponse> => {
      await delay(half);
      return realBegin(request);
    },
  );
  f.transport.finishHandshake.mockImplementation(async () => {
    await delay(half);
  });

  f.callbacks().onClosed(pipeClosed);
  await waitUntil(() => loggedFields('core_reconnect_budget_exhausted').length > 0, 5_000);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
  expect(f.child.kill).toHaveBeenCalled();
  expect(f.transport.close.mock.calls.length).toBeGreaterThan(0);
  // One absolute deadline across both halves looks like this from outside: the second
  // half is handed strictly less than the first, and less than the whole budget.
  const beginBudgets = budgetsOfCalls(f.transport.beginHandshake);
  const finishBudgets = budgetsOfCalls(f.transport.finishHandshake);
  expect(beginBudgets.length).toBeGreaterThan(0);
  expect(finishBudgets.length).toBeGreaterThan(0);
  const firstHalf = beginBudgets[beginBudgets.length - 1] as number;
  const secondHalf = finishBudgets[finishBudgets.length - 1] as number;
  expect(firstHalf).toBeGreaterThan(0);
  expect(secondHalf).toBeLessThan(firstHalf);
  expect(secondHalf).toBeLessThan(250);
  f.transport.beginHandshake.mockImplementation(realBegin);
});

/** Repeated connect failures are bounded by the same total budget. */ test('consecutive connect failures cannot keep recovery running past its budget', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 300 } });
  await f.supervisor.start();

  const connect = vi.mocked(CoreTransport.connect);
  const original = connect.getMockImplementation();
  if (original === undefined) throw new Error('connect fixture is missing');
  let attempts = 0;
  connect.mockImplementation(async () => {
    attempts += 1;
    throw new Error('fixture: pipe is not published yet');
  });

  try {
    f.callbacks().onClosed(pipeClosed);
    await vi.waitFor(
      () => {
        expect(loggedFields('core_reconnect_budget_exhausted').length).toBeGreaterThan(0);
      },
      { timeout: 5_000 },
    );
    const logged = loggedFields('core_reconnect_budget_exhausted')[0];
    expect(attempts).toBeGreaterThan(0);
    expect(Number(logged?.attempts)).toBe(attempts);
    expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
  } finally {
    connect.mockImplementation(original);
  }
});

/**
 * The independent counterexample, kept as a permanent regression: a 250 ms
 * recovery budget with the first projection subscription hanging after a
 * successful authentication. Recovery ends on its budget and reaches the
 * existing controlled exit — close the channel, cancel the owner, end the
 * Core — instead of waiting for the hung reply to be released.
 */
test('a projection subscription that has not replied ends recovery on its budget', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();
  const original = f.transport.request.getMockImplementation();
  if (original === undefined) throw new Error('missing request fixture');
  let unblock: (() => void) | undefined;
  const barrier = new Promise<void>((resolve) => {
    unblock = resolve;
  });
  let entered = false;
  f.transport.request.mockImplementation(async (method, params) => {
    if (method === 'projection.subscribe' && !entered) {
      entered = true;
      await barrier;
    }
    return original(method, params);
  });
  const began = performance.now();
  f.callbacks().onClosed(pipeClosed);
  await waitUntil(() => entered);
  await waitUntil(() => f.child.kill.mock.calls.length > 0);
  const elapsed = performance.now() - began;
  // Releasing the hung reply is cleanup: the recovery must already be over.
  unblock?.();
  expect(elapsed).toBeGreaterThanOrEqual(240);
  expect(elapsed).toBeLessThan(700);
  expect(loggedFields('core_reconnect_budget_exhausted').length).toBeGreaterThan(0);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
  expect(f.supervisor.snapshot.payload.status_code).toBe('core.reconnect_budget_exhausted');
  expect(f.transport.close.mock.calls.length).toBeGreaterThan(0);
});

test('a late projection reply does not update the generation that took over', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();
  const originalRequest = f.transport.request.getMockImplementation();
  if (originalRequest === undefined) throw new Error('missing request fixture');
  const begin = f.transport.beginHandshake.getMockImplementation();
  if (begin === undefined) throw new Error('missing beginHandshake fixture');
  const connect = vi.mocked(CoreTransport.connect);
  const connectOriginal = connect.getMockImplementation();
  if (connectOriginal === undefined) throw new Error('missing connect fixture');
  // The respawn after the controlled exit gets its own channel object, as a real
  // new connection would; asserting isolation against the same object would be vacuous.
  const secondTransport = {
    peerIdentity: f.transport.peerIdentity,
    close: vi.fn(),
    finishHandshake: vi.fn(),
    beginHandshake: vi.fn(async (request: RuntimeHandshakeRequest) => begin(request)),
    request: vi.fn(async (method: string, params: { projection_kind: Kind }) => {
      if (method === 'projection.subscribe') return { subscribed: true };
      if (method !== 'projection.snapshot') throw new Error(`Unexpected RPC ${method}`);
      return f.snapshot(params.projection_kind);
    }),
  };
  let connects = 0;
  connect.mockImplementation(async (options, listeners) => {
    connects += 1;
    if (connects <= 2) return connectOriginal(options, listeners);
    return secondTransport as unknown as CoreTransport;
  });
  const oldRuntime = f.supervisor.snapshot.runtime_instance_id;
  const oldCallbacks = f.callbacks();
  let unblock: (() => void) | undefined;
  const barrier = new Promise<void>((resolve) => {
    unblock = resolve;
  });
  let hung = false;
  f.transport.request.mockImplementation(async (method, params) => {
    if (!hung && method === 'projection.snapshot' && params.projection_kind === 'core_health') {
      hung = true;
      await barrier;
      // A late reply from the ended recovery carries the old runtime identity.
      return { ...f.snapshot(params.projection_kind, 99), runtime_instance_id: oldRuntime };
    }
    return originalRequest(method, params);
  });
  f.callbacks().onClosed(pipeClosed);
  await waitUntil(() => hung);
  await waitUntil(() => f.child.kill.mock.calls.length > 0);
  f.child.exitCode = 1;
  f.child.emit('exit', 1, null);
  f.supervisor.stop();
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  expect(f.supervisor.snapshot.payload.status_message).toBe('Ready 1');
  const revisionBeforeLateReplies = f.supervisor.snapshot.revision;
  // Both kinds of late arrivals from the old channel: the raced-off snapshot reply
  // and a projection change delivered through the old transport's callback. Neither
  // may touch the generation that took over.
  unblock?.();
  oldCallbacks.onNotification('projection.change', f.snapshot('core_health', 2));
  await delay(100);
  expect(f.supervisor.snapshot.payload.status_message).toBe('Ready 1');
  expect(f.supervisor.snapshot.revision).toBe(revisionBeforeLateReplies);
  expect(loggedFields('core_reconnect_budget_exhausted').length).toBeGreaterThan(0);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
});

test('projection sync stages inside their own windows cannot exceed the recovery budget', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();
  const original = f.transport.request.getMockImplementation();
  if (original === undefined) throw new Error('missing request fixture');
  // Each stage answers within the budget it is handed, but together they are 360 ms:
  // only one absolute deadline re-measured per stage can end the recovery in time.
  const half = 180;
  let delayed = 0;
  f.transport.request.mockImplementation(async (method, params) => {
    if (String(method).startsWith('projection.') && delayed < 2) {
      delayed += 1;
      await delay(half);
    }
    return original(method, params);
  });
  const began = performance.now();
  f.callbacks().onClosed(pipeClosed);
  await waitUntil(() => delayed >= 2);
  await waitUntil(() => f.child.kill.mock.calls.length > 0);
  const elapsed = performance.now() - began;
  expect(elapsed).toBeGreaterThanOrEqual(240);
  expect(elapsed).toBeLessThan(450);
  expect(loggedFields('core_reconnect_budget_exhausted').length).toBeGreaterThan(0);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
  expect(f.transport.close.mock.calls.length).toBeGreaterThan(0);
});

test('a cancelled recovery owner does not continue subscribing after its late reply', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();
  const original = f.transport.request.getMockImplementation();
  if (original === undefined) throw new Error('missing request fixture');
  let unblock: (() => void) | undefined;
  const barrier = new Promise<void>((resolve) => {
    unblock = resolve;
  });
  let hung = false;
  f.transport.request.mockImplementation(async (method, params) => {
    if (
      !hung &&
      method === 'projection.subscribe' &&
      params.projection_kind === 'workspace_session'
    ) {
      hung = true;
      await barrier;
    }
    return original(method, params);
  });
  f.callbacks().onClosed(pipeClosed);
  await waitUntil(() => hung);
  const callsAtHang = projectionCalls(f.transport.request);
  await waitUntil(() => f.child.kill.mock.calls.length > 0);
  expect(loggedFields('core_reconnect_budget_exhausted').length).toBeGreaterThan(0);
  // The hung subscription's late reply lands after the owner was cancelled: the
  // sync must stay dead instead of sending the remaining subscription kinds.
  unblock?.();
  await delay(100);
  expect(projectionCalls(f.transport.request)).toBe(callsAtHang);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
});

test('budget exhaustion preserves completed stages while the fresh Core restart is pending', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();
  const original = f.transport.request.getMockImplementation();
  if (original === undefined) throw new Error('missing request fixture');
  let unblock: (() => void) | undefined;
  const barrier = new Promise<void>((resolve) => {
    unblock = resolve;
  });
  let hung = false;
  f.transport.request.mockImplementation(async (method, params) => {
    if (
      !hung &&
      method === 'projection.subscribe' &&
      params.projection_kind === 'workspace_session'
    ) {
      hung = true;
      await barrier;
    }
    return original(method, params);
  });
  const subscribesFor = (kind: Kind): number =>
    f.transport.request.mock.calls.filter(
      (call) =>
        call[0] === 'projection.subscribe' &&
        (call[1] as { projection_kind?: unknown }).projection_kind === kind,
    ).length;
  f.callbacks().onClosed(pipeClosed);
  await waitUntil(() => hung);
  await waitUntil(() => f.child.kill.mock.calls.length > 0);
  // Core health completed before the hang; those requests must not be re-sent.
  expect(subscribesFor('core_health')).toBe(2);
  const callsAtTimeout = projectionCalls(f.transport.request);
  const connectsAtTimeout = f.connectAttempts();
  const killsAtTimeout = f.child.kill.mock.calls.length;
  f.child.exitCode = 1;
  f.child.emit('exit', 1, null);
  unblock?.();
  await delay(150);
  expect(f.connectAttempts()).toBe(connectsAtTimeout);
  expect(projectionCalls(f.transport.request)).toBe(callsAtTimeout);
  expect(subscribesFor('core_health')).toBe(2);
  expect(f.child.kill.mock.calls.length).toBe(killsAtTimeout);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
  expect(f.supervisor.snapshot.payload.status_code).toBe('core.restarting');
});

/**
 * One request timing out is ONE request.
 *
 * The counterexample this replaces: a request-local timeout used to retire the channel, so every
 * other request failed with it, the window said the Core was recovering, and the process was
 * eventually ended. The timeout now settles its own waiter; what a caller must still receive is the
 * precise reason and the way to obtain the authoritative result.
 */
test('a request-local timeout settles one call and no connection event', async () => {
  const f = await fixture();
  const before = f.supervisor.snapshot;
  const closes = f.transport.close.mock.calls.length;
  const connects = f.connectAttempts();
  const failure = rejectionWith(requestTimeout);
  f.transport.request.mockImplementationOnce(async () => {
    throw failure;
  });

  // The caller's own error, unwrapped: the reason (`ipc.request_timeout`) and the queryable hint
  // are what Main reads to answer the Renderer, so neither may be replaced on the way out.
  await expect(f.supervisor.requestP05('models.settings.get_snapshot', {})).rejects.toBe(failure);
  expect(failure.envelope).toEqual(requestTimeout);
  expect(failure.envelope.details).toMatchObject({
    outcome_recovery: 'query_runtime.command_outcome_without_replay',
  });

  await delay(50);
  // No connection event followed: no close, no reconnect, no exit, no "recovering" projection.
  expect(f.transport.close.mock.calls.length).toBe(closes);
  expect(f.connectAttempts()).toBe(connects);
  expect(f.child.kill).not.toHaveBeenCalled();
  expect(loggedFields('core_transport_closed')).toHaveLength(0);
  expect(f.supervisor.snapshot).toEqual(before);
  expect(f.supervisor.snapshot.payload.state).toBe('ready');

  // And the same channel still serves the next request, which is the point of a local settle.
  f.setRead(async () => f.snapshot('core_health', 2));
  await expect(f.supervisor.refreshSnapshot()).resolves.toMatchObject({
    payload: { status_message: 'Ready 2' },
  });
  f.supervisor.stop();
});
