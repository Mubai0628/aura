import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { usePolling } from './usePolling';

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe('usePolling', () => {
  it('waits for a slow read before the next one, then keeps its interval', async () => {
    let settle: () => void = () => undefined;
    const read = vi.fn(() => new Promise<void>((resolve) => (settle = resolve)));
    renderHook(() => usePolling(read, 2_000));
    await vi.advanceTimersByTimeAsync(0);
    expect(read).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(read).toHaveBeenCalledTimes(1);
    settle();
    await vi.advanceTimersByTimeAsync(1_999);
    expect(read).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(read).toHaveBeenCalledTimes(2);
  });

  it('goes on after a failed read, and stops when unmounted', async () => {
    const read = vi.fn(async () => {
      throw new Error('unavailable');
    });
    const { unmount } = renderHook(() => usePolling(read, 1_000));
    await vi.advanceTimersByTimeAsync(2_500);
    expect(read).toHaveBeenCalledTimes(3);
    unmount();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(read).toHaveBeenCalledTimes(3);
  });

  it('tells a read whose effect ended that its answer is no longer wanted', async () => {
    const answers: (() => boolean)[] = [];
    const first = vi.fn(async (live: () => boolean) => void answers.push(live));
    const second = vi.fn(async () => undefined);
    const { rerender } = renderHook(({ read }) => usePolling(read, 1_000), {
      initialProps: { read: first as (live: () => boolean) => Promise<unknown> },
    });
    await vi.advanceTimersByTimeAsync(0);
    rerender({ read: second });
    await vi.advanceTimersByTimeAsync(0);
    expect(answers[0]?.()).toBe(false);
    expect(second).toHaveBeenCalledTimes(1);
  });
});
