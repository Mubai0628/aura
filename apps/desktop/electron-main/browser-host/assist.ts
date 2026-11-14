/**
 * A model's request for the user's help on its page, or on a page the user handed over
 * (`browser.request_user`). The request stands until the user answers Done or Cancel, closes the
 * page or the time runs out, or Core withdraws it because the turn stopped. Aura does not check
 * whether what the user did worked.
 */
import { MODEL_PAGE_ID } from '../../shared/browser-api.js';
import type { BrowserPages } from './pages.js';

export type AssistOutcome = 'done' | 'declined' | 'timed_out' | 'closed';

interface Waiting {
  readonly commandId: string;
  /** The page the request is shown on. */
  readonly pageId: string;
  finish(outcome: AssistOutcome | 'withdrawn'): void;
}

export class Assists {
  readonly #pages: BrowserPages;
  readonly #now: () => number;
  /** The request each conversation shows, at most one: its commands run one at a time. */
  readonly #waiting = new Map<string, Waiting>();

  constructor(pages: BrowserPages, now: () => number = Date.now) {
    this.#pages = pages;
    this.#now = now;
    pages.onState((state) => {
      const waiting = this.#waiting.get(state.sessionId);
      if (state.closed && waiting?.pageId === state.pageId) waiting.finish('closed');
    });
  }

  /** Shows the request on the page (the conversation's model page) and waits for how it ends. */
  wait(
    sessionId: string,
    commandId: string,
    reason: string,
    timeoutMs: number,
    pageId: string = MODEL_PAGE_ID,
  ): Promise<AssistOutcome | 'withdrawn'> {
    this.#waiting.get(sessionId)?.finish('withdrawn');
    return new Promise((resolve) => {
      const waiting: Waiting = {
        commandId,
        pageId,
        finish: (outcome) => {
          if (this.#waiting.get(sessionId) !== waiting) return;
          this.#waiting.delete(sessionId);
          clearTimeout(timer);
          this.#pages.setAssist(sessionId, pageId, undefined);
          resolve(outcome);
        },
      };
      const timer = setTimeout(() => waiting.finish('timed_out'), timeoutMs);
      this.#waiting.set(sessionId, waiting);
      this.#pages.setAssist(sessionId, pageId, {
        reason,
        deadline: this.#now() + timeoutMs,
      });
    });
  }

  /**
   * The user's answer from the request's buttons, on the page named if one is; false when
   * nothing waits for one there.
   */
  answer(sessionId: string, outcome: 'done' | 'declined', pageId?: string): boolean {
    const waiting = this.#waiting.get(sessionId);
    if (waiting === undefined || (pageId !== undefined && waiting.pageId !== pageId)) return false;
    waiting.finish(outcome);
    return true;
  }

  /** Core no longer waits for this command. */
  withdraw(commandId: string): void {
    for (const waiting of [...this.#waiting.values()])
      if (waiting.commandId === commandId) waiting.finish('withdrawn');
  }
}
