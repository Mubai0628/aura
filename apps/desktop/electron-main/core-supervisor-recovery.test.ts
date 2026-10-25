// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { spawn } from 'node:child_process';
import { CoreTransport } from './core-transport.js';
import {
  createFixture,
  pipeClosed,
  waitUntil,
  loggedFields,
} from './core-supervisor-tests/fixture.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

test('an exhausted reconnect restarts the owned Core and never replays a user request', async () => {
  const f = await createFixture({ freshChildren: true, startupTimeouts: { handshakeMs: 80 } });
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  const originalRuntime = f.supervisor.snapshot.runtime_instance_id;
  await f.supervisor.requestWithBudget('fixture.echo', { input: 'send-once' }, 100);
  const initialConnect = vi.mocked(CoreTransport.connect).getMockImplementation();
  if (initialConnect === undefined) throw new Error('initial transport fixture missing');
  vi.mocked(CoreTransport.connect).mockImplementation(async (...args) => {
    if (f.children.length === 1) throw new Error('temporary unavailable pipe');
    return initialConnect(...args);
  });
  f.callbacks().onClosed(pipeClosed);
  await waitUntil(() => f.child.kill.mock.calls.length > 0);
  f.child.exitCode = 1;
  f.child.emit('exit', 1, null);
  await waitUntil(() => vi.mocked(spawn).mock.calls.length === 2, 2_000);
  await waitUntil(() => f.supervisor.snapshot.payload.state === 'ready');
  expect(f.supervisor.snapshot.runtime_instance_id).not.toBe(originalRuntime);
  expect(f.children[1]?.pid).not.toBe(f.child.pid);
  expect(
    f.transport.request.mock.calls.filter(([method]) => method === 'fixture.echo'),
  ).toHaveLength(1);
  expect(loggedFields('core_exited')).toHaveLength(1);
  expect(f.onEscalated).not.toHaveBeenCalled();
});

test('exhausted reconnect exits consume the existing crash budget and then escalate', async () => {
  const f = await createFixture({ freshChildren: true, startupTimeouts: { handshakeMs: 80 } });
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  const initialConnect = vi.mocked(CoreTransport.connect).getMockImplementation();
  if (initialConnect === undefined) throw new Error('initial transport fixture missing');
  let failingGeneration = 0;
  vi.mocked(CoreTransport.connect).mockImplementation(async (...args) => {
    if (f.children.length === failingGeneration) throw new Error('temporary unavailable pipe');
    return initialConnect(...args);
  });
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    failingGeneration = f.children.length;
    const child = f.children.at(-1);
    if (child === undefined) throw new Error('owned child fixture missing');
    f.callbacks().onClosed(pipeClosed);
    await waitUntil(() => child.kill.mock.calls.length > 0);
    child.exitCode = 1;
    child.emit('exit', 1, null);
    if (attempt < 3) {
      await waitUntil(() => f.children.length === attempt + 1);
      await waitUntil(() => f.supervisor.snapshot.payload.state === 'ready');
    }
  }
  await waitUntil(() => f.onEscalated.mock.calls.length === 1);
  expect(f.supervisor.snapshot.payload.state).toBe('app_crash_escalated');
  expect(vi.mocked(spawn)).toHaveBeenCalledTimes(3);
  expect(loggedFields('core_exited').map(({ attempt }) => attempt)).toEqual([1, 2, 3]);
});
