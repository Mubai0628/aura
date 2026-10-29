// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { createFixture, loggedFields, waitUntil } from './core-supervisor-tests/fixture.js';
import type { SupervisorFixture } from './core-supervisor-tests/types.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

// Short waits keep the cases fast; production waits 10 s before the notice and 60 s before ending
// an idle Core (core-supervisor/config.ts).
const responsiveness = { noticeMs: 400, deadlockMs: 800, checkIntervalMs: 50 };
const idle = () => ({ cpuTime: 1_000n, ioOperations: 5n });

async function started(work: () => { cpuTime: bigint; ioOperations: bigint } | undefined) {
  const f = await createFixture({ responsiveness, readProcessActivity: work });
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  return f;
}

let sequence = 0;
const heartbeat = (f: SupervisorFixture) =>
  f.callbacks().onNotification('runtime.heartbeat', {
    runtime_instance_id: f.supervisor.snapshot.runtime_instance_id,
    sequence: (sequence += 1),
    sent_at: new Date().toISOString(),
  });

test('a silent Core is waited for and shown as such; its heartbeat ends the wait', async () => {
  const f = await started(idle);
  heartbeat(f);
  await waitUntil(() => f.supervisor.snapshot.payload.waiting !== undefined);
  const since = Date.parse(f.supervisor.snapshot.payload.waiting!.since);
  expect(Date.now() - since).toBeGreaterThanOrEqual(350);
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  expect(loggedFields('core_waiting')).toHaveLength(1);
  heartbeat(f);
  expect(f.supervisor.snapshot.payload.waiting).toBeUndefined();
  await waitUntil(() => loggedFields('core_responsive_again').length === 1);
  expect(f.child.kill).not.toHaveBeenCalled();
});

test('a Core silent and idle for the whole window is ended and restarted, and the window says why', async () => {
  const f = await started(idle);
  await waitUntil(() => f.child.kill.mock.calls.length > 0, 5_000);
  expect(f.supervisor.snapshot.payload.status_code).toBe('core.unresponsive_restart');
  expect(f.supervisor.snapshot.payload.waiting).toBeUndefined();
  const [ended] = loggedFields('core_unresponsive');
  expect(ended).toMatchObject({ cpuMs: 0, ioOperations: 0, recovery: 'restart' });
  f.child.exitCode = 1;
  f.child.emit('exit', 1, null);
  await waitUntil(() => loggedFields('core_exited').length > 0);
  expect(loggedFields('core_exited')[0]).toMatchObject({ cause: 'unresponsive', attempt: 1 });
});

test('a silent Core that still works is waited for; 立即重启 ends it at once, not as a crash', async () => {
  let cpuTime = 0n;
  const f = await started(() => ({ cpuTime: (cpuTime += 5_000_000n), ioOperations: 0n }));
  // The wait is shown and Core is left alone well past the idle window.
  await waitUntil(() => f.supervisor.snapshot.payload.waiting !== undefined);
  await new Promise((resolve) => setTimeout(resolve, 1_500));
  expect(f.child.kill).not.toHaveBeenCalled();
  expect(f.supervisor.restartNow()).toBe(true);
  expect(f.child.kill).toHaveBeenCalledTimes(1);
  expect(f.supervisor.snapshot.payload).toMatchObject({
    state: 'restarting',
    status_code: 'core.restart_requested',
  });
  await waitUntil(() => loggedFields('core_restart_requested').length === 1);
  f.child.exitCode = 1;
  f.child.emit('exit', 1, null);
  await waitUntil(() => loggedFields('core_exited').length > 0);
  expect(loggedFields('core_exited')[0]).toMatchObject({ cause: 'user_restart' });
  expect(f.onEscalated).not.toHaveBeenCalled();
});

test('立即重启 leaves a Core that answers alone', async () => {
  const f = await started(idle);
  heartbeat(f);
  expect(f.supervisor.restartNow()).toBe(false);
  expect(f.child.kill).not.toHaveBeenCalled();
});

test('without a reading of the process, a silent Core is never ended by Main', async () => {
  const f = await started(() => undefined);
  await waitUntil(() => f.supervisor.snapshot.payload.waiting !== undefined);
  await new Promise((resolve) => setTimeout(resolve, 1_500));
  expect(f.child.kill).not.toHaveBeenCalled();
  expect(loggedFields('core_unresponsive')).toEqual([]);
});
