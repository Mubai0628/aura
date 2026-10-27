/**
 * DevTools for the shared tabs through `chrome.debugger`. Chrome shows its "being debugged" bar
 * while Aura is attached: Aura attaches when a command needs a tab, leaves after a minute
 * without one, and leaves at once when the share ends or the user takes the tab. A tab's refs
 * outlive an idle detach, so the model can act on what it read a while ago.
 */
import type { DebuggerApi } from '../chrome-api.js';
import { CommandError } from '../protocol.js';
import { PageDebugger, type DevTools } from './facade.js';

type Params = Record<string, unknown>;
type MessageListener = (event: unknown, method: string, params: Params) => void;
type DetachListener = (event: unknown, reason: string) => void;

export const IDLE_MS = 60_000;

/** `chrome.debugger` for one tab, in the shape of Electron's `webContents.debugger`. */
class TabDevTools implements DevTools {
  #attaching: Promise<void> | null = null;
  readonly #messages: MessageListener[] = [];
  readonly #detaches: DetachListener[] = [];

  constructor(
    readonly tabId: number,
    readonly api: DebuggerApi,
  ) {}

  attach(protocolVersion = '1.3'): void {
    this.#attaching ??= this.api
      .attach({ tabId: this.tabId }, protocolVersion)
      .catch((error: unknown) => {
        this.#attaching = null;
        throw error;
      });
  }

  isAttached(): boolean {
    return this.#attaching !== null;
  }

  async sendCommand(method: string, params?: object): Promise<unknown> {
    if (this.#attaching === null) this.attach();
    await this.#attaching;
    return this.api.sendCommand({ tabId: this.tabId }, method, params);
  }

  on(event: 'message', listener: MessageListener): unknown;
  on(event: 'detach', listener: DetachListener): unknown;
  on(event: 'message' | 'detach', listener: MessageListener | DetachListener): unknown {
    if (event === 'message') this.#messages.push(listener as MessageListener);
    else this.#detaches.push(listener as DetachListener);
    return this;
  }

  message(method: string, params: Params): void {
    for (const listener of this.#messages) listener(undefined, method, params);
  }

  detached(reason: string): void {
    this.#attaching = null;
    for (const listener of this.#detaches) listener(undefined, reason);
  }

  async detach(): Promise<void> {
    if (this.#attaching === null) return;
    await this.api.detach({ tabId: this.tabId }).catch(() => undefined);
    this.detached('released');
  }
}

interface Entry {
  tools: TabDevTools;
  page: PageDebugger;
  idle: unknown;
}

export interface Timers {
  set(run: () => void, ms: number): unknown;
  clear(timer: unknown): void;
}

export class Pages {
  readonly #api: DebuggerApi;
  readonly #timers: Timers;
  readonly #byTab = new Map<number, Entry>();

  constructor(api: DebuggerApi, timers: Timers) {
    this.#api = api;
    this.#timers = timers;
    api.onEvent.addListener((source, method, params) => {
      const entry = this.#byTab.get(source.tabId ?? -1);
      entry?.tools.message(method, (params ?? {}) as Params);
    });
    api.onDetach.addListener((source, reason) => {
      this.#byTab.get(source.tabId ?? -1)?.tools.detached(reason);
    });
  }

  /** The tab's DevTools, attached and reading the page's events; the idle minute starts again. */
  async page(tabId: number): Promise<PageDebugger> {
    let entry = this.#byTab.get(tabId);
    if (entry === undefined) {
      const tools = new TabDevTools(tabId, this.#api);
      entry = { tools, page: new PageDebugger(tools, 'chrome.snapshot'), idle: null };
      this.#byTab.set(tabId, entry);
    }
    this.#timers.clear(entry.idle);
    const current = entry;
    current.idle = this.#timers.set(() => void current.tools.detach(), IDLE_MS);
    try {
      await current.page.ready();
    } catch (error) {
      const why = error instanceof Error ? error.message : String(error);
      throw new CommandError(
        'chrome.refused',
        `Chrome 不让 Aura 调试这个标签页（${why}）：可能开着它的开发者工具，或这是 Chrome 自己的页面。这一步没有执行。`,
      );
    }
    return current.page;
  }

  attached(tabId: number): boolean {
    return this.#byTab.get(tabId)?.tools.isAttached() ?? false;
  }

  /** Leaves the tab and forgets it: the share ended or the tab closed. */
  async forget(tabId: number): Promise<void> {
    const entry = this.#byTab.get(tabId);
    if (entry === undefined) return;
    this.#byTab.delete(tabId);
    this.#timers.clear(entry.idle);
    await entry.tools.detach();
  }

  /** Leaves the tab for now, keeping its refs, while the user has it. */
  async pause(tabId: number): Promise<void> {
    const entry = this.#byTab.get(tabId);
    if (entry === undefined) return;
    this.#timers.clear(entry.idle);
    await entry.tools.detach();
  }

  tabs(): number[] {
    return [...this.#byTab.keys()];
  }
}
