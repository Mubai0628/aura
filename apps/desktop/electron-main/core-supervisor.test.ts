// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type { ProjectionChange } from '@aura/contracts/generated';
import { HANDSHAKE_TIMEOUT_MS, STARTUP_CONNECT_TIMEOUT_MS } from './core-supervisor.js';
import { createFixture, fixture, loggedFields } from './core-supervisor-tests/fixture.js';
import { refresh, current, listen } from './core-supervisor-tests/support.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

test('a reported Tools configuration fault leaves startup and recovery reachable', async () => {
  for (const starting of [false, true]) {
    const f = await createFixture({ configurationRecovery: starting });
    f.transport.request.mockImplementation(async (method, params) => {
      if (method === 'config.recovery.notice')
        return {
          affected_domains: ['tools'],
          unavailable_capabilities: ['tool_policy'],
          fault_codes: ['config.domain.codec'],
          suspended_calls: ['new_tool_execution'],
          available_actions: ['reset_to_default'],
          scope_known: true,
          recovery_in_progress: false,
          next_step: 'review_recovery',
        };
      if (params.projection_kind === 'workspace_files')
        throw new Error('The affected service is genuinely absent');
      if (method === 'projection.subscribe') return { subscribed: true };
      const current = f.snapshot(params.projection_kind, 1);
      return current.projection_kind === 'core_health'
        ? {
            ...current,
            payload: {
              ...current.payload,
              state: starting ? 'starting' : 'ready',
              status_code: starting ? 'core.starting' : 'core.ready_degraded',
            },
          }
        : current;
    });
    await f.supervisor.start();
    expect(f.supervisor.snapshot.payload.state).toBe(starting ? 'starting' : 'ready');
    expect(f.transport.request).toHaveBeenCalledWith('config.recovery.notice', {});
    expect(
      f.transport.request.mock.calls.some(([, p]) => p.projection_kind === 'workspace_session'),
    ).toBe(true);
    expect(
      f.transport.request.mock.calls.some(([, p]) => p.projection_kind === 'workspace_files'),
    ).toBe(false);
    expect(f.child.kill).not.toHaveBeenCalled();
    f.supervisor.stop();
  }
});

test.each(['core_health', 'workspace_session', 'workspace_files'] as const)(
  '%s keeps a notification that overtakes an in-flight snapshot reply',
  async (kind) => {
    const f = await fixture();
    const reply = Promise.withResolvers<ProjectionChange>();
    f.setRead(() => reply.promise);
    const published = vi.fn();
    listen(f.supervisor, kind, published);
    const pending = refresh(f.supervisor, kind);
    f.push(kind, 2);
    const newer = current(f.supervisor, kind);
    reply.resolve(f.snapshot(kind, 1));
    await expect(pending).resolves.toEqual(newer);
    expect(current(f.supervisor, kind)).toEqual(newer);
    expect(published).toHaveBeenCalledTimes(1);
    expect(f.transport.close).not.toHaveBeenCalled();
  },
);

test.each(['core_health', 'workspace_session', 'workspace_files'] as const)(
  '%s preserves the newest of two concurrent snapshot reads',
  async (kind) => {
    const f = await fixture();
    const older = Promise.withResolvers<ProjectionChange>();
    const newer = Promise.withResolvers<ProjectionChange>();
    const replies = [older, newer];
    f.setRead(() => replies.shift()!.promise);
    const first = refresh(f.supervisor, kind);
    const second = refresh(f.supervisor, kind);
    newer.resolve(f.snapshot(kind, 3));
    const applied = await second;
    older.resolve(f.snapshot(kind, 2));
    await expect(first).resolves.toEqual(applied);
    expect(current(f.supervisor, kind)).toEqual(applied);
  },
);

test.each(['core_health', 'workspace_session', 'workspace_files'] as const)(
  '%s rejects snapshots older than the revision known before the request',
  async (kind) => {
    const f = await fixture();
    f.push(kind, 2);
    const before = current(f.supervisor, kind);
    f.setRead(async () => f.snapshot(kind, 1));
    await expect(refresh(f.supervisor, kind)).rejects.toThrow('revision moved backwards');
    expect(current(f.supervisor, kind)).toEqual(before);
  },
);

