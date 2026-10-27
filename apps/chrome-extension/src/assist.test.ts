import { describe, expect, it } from 'vitest';
import { Assists } from './assist.js';

function harness() {
  const timers: { run: () => void; ms: number; cleared: boolean }[] = [];
  const focused: number[] = [];
  let changes = 0;
  const assists = new Assists({
    focus: async (tabId) => void focused.push(tabId),
    onChange: () => {
      changes += 1;
    },
    setTimer: (run, ms) => {
      const timer = { run, ms, cleared: false };
      timers.push(timer);
      return timer;
    },
    clearTimer: (timer) => {
      if (timer) (timer as { cleared: boolean }).cleared = true;
    },
    now: () => 1_000,
  });
  return { assists, timers, focused, changes: () => changes };
}

describe('a tab handed to the user', () => {
  it('brings the tab forward, shows why and until when, and ends with the user’s answer', async () => {
    const h = harness();
    const waiting = h.assists.wait(7, '请登录', 300, new AbortController().signal);
    await Promise.resolve();
    await Promise.resolve();
    expect(h.focused).toEqual([7]);
    expect(h.assists.current).toEqual({ tab_id: 7, reason: '请登录', deadline: 301_000 });
    h.assists.answer('done');
    expect(await waiting).toBe('done');
    expect(h.assists.current).toBeNull();
    expect(h.timers[0]?.cleared).toBe(true);
  });

  it('ends when the time runs out, the tab closes or Core withdraws it', async () => {
    const timedOut = harness();
    const late = timedOut.assists.wait(7, 'x', 10, new AbortController().signal);
    await Promise.resolve();
    await Promise.resolve();
    expect(timedOut.timers[0]?.ms).toBe(10_000);
    timedOut.timers[0]?.run();
    expect(await late).toBe('timed_out');

    const closed = harness();
    const gone = closed.assists.wait(7, 'x', 10, new AbortController().signal);
    await Promise.resolve();
    await Promise.resolve();
    closed.assists.tabClosed(8);
    closed.assists.tabClosed(7);
    expect(await gone).toBe('closed');

    const withdrawn = harness();
    const controller = new AbortController();
    const cancelled = withdrawn.assists.wait(7, 'x', 10, controller.signal);
    await Promise.resolve();
    await Promise.resolve();
    controller.abort();
    expect(await cancelled).toBe('declined');
    expect(withdrawn.assists.current).toBeNull();
  });
});
