import { useSyncExternalStore } from 'react';

import { MODEL_PAGE_ID } from '../../../shared/browser-api';
import { toolTabs } from '../right-workspace/tool-tabs';
import { browserApi } from './bridge';

/**
 * A conversation has one
 * 浏览器 tab in the right sidebar, and its pages are tabs inside it. The page the model drives is
 * one of them, first in the row. Each conversation keeps its pages while Aura runs.
 */
export interface BrowserTabs {
  pages: readonly string[];
  /** The page on screen; none shows the start page. */
  shown?: string | undefined;
}

const EMPTY: BrowserTabs = { pages: [] };

const pageKey = (session: string, page: string) => `${session}\u0000${page}`;

class BrowserTabStore {
  readonly #sessions = new Map<string, BrowserTabs>();
  readonly #listeners = new Set<() => void>();
  /** The user's pages closed here: a state of one still on its way does not bring it back. */
  readonly #closed = new Set<string>();
  #ids = 0;

  get = (session: string): BrowserTabs => this.#sessions.get(session) ?? EMPTY;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** A page id Main has not seen: it keeps pages across a reload of the window. */
  newId(): string {
    return `page-${Date.now().toString(36)}-${++this.#ids}`;
  }

  /** A new page, shown. */
  open(session: string): string {
    const page = this.newId();
    this.#put(session, { pages: [...this.get(session).pages, page], shown: page });
    return page;
  }

  /** A page Main opened for the user joins the row last, and is shown. */
  add(session: string, page: string): void {
    const current = this.get(session);
    const pages = current.pages.includes(page) ? current.pages : [...current.pages, page];
    this.#put(session, { pages, shown: page });
  }

  /** A page opened for the user that will not join the row: Main closes it. */
  discard(session: string, page: string): void {
    this.#close(session, page);
  }

  /** A page Main holds joins the row once: the model's first, any other last. */
  adopt(session: string, page: string): boolean {
    const current = this.get(session);
    if (current.pages.includes(page) || this.#closed.has(pageKey(session, page))) return false;
    const pages = page === MODEL_PAGE_ID ? [page, ...current.pages] : [...current.pages, page];
    this.#put(session, { pages, shown: current.shown ?? page });
    return true;
  }

  show(session: string, page: string): void {
    const current = this.get(session);
    if (current.shown !== page && current.pages.includes(page))
      this.#put(session, { ...current, shown: page });
  }

  /** Closes a page; its neighbour (the next, else the previous) is shown instead. */
  close(session: string, page: string): void {
    const current = this.get(session);
    const index = current.pages.indexOf(page);
    if (index < 0) return;
    this.#close(session, page);
    const pages = current.pages.filter((other) => other !== page);
    const shown = current.shown === page ? (pages[index] ?? pages[index - 1]) : current.shown;
    this.#put(session, { pages, shown });
  }

  /** The sidebar's 浏览器 tab closed: every page of the conversation ends with it. */
  closeAll(session: string): void {
    for (const page of this.get(session).pages) this.#close(session, page);
    this.#sessions.delete(session);
    this.#notify();
  }

  resetForTests(): void {
    this.#sessions.clear();
    this.#closed.clear();
    this.#ids = 0;
  }

  #close(session: string, page: string): void {
    // The model's page is the same page each time the model opens it again.
    if (page !== MODEL_PAGE_ID) this.#closed.add(pageKey(session, page));
    void browserApi()?.command({ kind: 'close', sessionId: session, pageId: page });
  }

  #put(session: string, value: BrowserTabs): void {
    this.#sessions.set(session, value);
    this.#notify();
  }

  #notify(): void {
    for (const listener of this.#listeners) listener();
  }
}

export const browserTabs = new BrowserTabStore();

export function useBrowserTabs(session: string): BrowserTabs {
  return useSyncExternalStore(browserTabs.subscribe, () => browserTabs.get(session));
}

/** The 浏览器 tab and the sidebar, on one of its pages. */
function reveal(session: string, page: string): void {
  browserTabs.show(session, page);
  toolTabs.reveal(session, 'browser');
}

/**
 * The model's page: the first time it opens in a conversation it joins the row and is shown with
 * the sidebar, so the user sees the model at work; asked for from the conversation, it is shown.
 */
export function adoptModelPage(session: string): void {
  if (browserTabs.adopt(session, MODEL_PAGE_ID)) reveal(session, MODEL_PAGE_ID);
}

export function revealModelPage(session: string): void {
  browserTabs.adopt(session, MODEL_PAGE_ID);
  reveal(session, MODEL_PAGE_ID);
}

/**
 * An address, or a workspace web page, opens in a
 * new page of the browser, shown with the sidebar. A refused one opens no page: why is returned
 * to be said.
 */
export async function openInBrowser(
  session: string,
  target: { address: string } | { workspaceId: string; path: string },
): Promise<string | undefined> {
  const api = browserApi();
  if (api === undefined) return '内置浏览器不可用';
  const page = browserTabs.newId();
  // Opening reads the browser's settings first: a page must not load on rules not yet seen.
  const opened = await api.command({ kind: 'open', sessionId: session, pageId: page });
  const reply = !opened.ok
    ? opened
    : await api.command(
        'address' in target
          ? { kind: 'navigate', sessionId: session, pageId: page, input: target.address }
          : { kind: 'open-file', sessionId: session, pageId: page, ...target },
      );
  if (!reply.ok) {
    browserTabs.discard(session, page);
    return reply.message;
  }
  browserTabs.add(session, page);
  toolTabs.reveal(session, 'browser');
  return undefined;
}

export function resetBrowserTabsForTests(): void {
  browserTabs.resetForTests();
}