test.each(['core_health', 'workspace_session', 'workspace_files'] as const)(
  '%s ignores a snapshot from a different Core instance and preserves its last good data',
  async (kind) => {
    const f = await fixture();
    const before = current(f.supervisor, kind);
    f.setRead(async () => ({
      ...f.snapshot(kind, 2),
      runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    }));
    await expect(refresh(f.supervisor, kind)).resolves.toEqual(before);
    expect(current(f.supervisor, kind)).toEqual(before);
    expect(f.transport.close).not.toHaveBeenCalled();
  },
);

// The startup budget waits for a cold Core to open its real Store; the handshake
// budget only covers the proof-of-secret exchange on an already reachable Core.
// A cold check of a large Store must not be cut off and started again.
test('the startup budget stays separate from and longer than the handshake budget', () => {
  expect(STARTUP_CONNECT_TIMEOUT_MS).toBe(120_000);
  expect(HANDSHAKE_TIMEOUT_MS).toBe(10_000);
  expect(STARTUP_CONNECT_TIMEOUT_MS).toBeGreaterThan(HANDSHAKE_TIMEOUT_MS);
});

test('a Core that becomes reachable after the old ten second window still starts', async () => {
  const openedAt = Date.now();
  const f = await createFixture({
    connect: () => {
      if (Date.now() - openedAt < 11_000) throw new Error('ENOENT: pipe not published yet');
    },
  });
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  expect(f.connectAttempts()).toBeGreaterThan(1);
  expect(f.transport.beginHandshake).toHaveBeenCalledTimes(1);
  const timings = loggedFields('core_startup_timings');
  expect(timings).toHaveLength(1);
  expect(timings[0]?.connectMilliseconds).toBeGreaterThanOrEqual(10_000);
  expect(timings[0]?.handshakeMilliseconds).toBeLessThan(5_000);
  expect(timings[0]?.totalMilliseconds).toBeGreaterThanOrEqual(10_000);
  // A slow Store open is observable while it happens, not only on failure.
  expect(loggedFields('core_startup_progress').length).toBeGreaterThanOrEqual(2);
}, 30_000);

test('a Core that never becomes reachable fails on the startup budget alone', async () => {
  const f = await createFixture({
    connect: () => {
      throw new Error('ENOENT: no pipe');
    },
    startupTimeouts: { connectMs: 600, handshakeMs: 200 },
  });
  const startedAt = Date.now();
  await f.supervisor.start();
  const elapsed = Date.now() - startedAt;
  expect(elapsed).toBeGreaterThanOrEqual(600);
  expect(elapsed).toBeLessThan(5_000);
  const failed = loggedFields('core_startup_failed');
  expect(failed).toHaveLength(1);
  expect(failed[0]).toMatchObject({
    stage: 'transport_connect',
    failureCode: 'ERR_AURA_CORE_START_TIMEOUT',
    failureKind: 'request_timeout',
    retryable: true,
    statusCode: 'core.transport_connect_failed',
  });
  expect(failed[0]?.elapsedMilliseconds).toBeGreaterThanOrEqual(600);
  expect(failed[0]?.elapsedMilliseconds).toBeLessThan(10_000);
  // Reachability is not identity: the handshake phase never ran.
  expect(f.transport.beginHandshake).not.toHaveBeenCalled();
  expect(f.supervisor.snapshot.payload.state).toBe('restarting');
  f.supervisor.stop();
});

