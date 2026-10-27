/**
 * One page's DevTools connection. It records what the page reports (console messages,
 * uncaught errors, loads that failed, its requests) and answers its dialogs so a page can never
 * block on one; the model reads them with its next browser result. Addresses are kept with their
 * credentials hidden.
 */
import { NetworkLog, redactUrl } from './network-log.js';
import { RefTable } from './snapshot.js';

type Params = Record<string, unknown>;

/** The parts of Electron's `webContents.debugger` this uses; tests pass a fake. */
export interface DevTools {
  attach(protocolVersion?: string): void;
  isAttached(): boolean;
  sendCommand(method: string, params?: object): Promise<unknown>;
  on(event: 'message', listener: (event: unknown, method: string, params: Params) => void): unknown;
  on(event: 'detach', listener: (event: unknown, reason: string) => void): unknown;
}

export interface ConsoleEntry {
  level: 'log' | 'info' | 'warning' | 'error' | 'exception' | 'failed_load';
  text: string;
  source: string | null;
}

const KEEP = 500;
const LONGEST = 500;
const DOMAINS = [
  'Page.enable',
  'Runtime.enable',
  'DOM.enable',
  'Accessibility.enable',
  'Network.enable',
  'Log.enable',
  'Overlay.enable',
];

const record = (value: unknown): Params =>
  typeof value === 'object' && value !== null ? (value as Params) : {};
const short = (value: string) => (value.length > LONGEST ? `${value.slice(0, LONGEST)}…` : value);

function argumentText(argument: unknown): string {
  const value = record(argument);
  if ('value' in value)
    return typeof value.value === 'string' ? value.value : JSON.stringify(value.value);
  if (typeof value.unserializableValue === 'string') return value.unserializableValue;
  return typeof value.description === 'string' ? value.description : String(value.type ?? '');
}

function where(frame: unknown): string | null {
  const value = record(frame);
  if (typeof value.url !== 'string' || value.url === '') return null;
  const line = typeof value.lineNumber === 'number' ? `:${value.lineNumber + 1}` : '';
  return `${value.url}${line}`;
}

export class PageDebugger {
  readonly refs = new RefTable();
  /** The page's requests, for `browser.network`. */
  readonly network = new NetworkLog();
  /** Synthetic input in flight: the page's input events now are Aura's, not the user's. */
  synthetic = 0;
  readonly #devtools: DevTools;
  #entries: ConsoleEntry[] = [];
  /** The first entry the next result has not reported yet. */
  #unread = 0;
  #dialogs: string[] = [];
  readonly #urls = new Map<string, string>();
  #enabled: Promise<void> | undefined;

  /**
   * The tool that reads the page again, as the model is told to call it when a ref is unknown.
   */
  readonly snapshotTool: string;

  constructor(devtools: DevTools, snapshotTool = 'browser.snapshot') {
    this.#devtools = devtools;
    this.snapshotTool = snapshotTool;
    devtools.on('message', (_event, method, params) => this.#message(method, record(params)));
    devtools.on('detach', () => {
      this.#enabled = undefined;
    });
  }

  /** Attached, with the domains this host reads; once per connection. */
  ready(): Promise<void> {
    this.#enabled ??= (async () => {
      if (!this.#devtools.isAttached()) this.#devtools.attach('1.3');
      for (const domain of DOMAINS) await this.#devtools.sendCommand(domain);
    })().catch((error: unknown) => {
      this.#enabled = undefined;
      throw error;
    });
    return this.#enabled;
  }

  send<T = Params>(method: string, params?: object): Promise<T> {
    return this.#devtools.sendCommand(method, params) as Promise<T>;
  }

  /** What the page reported since the previous result, at most `limit` of the newest. */
  takeNew(limit = 20): ConsoleEntry[] {
    const fresh = this.#entries.slice(this.#unread);
    this.#unread = this.#entries.length;
    return fresh.slice(-limit);
  }

