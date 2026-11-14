import { expect, test } from 'vitest';

import { localTime } from './local-time.js';

// A time a person reads: the clock alone today, the month and day before it on another day.

test('a time today is its clock; another day adds the month and day', () => {
  const now = new Date(2026, 10, 10, 9, 0);
  expect(localTime(new Date(2026, 10, 10, 16, 5).toISOString(), now)).toBe('16:05');
  expect(localTime(new Date(2026, 10, 12, 8, 0).toISOString(), now)).toBe('11月12日 08:00');
});
