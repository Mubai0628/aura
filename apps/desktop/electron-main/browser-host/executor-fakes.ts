/**
 * A model page that loads nothing and a DevTools connection that answers what the
 * executor asks, for the executor's tests.
 */
import type { BrowserHostActionV1, BrowserHostCommandV1 } from '@aura/contracts/generated';

import type { ExecutorDeps } from './actions.js';
import { PageDebugger, type DevTools } from './cdp.js';
import { BrowserPages, type PageContents, type PageView, type PageViews } from './pages.js';
import { parsedRules } from './site-rules.js';

export class Contents implements PageContents {
  static next = 1;
  readonly id = Contents.next++;
  url = 'about:blank';
  loads: string[] = [];
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
    return 'Game';
  }
  isLoading() {
    return false;
  }
  isDestroyed() {
    return false;
  }
  stop() {}
  reload() {}
  close() {}
  setWindowOpenHandler() {}
  openDevTools() {}
  on() {
    return this;
  }
}

export class Tools implements DevTools {
  sent: [string, Record<string, unknown>][] = [];
  /** Answers a test gives to a method, before the defaults. */
  readonly answers = new Map<string, (params: Record<string, unknown>) => unknown>();
  #listeners: ((event: unknown, method: string, params: Record<string, unknown>) => void)[] = [];
  attach() {}
  isAttached() {
    return true;
  }
  on(event: string, listener: (event: unknown, method: string, params: never) => void) {
    if (event === 'message') this.#listeners.push(listener as never);
    return this;
  }
  /** What the page reports, as DevTools would tell it. */
  emit(method: string, params: Record<string, unknown>) {
    for (const listener of this.#listeners) listener({}, method, params);
  }
  sendCommand(method: string, params: Record<string, unknown> = {}) {
    this.sent.push([method, params]);
    const answer = this.answers.get(method);
    if (answer !== undefined) return Promise.resolve().then(() => answer(params));
    if (method === 'Accessibility.getFullAXTree')
      return Promise.resolve({
        nodes: [
          {
            nodeId: '1',
            role: { value: 'RootWebArea' },
            name: { value: 'Game' },
            childIds: ['2', '3'],
          },
          {
            nodeId: '2',
            parentId: '1',
            role: { value: 'button' },
            name: { value: 'Start' },
            backendDOMNodeId: 50,
          },
          {
            nodeId: '3',
            parentId: '1',
            role: { value: 'textbox' },
            name: { value: 'Password' },
            backendDOMNodeId: 60,
          },
        ],
      });
    if (method === 'DOM.getContentQuads')
      return Promise.resolve({ quads: [[100, 200, 140, 200, 140, 220, 100, 220]] });
    if (method === 'DOM.describeNode')
      return Promise.resolve({
        node: {
          nodeName: 'INPUT',
          attributes: params.backendNodeId === 60 ? ['type', 'password'] : ['type', 'text'],
        },
      });
    return Promise.resolve({});
  }
  methods() {
    return this.sent.map(([method]) => method);
  }
}

export function setup(overrides: Partial<ExecutorDeps> = {}) {
  const views: PageViews = {
    create: () =>
      ({ webContents: new Contents(), setBounds() {}, setVisible() {} }) as unknown as PageView,
    attach() {},
    detach() {},
  };
  const pages = new BrowserPages(views);
  pages.setPolicy({ enabled: true, allowed: [], blocked: parsedRules(['blocked.example']) });
  const tools = new Tools();
  const devtools = new PageDebugger(tools);
  const deps: ExecutorDeps = {
    pages,
    debuggerFor: () => devtools,
    emulate: async () => 0.5,
    assist: async () => 'done',
    ...overrides,
  };
  return { pages, tools, devtools, deps };
}

export const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
export const command = (
  action: BrowserHostActionV1,
  expected: string | null = null,
): BrowserHostCommandV1 => ({
  command_id: 'bcm-1',
  runtime_instance_id:
    'run_01ARZ3NDEKTSV4RRFFQ69G5FAV' as BrowserHostCommandV1['runtime_instance_id'],
  session_id: S as BrowserHostCommandV1['session_id'],
  expected_host: expected,
  access: { read_only: false, approved_hosts: [], file_root: null },
  action,
  timeout_ms: 20_000,
});
