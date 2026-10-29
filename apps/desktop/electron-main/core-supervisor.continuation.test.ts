// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { spawn } from 'node:child_process';
import { createFixture, waitUntil } from './core-supervisor-tests/fixture.js';
import type { SupervisorFixture } from './core-supervisor-tests/types.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

// A new Core continues the turns a restart cut off only when Main says why the Core that served
// before it ended (--continue-after); the first Core of a Main, after Aura was closed, is not told.

/** The `--continue-after` each spawned Core was given, in order. */
const continueArguments = () =>
  vi
    .mocked(spawn)
    .mock.calls.map(([, args]) =>
      (args as string[]).find((argument) => argument.startsWith('--continue-after=')),
    );

let sequence = 0;
/** Core says its start reached ready, as it does once it has served its first turns. */
const ready = (f: SupervisorFixture) =>
  f.callbacks().onNotification('runtime.startup_progress', {
    runtime_instance_id: f.supervisor.snapshot.runtime_instance_id,
    sequence: sequence++,
    phase: 'ready',
    message: 'ready',
    occurred_at: new Date().toISOString(),
  });

/** The newest Core exits, and the next one is started and connected. */
const exit = async (f: SupervisorFixture, spawns: number) => {
  const child = f.children.at(-1)!;
  child.exitCode = 1;
  child.emit('exit', 1, null);
  await waitUntil(() => vi.mocked(spawn).mock.calls.length === spawns, 5_000);
  await waitUntil(() => f.supervisor.snapshot.payload.state === 'ready', 5_000);
  // Each Core numbers its startup progress from 0.
  sequence = 0;
};

test('each new Core is told why the Core that served before it ended, until one is ready', async () => {
  sequence = 0;
  const f = await createFixture({
    freshChildren: true,
    responsiveness: { noticeMs: 200, deadlockMs: 60_000, checkIntervalMs: 50 },
    readProcessActivity: () => undefined,
  });
  await f.supervisor.start();
  ready(f);
  // It crashed; the next one failed to start before it was ready, so the third is told the same.
  await exit(f, 2);
  await exit(f, 3);
  ready(f);
  // The third served; the user restarted it while Main waited for its heartbeat.
  await waitUntil(() => f.supervisor.snapshot.payload.waiting !== undefined);
  expect(f.supervisor.restartNow()).toBe(true);
  await exit(f, 4);
  expect(continueArguments()).toEqual([
    undefined,
    '--continue-after=crash',
    '--continue-after=crash',
    '--continue-after=user_restart',
  ]);
});

test('a Core that never served is not followed by a continuation', async () => {
  sequence = 0;
  const f = await createFixture({ freshChildren: true });
  await f.supervisor.start();
  await exit(f, 2);
  expect(continueArguments()).toEqual([undefined, undefined]);
});
