import { EventEmitter } from 'node:events';
import type { ChildProcess } from 'node:child_process';
import { afterEach, expect, test, vi } from 'vitest';
import { waitForChildExit } from './child-exit.js';

afterEach(() => vi.useRealTimers());
const child = (exitCode: number | null = null) =>
  Object.assign(new EventEmitter(), {
    exitCode,
    signalCode: null,
    killed: true,
  }) as unknown as ChildProcess;

test('sending kill does not prove exit; the actual exit event does', async () => {
  const process = child();
  let stopped = false;
  const wait = waitForChildExit(process).then(() => {
    stopped = true;
  });
  await Promise.resolve();
  expect(stopped).toBe(false);
  process.emit('exit', null, 'SIGTERM');
  await wait;
  expect(stopped).toBe(true);
  expect(process.listenerCount('exit')).toBe(0);
});

test('unconfirmed exit times out and releases listeners', async () => {
  vi.useFakeTimers();
  const process = child();
  const wait = expect(waitForChildExit(process, 100)).rejects.toThrow('not confirmed');
  await vi.advanceTimersByTimeAsync(100);
  await wait;
  expect(process.listenerCount('exit')).toBe(0);
  expect(process.listenerCount('error')).toBe(0);
});

test('observed earlier exit needs no new signal or listener', async () => {
  const process = child(0);
  await waitForChildExit(process);
  expect(process.listenerCount('exit')).toBe(0);
});