  /** Everything kept, newest last. */
  all(
    level: 'error' | 'warning' | 'all',
    max: number,
  ): { entries: ConsoleEntry[]; truncated: boolean } {
    const wanted = this.#entries.filter((entry) =>
      level === 'all'
        ? true
        : level === 'error'
          ? ['error', 'exception', 'failed_load'].includes(entry.level)
          : entry.level !== 'log' && entry.level !== 'info',
    );
    return { entries: wanted.slice(-max), truncated: wanted.length > max };
  }

  takeDialogs(): string[] {
    const dialogs = this.#dialogs;
    this.#dialogs = [];
    return dialogs;
  }

  #push(entry: ConsoleEntry): void {
    this.#entries.push({ ...entry, text: short(entry.text) });
    if (this.#entries.length > KEEP) {
      const dropped = this.#entries.length - KEEP;
      this.#entries = this.#entries.slice(dropped);
      this.#unread = Math.max(0, this.#unread - dropped);
    }
  }

  #message(method: string, params: Params): void {
    this.network.message(method, params);
    switch (method) {
      case 'Runtime.consoleAPICalled': {
        const type = String(params.type ?? 'log');
        const level =
          type === 'error' || type === 'assert'
            ? 'error'
            : type === 'warning'
              ? 'warning'
              : type === 'info'
                ? 'info'
                : 'log';
        const args = Array.isArray(params.args) ? params.args : [];
        // Electron's own warning in an unpackaged build is not the page's.
        if (argumentText(args[0]).startsWith('%cElectron Security Warning')) return;
        const frames = record(params.stackTrace).callFrames;
        this.#push({
          level,
          text: args.map(argumentText).join(' '),
          source: Array.isArray(frames) ? where(frames[0]) : null,
        });
        return;
      }
      case 'Runtime.exceptionThrown': {
        const details = record(params.exceptionDetails);
        const exception = record(details.exception);
        this.#push({
          level: 'exception',
          text: String(exception.description ?? details.text ?? 'Uncaught error'),
          source: where(details),
        });
        return;
      }
      case 'Network.requestWillBeSent': {
        const url = record(params.request).url;
        if (typeof params.requestId === 'string' && typeof url === 'string')
          this.#urls.set(params.requestId, url);
        return;
      }
      case 'Network.responseReceived': {
        const response = record(params.response);
        const status = typeof response.status === 'number' ? response.status : 0;
        if (status >= 400)
          this.#push({
            level: 'failed_load',
            text: `HTTP ${status} ${redactUrl(String(response.url ?? ''))}`,
            source: null,
          });
        return;
      }
      case 'Network.loadingFailed': {
        const id = typeof params.requestId === 'string' ? params.requestId : '';
        const url = redactUrl(this.#urls.get(id) ?? '');
        this.#urls.delete(id);
        const error = String(params.errorText ?? '');
        if (params.canceled === true || error === 'net::ERR_ABORTED') return;
        const blocked = error === 'net::ERR_BLOCKED_BY_CLIENT' ? '（浏览器设置不允许）' : '';
        this.#push({
          level: 'failed_load',
          text: `${error}${blocked} ${url}`.trim(),
          source: null,
        });
        return;
      }
      case 'Network.loadingFinished':
        if (typeof params.requestId === 'string') this.#urls.delete(params.requestId);
        return;
      case 'Page.javascriptDialogOpening': {
        // An alert is acknowledged and a page may leave; a question is answered no.
        const type = String(params.type ?? 'alert');
        const accept = type === 'alert' || type === 'beforeunload';
        this.#dialogs.push(
          `${type}: ${short(String(params.message ?? ''))}（Aura 已${accept ? '确认' : '取消'}）`,
        );
        void this.#devtools
          .sendCommand('Page.handleJavaScriptDialog', { accept })
          .catch(() => undefined);
        return;
      }
      case 'Page.frameNavigated':
        // A new document: the old refs name nodes that no longer exist.
        if (record(params.frame).parentId === undefined) this.refs.clear();
        return;
    }
  }
}
