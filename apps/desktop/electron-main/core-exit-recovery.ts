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

/** One bounded crash history for both unexpected exits and exhausted channel recovery. */
export class CoreExitRecovery {
  #crashTimes: number[] = [];

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
