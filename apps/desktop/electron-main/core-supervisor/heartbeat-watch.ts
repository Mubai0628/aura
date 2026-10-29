import { readCoreProcessActivity } from '../main-pipe-bridge.js';
import type { CoreSupervisorOptions } from './config.js';
import {
  CoreResponsiveness,
  type CoreProcessActivity,
  type ResponsivenessClock,
  type ResponsivenessTimings,
} from './responsiveness.js';

/** What the watch needs from the supervisor that owns the Core process. */
export interface HeartbeatWatchHost {
  /** Core is stopping, starting again, suspended or gone: this look is skipped and the silence starts again. */
  paused(): boolean;
  log(event: string, fields: Record<string, unknown>): Promise<void>;
  /** The window starts (`sinceWallMs`) or stops (`undefined`) saying Main is waiting for Core. */
  waiting(sinceWallMs: number | undefined): void;
  /** Core has been silent, and its process idle, for the whole window: end it. */
  deadlocked(): void;
}

/** The timer that looks at Core's heartbeats, and what each look leads to (see responsiveness.ts). */
export class HeartbeatWatch {
  readonly #watch: CoreResponsiveness;
  readonly #host: HeartbeatWatchHost;
  readonly #intervalMs: number;
  #timer: NodeJS.Timeout | undefined;

  constructor(
    host: HeartbeatWatchHost,
    timings: ResponsivenessTimings,
    readActivity: () => CoreProcessActivity | undefined,
    clock?: ResponsivenessClock,
  ) {
    this.#host = host;
    this.#intervalMs = timings.checkIntervalMs;
    this.#watch = new CoreResponsiveness(timings, readActivity, clock);
  }

  /** Since when the window has been waiting (epoch milliseconds), while it is. */
  get waitingSince(): number | undefined {
    return this.#watch.waitingSince;
  }

  silenceMs(): number {
    return this.#watch.silenceMs();
  }

  /** Looks once a check interval from now on; the silence starts again. */
  start(): void {
    this.stop();
    this.#timer = setInterval(() => this.#look(), this.#intervalMs);
  }

  /** No more looks; a wait the window showed is over (the caller publishes what replaces it). */
  stop(): void {
    if (this.#timer !== undefined) clearInterval(this.#timer);
    this.#timer = undefined;
    this.#watch.reset();
  }

  /** The system resumed: the silence starts again. */
  reset(): void {
    const waited = this.#watch.waitingSince !== undefined;
    this.#watch.reset();
    if (waited) this.#host.waiting(undefined);
  }

  heartbeat(): void {
    const resumed = this.#watch.heartbeat();
    if (resumed === undefined) return;
    void this.#host.log('core_responsive_again', { waitedMs: resumed.waitedMs });
    this.#host.waiting(undefined);
  }

  #look(): void {
    if (this.#host.paused()) {
      this.reset();
      return;
    }
    for (const event of this.#watch.check()) {
      if (event.kind === 'main_stalled') {
        void this.#host.log('main_event_loop_stall', { stalledMs: event.stalledMs });
      } else if (event.kind === 'waiting') {
        void this.#host.log('core_waiting', { silenceMs: event.silenceMs });
        this.#host.waiting(event.sinceWallMs);
      } else {
        void this.#host.log('core_unresponsive', {
          silenceMs: event.silenceMs,
          cpuMs: event.cpuMs,
          ioOperations: event.ioOperations,
          recovery: 'restart',
        });
        this.#host.deadlocked();
      }
    }
  }
}

/** The native reading of the Core process by its verified identity, unless a test supplies one. */
export const processActivityReader = (
  options: Pick<CoreSupervisorOptions, 'readProcessActivity' | 'release'>,
): ((pid: number, creationTime: string) => CoreProcessActivity | undefined) =>
  options.readProcessActivity ??
  ((pid, creationTime) =>
    readCoreProcessActivity(options.release.mainPipeBridgePath, pid, creationTime));
