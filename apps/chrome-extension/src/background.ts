/**
 * The extension's background: the connection to Aura, Core's commands, the shared tabs, a tab
 * handed to the user, and the side panels that show all of it.
 */
import { base64ToBytes, bytesToBase64 } from './authentication.js';
import { Assists } from './assist.js';
import type { ChromeApi, Port, StorageArea } from './chrome-api.js';
import { parseCommand, run } from './commands/index.js';
import type { CommandContext } from './commands/context.js';
import { NativeConnection, type SecretStore } from './native.js';
import { Pages, type Timers } from './page/pages.js';
import { PANEL_PORT, type PanelRequest, type PanelState } from './panel-messages.js';
import { CommandError, type CommandOutcome, type CoreMessage } from './protocol.js';
import { SharedTabs } from './shares.js';

const SECRET_KEY = 'pairing_secret';

export function storageSecrets(area: StorageArea): SecretStore {
  return {
    async load() {
      const value = (await area.get(SECRET_KEY))[SECRET_KEY];
      return typeof value === 'string' ? base64ToBytes(value) : null;
    },
    async save(secret) {
      await area.set({ [SECRET_KEY]: bytesToBase64(secret) });
    },
    async forget() {
      await area.remove(SECRET_KEY);
    },
  };
}

export interface BackgroundDeps {
  chrome: ChromeApi;
  hostName: string;
  timers: Timers;
  now(): number;
  sleep(ms: number): Promise<void>;
}

export class Background {
  readonly connection: NativeConnection;
  readonly pages: Pages;
  readonly shared: SharedTabs;
  readonly assists: Assists;
  readonly #deps: BackgroundDeps;
  readonly #panels = new Set<Port>();
  readonly #running = new Map<number, AbortController>();
  #state: PanelState;

  constructor(deps: BackgroundDeps) {
    this.#deps = deps;
    const { chrome, timers } = deps;
    this.pages = new Pages(chrome.debugger, timers);
    this.connection = new NativeConnection({
      hostName: deps.hostName,
      extensionId: chrome.runtime.id,
      version: chrome.runtime.getManifest().version,
      connect: (name) => chrome.runtime.connectNative(name),
      lastError: () => chrome.runtime.lastError?.message,
      secrets: storageSecrets(chrome.storage.local),
      onMessage: (message) => void this.#received(message),
      onState: (connection) => this.#publish({ connection }),
      setTimer: (work, ms) => timers.set(work, ms),
      clearTimer: (timer) => timers.clear(timer),
    });
    this.#state = {
      connection: this.connection.state,
      sessions: [],
      shares: [],
      assist: null,
      share_refused: null,
    };
    this.shared = new SharedTabs({
      tabs: chrome.tabs,
      tabGroups: chrome.tabGroups,
      pages: this.pages,
      send: (message) => this.connection.send(message),
    });
    this.assists = new Assists({
      focus: async (tabId) => {
        await chrome.tabs.update(tabId, { active: true });
      },
      onChange: () => this.#publish({ assist: this.assists.current }),
      setTimer: (work, ms) => timers.set(work, ms),
      clearTimer: (timer) => timers.clear(timer),
      now: deps.now,
    });
  }

  get state(): PanelState {
    return this.#state;
  }

  start(): void {
    const { chrome } = this.#deps;
    chrome.runtime.onConnect.addListener((port) => {
      if (port.name === PANEL_PORT) this.#panel(port);
    });
    chrome.tabs.onUpdated.addListener((tabId, change, tab) =>
      this.shared.tabChanged(tabId, change, tab),
    );
    chrome.tabs.onRemoved.addListener((tabId) => {
      this.assists.tabClosed(tabId);
      void this.shared.tabRemoved(tabId);
    });
    this.connection.start();
  }

  #panel(port: Port): void {
    this.#panels.add(port);
    port.onDisconnect.addListener(() => this.#panels.delete(port));
    port.onMessage.addListener((message) => this.request(message as PanelRequest));
    port.postMessage(this.#state);
    // The conversations change in Aura while no panel is open.
    this.connection.send({ type: 'list_sessions' });
  }

  #publish(change: Partial<PanelState>): void {
    this.#state = { ...this.#state, ...change };
    for (const port of this.#panels) port.postMessage(this.#state);
  }

  request(request: PanelRequest): void {
    switch (request.type) {
      case 'pair':
        this.connection.pair(request.code.trim());
        return;
      case 'share':
        this.#publish({ share_refused: null });
        this.connection.send(request);
        return;
      case 'unshare':
      case 'stop':
      case 'answer_origin':
        this.connection.send(request);
        return;
      case 'assist':
        this.assists.answer(request.outcome);
        return;
      case 'reconnect':
        this.connection.start();
        return;
    }
  }

  async #received(message: CoreMessage): Promise<void> {
    switch (message.type) {
      case 'command':
        await this.#execute(message.id, message.command, message.origin);
        return;
      case 'cancel':
        this.#running.get(message.id)?.abort();
        return;
      case 'sessions':
        this.#publish({ sessions: message.sessions });
        return;
      case 'shares':
        await this.shared.update(message.shares);
        this.#publish({ shares: message.shares });
        return;
      case 'share_refused':
        this.#publish({ share_refused: message.reason });
        return;
      case 'assist_done':
        if (this.assists.current?.tab_id === message.tab_id) this.assists.answer('done');
        return;
      default:
        return;
    }
  }

  #context(signal: AbortSignal): CommandContext {
    const { chrome } = this.#deps;
    return {
      tabs: chrome.tabs,
      windows: chrome.windows,
      pages: this.pages,
      group: (tab) => this.shared.group(tab),
      assist: (tabId, reason, seconds) => this.assists.wait(tabId, reason, seconds, signal),
      signal,
      sleep: this.#deps.sleep,
      now: this.#deps.now,
    };
  }

  async #execute(id: number, value: unknown, origin: string | undefined): Promise<void> {
    const controller = new AbortController();
    this.#running.set(id, controller);
    let outcome: CommandOutcome;
    try {
      const command = parseCommand(value);
      outcome = {
        status: 'done',
        value: await run(this.#context(controller.signal), command, origin),
      };
    } catch (error) {
      outcome =
        error instanceof CommandError
          ? { status: 'failed', code: error.code, message: error.message }
          : {
              status: 'failed',
              code: 'chrome.failed',
              message: error instanceof Error ? error.message : String(error),
            };
    } finally {
      this.#running.delete(id);
    }
    // Nobody waits for a release (id 0) or for a command Core withdrew.
    if (id !== 0 && !controller.signal.aborted)
      this.connection.send({ type: 'result', id, outcome });
  }
}