test('a Core that exits while starting reports the exit instead of the timeout', async () => {
  const f = await createFixture({
    connect: () => {
      throw new Error('ENOENT: no pipe');
    },
    startupTimeouts: { connectMs: 8_000 },
  });
  setTimeout(() => {
    f.child.exitCode = 4;
    f.child.stderr.emit('data', 'Aura Core failed: store C:\\Users\\private\\aura.db is locked\n');
    f.child.emit('exit', 4, null);
  }, 250);
  const startedAt = Date.now();
  await f.supervisor.start();
  const elapsed = Date.now() - startedAt;
  expect(elapsed).toBeLessThan(7_000);
  const failed = loggedFields('core_startup_failed');
  expect(failed[0]).toMatchObject({
    stage: 'transport_connect',
    failureCode: 'ERR_AURA_CORE_EXITED_EARLY',
    failureKind: 'transport_unavailable',
    coreExitCode: 4,
  });
  // The Core's own reason is kept, with the private path reduced to its base name.
  expect(String(failed[0]?.coreStderrTail)).toContain('store <path>\\aura.db is locked');
  expect(String(failed[0]?.coreStderrTail)).not.toContain('private');
  f.supervisor.stop();
});

test('a reachable Core that never proves its identity is rejected on the handshake budget', async () => {
  const f = await createFixture({
    handshake: 'hang',
    startupTimeouts: { connectMs: 5_000, handshakeMs: 300 },
  });
  const startedAt = Date.now();
  await f.supervisor.start();
  expect(Date.now() - startedAt).toBeLessThan(4_000);
  const failed = loggedFields('core_startup_failed');
  expect(failed).toHaveLength(1);
  expect(failed[0]).toMatchObject({
    stage: 'runtime_handshake',
    failureCode: 'ETIMEDOUT',
    failureKind: 'request_timeout',
    statusCode: 'core.handshake_failed',
  });
  expect(f.transport.finishHandshake).not.toHaveBeenCalled();
  expect(f.transport.close).toHaveBeenCalled();
  f.supervisor.stop();
});

test('a reachable Core from another Release Bundle stays incompatible', async () => {
  const f = await createFixture({ handshake: 'foreign-bundle' });
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('incompatible');
  expect(loggedFields('core_startup_failed')).toHaveLength(0);
  expect(f.transport.close).toHaveBeenCalled();
  expect(f.supervisor.snapshot.payload.status_message).toContain('Release Bundle');
  f.supervisor.stop();
});

// The native pipe connect cannot be cancelled once it started, so the budget has
// to bound the phase around it instead of trusting the attempt to return in time.
test('a native connect that lands after the budget is closed and never activated', async () => {
  const f = await createFixture({
    connect: (attempt) => {
      if (attempt > 1) throw new Error('ENOENT: no pipe');
      // The counterexample: 280ms of native waiting under a 100ms budget.
      return new Promise<void>((resolve) => setTimeout(resolve, 280));
    },
    startupTimeouts: { connectMs: 100, handshakeMs: 300 },
  });
  const startedAt = performance.now();
  await f.supervisor.start();
  const elapsed = performance.now() - startedAt;
  expect(elapsed).toBeLessThan(260);
  expect(loggedFields('core_startup_failed')[0]).toMatchObject({
    stage: 'transport_connect',
    failureCode: 'ERR_AURA_CORE_START_TIMEOUT',
    failureKind: 'request_timeout',
  });
  expect(loggedFields('core_startup_failed')[0]?.elapsedMilliseconds).toBeGreaterThanOrEqual(90);
  expect(loggedFields('core_startup_failed')[0]?.elapsedMilliseconds).toBeLessThan(10_000);
  expect(f.transport.beginHandshake).not.toHaveBeenCalled();
  expect(f.supervisor.snapshot.payload.state).not.toBe('ready');
  // Wait past the abandoned attempt: it must be drained and closed, not activated.
  await new Promise((done) => setTimeout(done, 260));
  expect(f.transport.close).toHaveBeenCalled();
  expect(f.transport.beginHandshake).not.toHaveBeenCalled();
  f.supervisor.stop();
});

test('a connect attempt that never returns cannot outlive the startup budget', async () => {
  const f = await createFixture({
    connect: () => new Promise<void>(() => {}),
    startupTimeouts: { connectMs: 150, handshakeMs: 200 },
  });
  const startedAt = performance.now();
  await f.supervisor.start();
  const elapsed = performance.now() - startedAt;
  // The phase is bounded by its budget, not by the attempt returning.
  expect(elapsed).toBeGreaterThanOrEqual(140);
  expect(elapsed).toBeLessThan(600);
  expect(loggedFields('core_startup_failed')[0]).toMatchObject({
    failureCode: 'ERR_AURA_CORE_START_TIMEOUT',
    failureKind: 'request_timeout',
  });
  expect(f.transport.close).not.toHaveBeenCalled();
  f.supervisor.stop();
});

