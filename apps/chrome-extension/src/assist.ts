/**
 * A tab handed to the user (`chrome.request_user`): the side panel shows why and how long is
 * left, with "完成" and "取消". It ends with the user's answer, the time running out, the tab
 * closing, or Core withdrawing the request.
 */
import type { AssistOutcome } from './protocol.js';

export interface Assisting {
  tab_id: number;
  reason: string;
  /** When the wait ends, in milliseconds since the epoch. */
  deadline: number;
}

interface Waiting extends Assisting {
  finish(outcome: AssistOutcome): void;
}

export interface AssistDeps {
  /** Brings the tab to the front for the user. */
  focus(tabId: number): Promise<void>;
  onChange(): void;
  setTimer(run: () => void, ms: number): unknown;
  clearTimer(timer: unknown): void;
  now(): number;
}

export class Assists {
  readonly #deps: AssistDeps;
  #waiting: Waiting | null = null;

  constructor(deps: AssistDeps) {
    this.#deps = deps;
  }

  get current(): Assisting | null {
    if (this.#waiting === null) return null;
    const { tab_id, reason, deadline } = this.#waiting;
    return { tab_id, reason, deadline };
  }

  async wait(
    tabId: number,
    reason: string,
    timeoutSeconds: number,
    signal: AbortSignal,
  ): Promise<AssistOutcome> {
    // An earlier request still showing ends as declined: only one is ever waited for.
    this.#waiting?.finish('declined');
    await this.#deps.focus(tabId).catch(() => undefined);
    return new Promise((resolve) => {
      let ended = false;
      const finish = (outcome: AssistOutcome) => {
        if (ended) return;
        ended = true;
        this.#deps.clearTimer(timer);
        signal.removeEventListener('abort', withdrawn);
        if (this.#waiting?.finish === finish) this.#waiting = null;
        this.#deps.onChange();
        resolve(outcome);
      };
      const withdrawn = () => finish('declined');
      const timer = this.#deps.setTimer(() => finish('timed_out'), timeoutSeconds * 1000);
      signal.addEventListener('abort', withdrawn);
      this.#waiting = {
        tab_id: tabId,
        reason,
        deadline: this.#deps.now() + timeoutSeconds * 1000,
        finish,
      };
      this.#deps.onChange();
      if (signal.aborted) withdrawn();
    });
  }

  answer(outcome: 'done' | 'declined'): void {
    this.#waiting?.finish(outcome);
  }

  tabClosed(tabId: number): void {
    if (this.#waiting?.tab_id === tabId) this.#waiting.finish('closed');
  }
}
