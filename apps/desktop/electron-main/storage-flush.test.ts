import { afterEach, expect, test, vi } from 'vitest';
import { keepStorageWritten } from './storage-flush.js';

afterEach(() => {
  vi.useRealTimers();
});

test('the window storage is written every two seconds until stopped', () => {
  vi.useFakeTimers();
  const session = { flushStorageData: vi.fn() };
  const stop = keepStorageWritten(session);
  vi.advanceTimersByTime(1_999);
  expect(session.flushStorageData).not.toHaveBeenCalled();
  vi.advanceTimersByTime(1);
  expect(session.flushStorageData).toHaveBeenCalledTimes(1);
  vi.advanceTimersByTime(4_000);
  expect(session.flushStorageData).toHaveBeenCalledTimes(3);
  stop();
  vi.advanceTimersByTime(10_000);
  expect(session.flushStorageData).toHaveBeenCalledTimes(3);
});
