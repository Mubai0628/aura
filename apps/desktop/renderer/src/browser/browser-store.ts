import { useSyncExternalStore } from 'react';

import { MODEL_PAGE_ID, type BrowserPageState } from '../../../shared/browser-api';
import { browserApi } from './bridge';
import { adoptModelPage, browserTabs } from './browser-tabs';

export { browserApi } from './bridge';

/** The browser pages Main holds, as their states arrive. */
const key = (sessionId: string, pageId: string) => `${sessionId}\u0000${pageId}`;

class BrowserStore {
  #pages = new Map<string, BrowserPageState>();
  #listeners = new Set<() => void>();
  #unsubscribe: (() => void) | undefined;

  subscribe = (listener: () => void): (() => void) => {
    this.#start();
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  get = (sessionId: string, pageId: string): BrowserPageState | undefined =>
    this.#pages.get(key(sessionId, pageId));

  /**
   * A state from Main or from a command's reply. The model's page joins the browser's row, shown
   * with the sidebar, the first time it opens in a conversation; the user's own pages Main still
   * holds when the window starts (after a reload) join it quietly.
   */
  put = (state: BrowserPageState, held = false): void => {
    const fresh = !this.#pages.has(key(state.sessionId, state.pageId));
    if (!state.closed && state.pageId === MODEL_PAGE_ID) {
      if (fresh) adoptModelPage(state.sessionId);
    } else if (!state.closed && held) browserTabs.adopt(state.sessionId, state.pageId);
    const next = new Map(this.#pages);
    if (state.closed && state.notice === undefined) next.delete(key(state.sessionId, state.pageId));
    else next.set(key(state.sessionId, state.pageId), state);
    this.#pages = next;
    for (const listener of this.#listeners) listener();
  };

  #start(): void {
    if (this.#unsubscribe !== undefined) return;
    const api = browserApi();
    if (api === undefined) return;
    this.#unsubscribe = api.onState((state) => this.put(state));
    void api.pages().then((pages) => {
      for (const page of pages) this.put(page, true);
    });
  }
}

export const browserStore = new BrowserStore();

export function useBrowserPage(sessionId: string, pageId: string): BrowserPageState | undefined {
  return useSyncExternalStore(browserStore.subscribe, () => browserStore.get(sessionId, pageId));
}
