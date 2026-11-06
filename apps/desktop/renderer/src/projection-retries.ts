import type { CoreHealthProjectionChange } from '../../shared/aura-api';

const FIRST_PAUSE_MS = 1_000;
const LONGEST_PAUSE_MS = 30_000;

interface Waiting {
  /** Subscribes the route again; its outcome comes back as `failed` or `settled`. */
  ask: () => void;
  /** How many subscriptions of the route failed in a row. */
  failures: number;
  /** The pause before the next one; none while one is under way. */
  timer: ReturnType<typeof setTimeout> | undefined;
}

/**
 * The scoped projection routes whose subscription was refused: Core had not started, was
 * restarting or did not answer. Main holds no route it refused, so nothing would ever ask for it
 * again. Each is asked again as soon as Core reports itself ready, and otherwise after a pause that
 * doubles from 1 s to at most 30 s, until it is subscribed or nobody shows it any more.
 */
export class SubscriptionRetries {
  readonly #waiting = new Map<string, Waiting>();
  #stopHealth: (() => void) | undefined;
  /** Core's runtime and state as last reported: only a change to ready asks again. */
  #health: string | undefined;

  /** The subscription of `key` failed; `ask` subscribes it again. */
  failed(key: string, ask: () => void): void {
    const failures = (this.#waiting.get(key)?.failures ?? 0) + 1;
    this.#cancel(key);
    const pause = Math.min(FIRST_PAUSE_MS * 2 ** (failures - 1), LONGEST_PAUSE_MS);
    const timer = setTimeout(() => this.#askAgain(key), pause);
    this.#waiting.set(key, { ask, failures, timer });
    this.#listen();
  }

  /** The subscription of `key` holds, or nobody shows the route any more. */
  settled(key: string): void {
    this.#cancel(key);
    this.#waiting.delete(key);
    if (this.#waiting.size !== 0) return;
    this.#stopHealth?.();
    this.#stopHealth = undefined;
    this.#health = undefined;
  }

  reset(): void {
    for (const key of [...this.#waiting.keys()]) this.settled(key);
  }

  #askAgain(key: string): void {
    const waiting = this.#waiting.get(key);
    // A subscription already under way answers for itself.
    if (waiting?.timer === undefined) return;
    clearTimeout(waiting.timer);
    waiting.timer = undefined;
    waiting.ask();
  }

  #cancel(key: string): void {
    const timer = this.#waiting.get(key)?.timer;
    if (timer !== undefined) clearTimeout(timer);
  }

  #listen(): void {
    if (this.#stopHealth !== undefined) return;
    const onHealth = window.aura?.v1?.onCoreHealth;
    if (typeof onHealth !== 'function') return;
    this.#stopHealth = onHealth((change: CoreHealthProjectionChange) => {
      const health = `${change.runtime_instance_id}:${change.payload.state}`;
      if (health === this.#health) return;
      this.#health = health;
      if (change.payload.state !== 'ready') return;
      for (const key of [...this.#waiting.keys()]) this.#askAgain(key);
    });
  }
}
