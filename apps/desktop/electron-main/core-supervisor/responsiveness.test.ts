import { describe, expect, test } from 'vitest';
import {
  CoreResponsiveness,
  type CoreProcessActivity,
  type ResponsivenessClock,
} from './responsiveness.js';

const TIMINGS = { noticeMs: 10_000, deadlockMs: 60_000, checkIntervalMs: 1_000 };

/** A clock the test moves, and a process whose work the test decides. */
function harness(work: (second: number) => CoreProcessActivity | undefined = () => idle) {
  let now = 0;
  const clock: ResponsivenessClock = { monotonic: () => now, wall: () => 1_000_000 + now };
  const watch = new CoreResponsiveness(TIMINGS, () => work(Math.round(now / 1_000)), clock);
  /** Advances one check interval at a time and returns every event in order. */
  const run = (seconds: number, lateBy = 0) => {
    const events = [];
    for (let second = 0; second < seconds; second += 1) {
      now += TIMINGS.checkIntervalMs + (second === 0 ? lateBy : 0);
      events.push(...watch.check());
    }
    return events;
  };
  return { watch, run, at: () => now };
}

const idle: CoreProcessActivity = { cpuTime: 5_000_000n, ioOperations: 400n };

describe('Core responsiveness', () => {
  test('ten seconds of silence starts the wait; a heartbeat ends it and says how long it was', () => {
    const { watch, run } = harness();
    expect(run(9).filter((event) => event.kind === 'waiting')).toEqual([]);
    expect(watch.waitingSince).toBeUndefined();
    const events = run(1);
    expect(events).toEqual([{ kind: 'waiting', sinceWallMs: 1_000_000, silenceMs: 10_000 }]);
    expect(watch.waitingSince).toBe(1_000_000);
    run(5);
    expect(watch.heartbeat()).toEqual({ waitedMs: 15_000 });
    expect(watch.waitingSince).toBeUndefined();
    expect(watch.heartbeat()).toBeUndefined();
  });

  test('a silent process that does no work for the whole minute is ended once', () => {
    const { run } = harness();
    const events = run(70);
    const ended = events.filter((event) => event.kind === 'deadlocked');
    expect(ended).toEqual([{ kind: 'deadlocked', silenceMs: 63_000, cpuMs: 0, ioOperations: 0 }]);
  });

  test('a silent process that is still working is waited for, however long', () => {
    // 2 ms of CPU a second: 120 ms in a minute, more than the idle allowance.
    const working = harness((second) => ({
      cpuTime: BigInt(second) * 20_000n,
      ioOperations: 0n,
    }));
    expect(working.run(300).filter((event) => event.kind === 'deadlocked')).toEqual([]);
    // Disk reads that still complete are work too, with no CPU time to speak of.
    const reading = harness((second) => ({ cpuTime: 0n, ioOperations: BigInt(second) }));
    expect(reading.run(300).filter((event) => event.kind === 'deadlocked')).toEqual([]);
  });

  test('work that stops is followed by a full idle minute before Core is ended', () => {
    // Works until second 40, then nothing.
    const { run } = harness((second) => ({
      cpuTime: BigInt(Math.min(second, 40)) * 1_000_000n,
      ioOperations: 0n,
    }));
    const ended = run(120).filter((event) => event.kind === 'deadlocked');
    expect(ended).toHaveLength(1);
    expect(ended[0]!.silenceMs).toBe(100_000);
  });

  test('without a reading of the process, Core is never ended for silence', () => {
    const { run } = harness(() => undefined);
    const events = run(300);
    expect(events.filter((event) => event.kind === 'deadlocked')).toEqual([]);
    expect(events.filter((event) => event.kind === 'waiting')).toHaveLength(1);
  });

  test("Main's own stall is logged and not counted as Core's silence", () => {
    const { watch, run } = harness();
    run(4);
    // The event loop stood still for 8 s; the heartbeats Core sent meanwhile are read after this.
    const events = run(1, 8_000);
    expect(events).toEqual([{ kind: 'main_stalled', stalledMs: 8_000 }]);
    expect(watch.silenceMs()).toBe(5_000);
    expect(watch.waitingSince).toBeUndefined();
  });

  test('a reset starts the silence again', () => {
    const { watch, run } = harness();
    run(12);
    expect(watch.waitingSince).toBeDefined();
    watch.reset();
    expect(watch.waitingSince).toBeUndefined();
    expect(watch.silenceMs()).toBe(0);
    expect(run(9).filter((event) => event.kind === 'waiting')).toEqual([]);
  });
});
