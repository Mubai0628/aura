// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { PREPARE_EXIT_TIMEOUT_MS, prepareExitThenStop } from './core-exit.js';

// A quitting Main asks Core to record a normal exit, then stops it whatever the answer.

function core(answer: () => Promise<unknown>) {
  const order: string[] = [];
  return {
    order,
    requestWithBudget: vi.fn(async (method: string) => {
      order.push(method);
      return answer();
    }),
    stop: vi.fn(() => order.push('stop')),
  };
}

test('Core is asked to record the exit before it is stopped', async () => {
  const running = core(async () => ({ marked_clean: true }));
  await prepareExitThenStop(running);
  expect(running.requestWithBudget).toHaveBeenCalledWith(
    'runtime.prepare_exit',
    {},
    PREPARE_EXIT_TIMEOUT_MS,
  );
  expect(running.order).toEqual(['runtime.prepare_exit', 'stop']);
});

test('a Core that cannot answer is stopped all the same', async () => {
  const silent = core(async () => {
    throw new Error('ERR_AURA_CORE_REQUEST_TIMEOUT');
  });
  await prepareExitThenStop(silent);
  expect(silent.stop).toHaveBeenCalledTimes(1);
});

test('without a Core there is nothing to stop', async () => {
  await expect(prepareExitThenStop(undefined)).resolves.toBeUndefined();
});
