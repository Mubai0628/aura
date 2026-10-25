// @vitest-environment node

import type { BrowserPageState } from '../../shared/browser-api.js';
import { BrowserPages, type PageContents, type PageView, type PageViews } from './pages.js';
import type { BrowserPolicy } from './policy.js';
import { parsedRules } from './site-rules.js';

export class FakeContents implements PageContents {
  static next = 1;
  readonly id = FakeContents.next++;
  url = '';
  loads: string[] = [];
  closed = false;
  handlers = new Map<string, ((...args: never[]) => void)[]>();
  openHandler?: (details: { url: string }) => { action: 'deny' };
  readonly navigationHistory = {
    canGoBack: () => false,
    canGoForward: () => false,
    goBack: () => undefined,
    goForward: () => undefined,
  };
  loadURL(url: string) {
    this.loads.push(url);
    this.url = url;
    return Promise.resolve();
  }
  getURL() {
    return this.url;
  }
  getTitle() {
    return '';
  }
  isLoading() {
    return false;
  }
  isDestroyed() {
    return this.closed;
  }
  stop() {}
  reload() {}
  close() {
    this.closed = true;
  }
  openDevTools() {}
  setWindowOpenHandler(handler: (details: { url: string }) => { action: 'deny' }) {
    this.openHandler = handler;
  }
  on(event: string, listener: (...args: never[]) => void) {
    this.handlers.set(event, [...(this.handlers.get(event) ?? []), listener]);
    return this;
  }
  fire(event: string, ...args: unknown[]) {
    for (const listener of this.handlers.get(event) ?? [])
      (listener as (...a: unknown[]) => void)(...args);
  }
}

export class FakeView implements PageView {
  readonly webContents = new FakeContents();
  bounds?: unknown;
  visible = true;
  setBounds(bounds: unknown) {
    this.bounds = bounds;
  }
  setVisible(visible: boolean) {
    this.visible = visible;
  }
}

export function setup(limit = 8) {
  const attached = new Set<FakeView>();
  const views: PageViews = {
    create: () => new FakeView(),
    attach: (view) => attached.add(view as FakeView),
    detach: (view) => attached.delete(view as FakeView),
  };
  let clock = 0;
  const pages = new BrowserPages(views, { limit, now: () => ++clock });
  const states: BrowserPageState[] = [];
  pages.onState((state) => states.push(state));
  const view = (index: number) => [...attached][index]!;
  return { pages, states, attached, view };
}

export const POLICY: BrowserPolicy = {
  enabled: true,
  allowed: parsedRules(['docs.rs']),
  blocked: parsedRules(['blocked.example', '*.tracker.example']),
};
export const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
