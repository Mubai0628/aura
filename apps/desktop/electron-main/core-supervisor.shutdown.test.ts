// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type { RuntimeHandshakeRequest } from '@aura/contracts/generated';
import { CoreTransport } from './core-transport.js';
import {
  createFixture,
  fixture,
  pipeClosed,
  waitUntil,
  loggedFields,
  type Kind,
} from './core-supervisor-tests/fixture.js';
import { projectionCalls } from './core-supervisor-tests/support.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

test('stop during beginHandshake does not activate or sync projections', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 5_000 } });
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  const begin = f.transport.beginHandshake.getMockImplementation();
  if (begin === undefined) throw new Error('beginHandshake fixture is missing');
  f.transport.beginHandshake.mockImplementation(async (request: RuntimeHandshakeRequest) => {
    seen.resolve();
    await release.promise;
    return begin(request);
  });
  const started = f.supervisor.start();
  await seen.promise;
  f.supervisor.stop();
  release.resolve();
  await started;
  expect(f.transport.finishHandshake).not.toHaveBeenCalled();
  expect(projectionCalls(f.transport.request)).toBe(0);
  expect(f.supervisor.snapshot.payload.state).not.toBe('ready');
  expect(f.transport.close).toHaveBeenCalled();
});

test('stop during finishHandshake does not activate or sync projections', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 5_000 } });
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  f.transport.finishHandshake.mockImplementation(async () => {
    seen.resolve();
    await release.promise;
  });
  const started = f.supervisor.start();
  await seen.promise;
  f.supervisor.stop();
  release.resolve();
  await started;
  expect(projectionCalls(f.transport.request)).toBe(0);
  expect(f.supervisor.snapshot.payload.state).not.toBe('ready');
  expect(f.transport.close).toHaveBeenCalled();
});

test('stop during projection sync does not apply a late snapshot or continue subscribing', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 5_000 } });
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  const original = f.transport.request.getMockImplementation();
  if (original === undefined) throw new Error('projection request fixture is missing');
  let blocked = false;
  f.transport.request.mockImplementation(
    async (method: string, params: { projection_kind: Kind }) => {
      if (!blocked && method === 'projection.snapshot') {
        blocked = true;
        seen.resolve();
        await release.promise;
      }
      return original(method, params);
    },
  );
  const started = f.supervisor.start();
  await seen.promise;
  const callsAtStop = projectionCalls(f.transport.request);
  f.supervisor.stop();
  release.resolve();
  await started;
  expect(projectionCalls(f.transport.request)).toBe(callsAtStop);
  expect(f.supervisor.snapshot.payload.state).not.toBe('ready');
});

test('a late handshake after the child exits does not activate that attempt', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 5_000 } });
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  f.transport.finishHandshake.mockImplementation(async () => {
    seen.resolve();
    await release.promise;
  });
  const started = f.supervisor.start();
  await seen.promise;
  f.child.exitCode = 1;
  f.child.emit('exit', 1, null);
  release.resolve();
  await started;
  expect(projectionCalls(f.transport.request)).toBe(0);
  expect(f.supervisor.snapshot.payload.state).not.toBe('ready');
  f.supervisor.stop();
});

test('stop inside model-global sync cannot send the next subscription', async () => {
  const f = await createFixture();
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  const original = f.transport.request.getMockImplementation();
  if (original === undefined) throw new Error('projection request fixture is missing');
  f.transport.request.mockImplementation(async (method, params) => {
    if (method === 'projection.subscribe' && String(params.projection_kind) === 'model_settings') {
      seen.resolve();
      await release.promise;
    }
    return original(method, params);
  });
  const started = f.supervisor.start();
  await seen.promise;
  const atStop = f.transport.request.mock.calls.length;
  f.supervisor.stop();
  release.resolve();
  await started;
  expect(f.transport.request.mock.calls.slice(atStop)).toEqual([]);
});

test('stop during same-process reconnect cannot reactivate the closed channel', async () => {
  const f = await fixture();
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  f.transport.finishHandshake.mockImplementation(async () => {
    seen.resolve();
    await release.promise;
  });
  f.callbacks().onClosed(pipeClosed);
  await seen.promise;
  const atStop = f.transport.request.mock.calls.length;
  f.supervisor.stop();
  const closedAtStop = f.transport.close.mock.calls.length;
  release.resolve();
  await vi.waitFor(() => {
    expect(
      loggedFields('core_transport_reconnected').length +
        f.transport.close.mock.calls.length -
        closedAtStop,
    ).toBeGreaterThan(0);
  });
  expect(f.transport.request.mock.calls.slice(atStop)).toEqual([]);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
});

