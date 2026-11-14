import { expect, test } from 'vitest';

import { DEFAULT_TIMEOUTS } from './endpoint-draft';

test('a new connection waits five minutes for the next bytes of an answer that has begun', () => {
  expect(DEFAULT_TIMEOUTS.stream_idle_ms).toBe(300_000);
});
