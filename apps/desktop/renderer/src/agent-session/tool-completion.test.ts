import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { expect, test } from 'vitest';
import { latestToolCompletion } from './tool-completion';
test('completion refresh facts include any tool and late older parallel calls, including missing historical times', () => {
  const call = (done: boolean) =>
    ({
      summary: { outcome: done ? { outcome: 'succeeded' } : null, completed_at: null },
    }) as unknown as AgentToolActivityEntry;
  const calls = new Map([
    ['older', call(false)],
    ['newer', call(true)],
  ]);
  const before = latestToolCompletion(calls);
  calls.set('older', call(true));
  expect(latestToolCompletion(calls)).not.toBe(before);
  expect(latestToolCompletion(new Map([...calls].reverse()))).toBe(latestToolCompletion(calls));
  expect(latestToolCompletion(new Map([['pending', call(false)]]))).toBe('');
});