test('a rejection that arrives after the budget cannot extend the wait', async () => {
  const f = await createFixture({
    connect: () =>
      new Promise<void>((_resolve, reject) => {
        setTimeout(() => reject(Object.assign(new Error('late ENOENT'), { code: 'ENOENT' })), 220);
      }),
    startupTimeouts: { connectMs: 100, handshakeMs: 200 },
  });
  const startedAt = performance.now();
  await f.supervisor.start();
  const elapsed = performance.now() - startedAt;
  expect(elapsed).toBeLessThan(200);
  expect(loggedFields('core_startup_failed')[0]).toMatchObject({
    failureCode: 'ERR_AURA_CORE_START_TIMEOUT',
    failureKind: 'request_timeout',
  });
  // The late ENOENT is not the reported reason, so it never became the decision.
  expect(loggedFields('core_startup_failed')[0]?.failureCode).not.toBe('ENOENT');
  f.supervisor.stop();
});

test('a connection that lands after stop is closed instead of becoming active', async () => {
  const pending = Promise.withResolvers<void>();
  const f = await createFixture({
    connect: () => pending.promise,
    startupTimeouts: { connectMs: 5_000 },
  });
  const started = f.supervisor.start();
  await new Promise((done) => setTimeout(done, 40));
  f.supervisor.stop();
  pending.resolve();
  await started;
  expect(f.transport.beginHandshake).not.toHaveBeenCalled();
  expect(f.transport.close).toHaveBeenCalled();
  expect(f.supervisor.snapshot.payload.state).not.toBe('ready');
});

test('a restart abandons the old startup phase and closes its late connection', async () => {
  const pending: Array<() => void> = [];
  const f = await createFixture({
    connect: () =>
      new Promise<void>((resolve) => {
        pending.push(() => resolve());
      }),
    startupTimeouts: { connectMs: 5_000 },
  });
  const started = f.supervisor.start();
  await new Promise((done) => setTimeout(done, 40));
  expect(pending.length).toBeGreaterThanOrEqual(1);
  // The Core process exits: the phase must end by itself, not on the next attempt.
  f.child.exitCode = 9;
  f.child.emit('exit', 9, null);
  await new Promise((done) => setTimeout(done, 60));
  pending[0]?.();
  await started;
  // The abandoned attempt resolves through its own microtask chain.
  await new Promise((done) => setTimeout(done, 60));
  expect(f.transport.beginHandshake).not.toHaveBeenCalled();
  expect(f.transport.close).toHaveBeenCalled();
  expect(loggedFields('core_startup_failed')[0]).toMatchObject({
    failureCode: 'ERR_AURA_CORE_EXITED_EARLY',
    coreExitCode: 9,
  });
  f.supervisor.stop();
});

test('startup progress is logged on its own timer while a connect waits', async () => {
  const f = await createFixture({
    connect: () => new Promise<void>(() => {}),
    startupTimeouts: { connectMs: 7_000, handshakeMs: 200 },
  });
  const started = f.supervisor.start();
  await new Promise((done) => setTimeout(done, 6_500));
  const progress = loggedFields('core_startup_progress');
  expect(progress.length).toBeGreaterThanOrEqual(2);
  expect(progress[0]?.phase).toBe('connect');
  expect(Number(progress[0]?.elapsedMilliseconds)).toBeLessThan(4_000);
  f.supervisor.stop();
  await started;
  // The timer is cleared with the phase: no further progress lines appear.
  const after = loggedFields('core_startup_progress').length;
  await new Promise((done) => setTimeout(done, 3_500));
  expect(loggedFields('core_startup_progress')).toHaveLength(after);
}, 20_000);
