import type { CoreHealthProjection } from '@aura/contracts/generated';

const CRASH_WINDOW_MS = 10 * 60_000;
type RecoveryPlan =
  | { kind: 'stopped' }
  | {
      kind: 'restart' | 'escalate';
      attempt: number;
      delayMilliseconds: number;
      state: 'restarting' | 'app_crash_escalated';
      statusCode: string;
      statusMessage: string;
    };

/**
 * Why a Core that was serving ended while Aura stayed open: it exited by itself, Main ended it
 * after a silent and idle minute, its channel could not be recovered, or the user restarted it.
 */
export type CoreRestartCause = 'crash' | 'unresponsive' | 'transport_lost' | 'user_restart';

/**
 * One bounded crash history for both unexpected exits and exhausted channel recovery, and what
 * the next Core is told about the one before it.
 */
export class CoreExitRecovery {
  #crashTimes: number[] = [];
  /** Whether the Core now running reached ready: only one that served had turns to cut off. */
  #served = false;
  /** Why the last Core that served ended, until a Core started after it reaches ready. */
  #continueAfter: CoreRestartCause | undefined;

  /** The arguments a new Core starts with: why the Core that served before it ended. */
  spawning(): string[] {
    this.#served = false;
    return this.#continueAfter === undefined ? [] : [`--continue-after=${this.#continueAfter}`];
  }

  /** The new Core reached ready, and has continued what the restart cut off. */
  ready(): void {
    this.#served = true;
    this.#continueAfter = undefined;
  }

  /** How Main recovers from an exit that `cause` explains. */
  afterExit(cause: CoreRestartCause, state: CoreHealthProjection['state'], now: number) {
    if (this.#served) this.#continueAfter = cause;
    this.#served = false;
    return cause === 'user_restart' ? this.requested() : this.plan(state, now);
  }

  /** The user's restart: at once, and not counted as a crash. */
  requested(): RecoveryPlan {
    return {
      kind: 'restart',
      attempt: 1,
      delayMilliseconds: 0,
      state: 'restarting',
      statusCode: 'core.restart_requested',
      statusMessage: '正在重启 Aura Core…',
    };
  }

  plan(state: CoreHealthProjection['state'], now: number): RecoveryPlan {
    // Missing executables and incompatible releases require owner repair, not a spawn loop.
    if (state === 'incompatible' || state === 'unavailable') return { kind: 'stopped' };
    this.#crashTimes = this.#crashTimes.filter((timestamp) => now - timestamp <= CRASH_WINDOW_MS);
    this.#crashTimes.push(now);
    const attempt = this.#crashTimes.length;
    const escalate = attempt >= 3;
    return {
      kind: escalate ? 'escalate' : 'restart',
      attempt,
      delayMilliseconds: escalate ? 0 : attempt === 1 ? 500 : 2_000,
      state: escalate ? 'app_crash_escalated' : 'restarting',
      statusCode: escalate ? 'core.crash_escalated' : 'core.restarting',
      statusMessage: escalate
        ? 'Aura Core 重复崩溃，已交由 Launcher 恢复。'
        : `Core 已退出，准备第 ${String(attempt)} 次恢复…`,
    };
  }
}