test('a reconnect that has not returned is closed when stop replaces its generation', async () => {
  const f = await fixture();
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  const connect = vi.mocked(CoreTransport.connect);
  const original = connect.getMockImplementation();
  if (original === undefined) throw new Error('connect fixture is missing');
  connect.mockImplementation(async (options, listeners) => {
    seen.resolve();
    await release.promise;
    return original(options, listeners);
  });
  f.callbacks().onClosed(pipeClosed);
  await seen.promise;
  const atStop = f.transport.request.mock.calls.length;
  f.supervisor.stop();
  release.resolve();
  await vi.waitFor(() => {
    expect(f.transport.close.mock.calls.length).toBeGreaterThan(0);
  });
  expect(f.transport.request.mock.calls.slice(atStop)).toEqual([]);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
});

test('a child exit replaces a reconnect that has not finished the handshake', async () => {
  const f = await fixture();
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  const original = f.transport.finishHandshake.getMockImplementation();
  let hung = false;
  f.transport.finishHandshake.mockImplementation(async (...args) => {
    if (!hung) {
      hung = true;
      seen.resolve();
      await release.promise;
      return;
    }
    if (original === undefined) throw new Error('finishHandshake fixture is missing');
    return original(...args);
  });
  f.callbacks().onClosed(pipeClosed);
  await seen.promise;
  const atExit = f.transport.request.mock.calls.length;
  f.child.exitCode = 1;
  f.child.emit('exit', 1, null);
  release.resolve();
  await vi.waitFor(() => {
    expect(f.transport.close.mock.calls.length).toBeGreaterThan(0);
  });
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
  expect(f.transport.request.mock.calls.slice(atExit)).toEqual([]);
  f.supervisor.stop();
});

test('stop during reconnect projection sync does not subscribe again or record recovery', async () => {
  const f = await fixture();
  const release = Promise.withResolvers<void>();
  const seen = Promise.withResolvers<void>();
  const original = f.transport.request.getMockImplementation();
  if (original === undefined) throw new Error('projection request fixture is missing');
  let arm = false;
  f.transport.request.mockImplementation(async (method, params) => {
    if (arm && String(method).startsWith('projection.')) {
      arm = false;
      seen.resolve();
      await release.promise;
    }
    return original(method, params);
  });
  arm = true;
  f.callbacks().onClosed(pipeClosed);
  await seen.promise;
  const atStop = f.transport.request.mock.calls.length;
  f.supervisor.stop();
  const closedAtStop = f.transport.close.mock.calls.length;
  release.resolve();
  await vi.waitFor(() => {
    expect(
      loggedFields('core_transport_reconnected').length +
        f.transport.close.mock.calls.length -
        closedAtStop,
    ).toBeGreaterThan(0);
  });
  expect(f.transport.request.mock.calls.slice(atStop)).toEqual([]);
  expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
});

/**
 * Recovery is bounded by a monotonic total budget, not only by a per-attempt timeout.
 *
 * A `while` loop around a bounded attempt is still unbounded: the window can say
 * "recovering" for as long as attempts keep starting. The budget is what ends it, and
 * ending it has to reach the existing controlled exit path — the Core this supervisor
 * owns is killed, no user request is re-sent, and the projection says the Core is
 * unavailable instead of leaving the user on "正在恢复".
 */
test('a reconnect whose connect never completes ends on its total budget', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');

  const connect = vi.mocked(CoreTransport.connect);
  const original = connect.getMockImplementation();
  if (original === undefined) throw new Error('connect fixture is missing');
  const attempted = Promise.withResolvers<void>();
  connect.mockImplementation(async () => {
    attempted.resolve();
    // A connect that never completes. Only a total budget can end the recovery that
    // is waiting on it.
    return new Promise<CoreTransport>(() => {});
  });

  try {
    f.callbacks().onClosed(pipeClosed);
    await attempted.promise;
    // Waiting for the outcome itself matters: a condition an empty call list already
    // satisfies would let these assertions run before recovery has ended.
    await waitUntil(
      () => f.supervisor.snapshot.payload.status_code === 'core.reconnect_budget_exhausted',
    );
    expect(f.child.kill).toHaveBeenCalled();
    expect(loggedFields('core_reconnect_budget_exhausted').length).toBeGreaterThan(0);
    expect(loggedFields('core_transport_reconnected')).toHaveLength(0);
    expect(f.supervisor.snapshot.payload.state).toBe('restarting');
  } finally {
    connect.mockImplementation(original);
  }
});
