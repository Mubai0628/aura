/**
 * Telling a slow Core from a stopped one.
 *
 * Core sends a heartbeat every two seconds. A Core whose store work is slow under load can miss
 * several, so silence alone ends nothing: after `noticeMs` the window says Main is waiting, and
 * Core is ended only when it has been silent for `deadlockMs` and its process did almost no work
 * in that whole window (CPU time and completed reads and writes, read from the process itself).
 * A process that still works is waited for; the user can restart it.
 *
 * Main's own stalls are not Core's silence: a check that runs late means Main's event loop was
 * stopped, and heartbeats that arrived meanwhile are read only after the timers that run first.
 */

/** What Main reads of the Core process. */
export interface CoreProcessActivity {
  /** Kernel and user time, in 100 ns units. */
  readonly cpuTime: bigint;
  /** Read and write operations the process has completed. */
  readonly ioOperations: bigint;
}

export interface ResponsivenessTimings {
  /** Silence after which the window says Main is waiting for Core. */
  readonly noticeMs: number;
  /** Silence, and as long a window without work, after which Core is ended and restarted. */
  readonly deadlockMs: number;
  /** How often Main looks. */
  readonly checkIntervalMs: number;
}

export type ResponsivenessEvent =
  | { readonly kind: 'main_stalled'; readonly stalledMs: number }
  | { readonly kind: 'waiting'; readonly sinceWallMs: number; readonly silenceMs: number }
  | {
      readonly kind: 'deadlocked';
      readonly silenceMs: number;
      readonly cpuMs: number;
      readonly ioOperations: number;
    };

/** A check later than this was Main's own stall, and is logged as one. */
const MAIN_STALL_MS = 1_000;
/** Activity is read once a heartbeat has been missed, and at the latest once the wait shows. */
const SAMPLE_AFTER_MS = 3_000;
/** Less CPU time than this in the whole window (100 ms) is no work. */
const IDLE_CPU_TIME = 1_000_000n;
/** Fewer completed reads and writes than this in the whole window is no work. */
const IDLE_IO_OPERATIONS = 10n;

interface Sample {
  readonly at: number;
  readonly activity: CoreProcessActivity;
}

export interface ResponsivenessClock {
  /** Milliseconds on a clock that cannot step. */
  monotonic(): number;
  /** Milliseconds since the epoch, for what the window shows. */
  wall(): number;
}

export const SYSTEM_CLOCK: ResponsivenessClock = {
  monotonic: () => performance.now(),
  wall: () => Date.now(),
};

export class CoreResponsiveness {
  readonly #timings: ResponsivenessTimings;
  readonly #clock: ResponsivenessClock;
  readonly #readActivity: () => CoreProcessActivity | undefined;
  #lastHeartbeat: number;
  #lastCheck: number;
  #waitingSince: number | undefined;
  #samples: Sample[] = [];
  #ended = false;

  constructor(
    timings: ResponsivenessTimings,
    readActivity: () => CoreProcessActivity | undefined,
    clock: ResponsivenessClock = SYSTEM_CLOCK,
  ) {
    this.#timings = timings;
    this.#readActivity = readActivity;
    this.#clock = clock;
    this.#lastHeartbeat = clock.monotonic();
    this.#lastCheck = this.#lastHeartbeat;
  }

  /** When the window started waiting (epoch milliseconds), while it is. */
  get waitingSince(): number | undefined {
    return this.#waitingSince;
  }

  /** How long Core has been silent, Main's own stalls excluded. */
  silenceMs(): number {
    return Math.max(0, Math.round(this.#clock.monotonic() - this.#lastHeartbeat));
  }

  /** A new channel, or a resumed system: the silence starts again. */
  reset(): void {
    const now = this.#clock.monotonic();
    this.#lastHeartbeat = now;
    this.#lastCheck = now;
    this.#waitingSince = undefined;
    this.#samples = [];
    this.#ended = false;
  }

  /** A heartbeat arrived; returns how long the window waited, when it did. */
  heartbeat(): { waitedMs: number } | undefined {
    const waitingSince = this.#waitingSince;
    const now = this.#clock.monotonic();
    const silence = now - this.#lastHeartbeat;
    this.#lastHeartbeat = now;
    this.#waitingSince = undefined;
    this.#samples = [];
    return waitingSince === undefined ? undefined : { waitedMs: Math.round(silence) };
  }

  /** One look; Main calls it every `checkIntervalMs`. */
  check(): ResponsivenessEvent[] {
    const now = this.#clock.monotonic();
    const events: ResponsivenessEvent[] = [];
    const late = now - this.#lastCheck - this.#timings.checkIntervalMs;
    this.#lastCheck = now;
    if (late > MAIN_STALL_MS) {
      events.push({ kind: 'main_stalled', stalledMs: Math.round(late) });
      this.#lastHeartbeat = Math.min(now, this.#lastHeartbeat + late);
    }
    if (this.#ended) return events;
    const silence = now - this.#lastHeartbeat;
    if (silence >= Math.min(SAMPLE_AFTER_MS, this.#timings.noticeMs)) this.#sample(now);
    if (silence >= this.#timings.noticeMs && this.#waitingSince === undefined) {
      this.#waitingSince = this.#clock.wall() - silence;
      events.push({
        kind: 'waiting',
        sinceWallMs: this.#waitingSince,
        silenceMs: Math.round(silence),
      });
    }
    if (silence >= this.#timings.deadlockMs) {
      const idle = this.#idleWindow(now);
      if (idle !== undefined) {
        this.#ended = true;
        events.push({ kind: 'deadlocked', silenceMs: Math.round(silence), ...idle });
      }
    }
    return events;
  }

  #sample(now: number): void {
    const activity = this.#readActivity();
    if (activity === undefined) {
      // Without a reading the window starts again: no reading is never "no work".
      this.#samples = [];
      return;
    }
    this.#samples.push({ at: now, activity });
    const windowStart = now - this.#timings.deadlockMs;
    while (this.#samples.length > 1 && this.#samples[1]!.at <= windowStart) this.#samples.shift();
  }

  #idleWindow(now: number): { cpuMs: number; ioOperations: number } | undefined {
    const first = this.#samples[0];
    const last = this.#samples.at(-1);
    if (first === undefined || last === undefined || last.at !== now) return undefined;
    if (now - first.at < this.#timings.deadlockMs) return undefined;
    const cpu = last.activity.cpuTime - first.activity.cpuTime;
    const io = last.activity.ioOperations - first.activity.ioOperations;
    if (cpu >= IDLE_CPU_TIME || io >= IDLE_IO_OPERATIONS) return undefined;
    return { cpuMs: Number(cpu / 10_000n), ioOperations: Number(io) };
  }
}
