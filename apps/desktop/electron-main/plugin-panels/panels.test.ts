// @vitest-environment node
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { PluginPanelState } from '../../shared/plugin-panel-api.js';
import {
  PluginPanels,
  RELOAD_QUIET_MS,
  type PanelContents,
  type PanelView,
  type PanelViews,
} from './panels.js';
import type { PanelSite } from './site.js';

const S1 = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const S2 = 'ses_01BX5ZZKBKACTAV9WEVGEMMVRZ';

type Listener = (...args: never[]) => void;

interface FakeContents extends PanelContents {
  emit(event: string, ...args: unknown[]): void;
  opening?: () => { action: 'deny' };
  closed: boolean;
}

interface FakeView extends PanelView {
  readonly webContents: FakeContents;
  visible: boolean;
  bounds?: unknown;
}

const site = (plugin = 'usage-panel', capabilities = ['session.read']): PanelSite => ({
  plugin,
  title: '本会话用量',
  folder: `C:\\plugins\\${plugin}`,
  entry: 'panel/index.html',
  network: [],
  capabilities,
});

let ids = 0;
let created: FakeView[];
let detached: FakeView[];
let watching: { site: PanelSite; changed: () => void; stopped: boolean }[];
let problems: string[];
let views: PanelViews;

function fakeView(): FakeView {
  const listeners = new Map<string, Listener[]>();
  const contents: FakeContents = {
    id: ++ids,
    closed: false,
    loadURL: vi.fn(async () => undefined),
    reload: vi.fn(),
    close: vi.fn(() => {
      contents.closed = true;
    }),
    isDestroyed: () => contents.closed,
    setZoomFactor: vi.fn(),
    setWindowOpenHandler: (handler) => {
      contents.opening = handler;
    },
    on: (event, listener) => {
      listeners.set(event, [...(listeners.get(event) ?? []), listener]);
      return contents;
    },
    emit: (event, ...args) => {
      for (const listener of listeners.get(event) ?? [])
        (listener as (...values: unknown[]) => void)(...args);
    },
  };
  const view: FakeView = {
    webContents: contents,
    visible: true,
    setBounds: (bounds) => {
      view.bounds = bounds;
    },
    setVisible: (visible) => {
      view.visible = visible;
    },
  };
  return view;
}

beforeEach(() => {
  created = [];
  detached = [];
  watching = [];
  problems = [];
  views = {
    create: () => {
      const view = fakeView();
      created.push(view);
      return view;
    },
    attach: vi.fn(),
    detach: (view) => detached.push(view as FakeView),
    watch: (watched, changed) => {
      const entry = { site: watched, changed, stopped: false };
      watching.push(entry);
      return () => {
        entry.stopped = true;
      };
    },
  };
});

afterEach(() => {
  vi.useRealTimers();
});

const host = () => new PluginPanels(views, (_panel, text) => problems.push(text));

test('a panel opens once per plugin and conversation, and stays while Core describes it alike', () => {
  const panels = host();
  const first = panels.open(S1, site());
  expect(panels.open(S1, site())).toBe(first);
  expect(created).toHaveLength(1);
  expect(created[0]!.webContents.loadURL).toHaveBeenCalledWith(
    'aura-plugin://usage-panel/index.html',
  );
  expect(views.attach).toHaveBeenCalledWith(created[0]);
  // It waits hidden until its tab places it.
  expect(created[0]!.visible).toBe(false);
  expect(panels.byContents(created[0]!.webContents.id)).toBe(first);

  // Another conversation gets its own page of the same plugin.
  panels.open(S2, site());
  expect(created).toHaveLength(2);

  // Core now describes the plugin differently: the old page goes and a new one opens.
  const changed = panels.open(S1, site('usage-panel', ['session.read', 'session.send']));
  expect(changed).not.toBe(first);
  expect(created[0]!.webContents.closed).toBe(true);
  expect(detached).toContain(created[0]);
  // What the plugin's pages may reach follows Core's newest description.
  expect(panels.site('usage-panel')?.capabilities).toEqual(['session.read', 'session.send']);
});

test('showing a panel lays it over its tab and hides every other panel', () => {
  const panels = host();
  panels.open(S1, site());
  panels.open(S2, site('other-panel'));
  const [first, second] = created as [FakeView, FakeView];
  expect(panels.show(S2, 'other-panel', { x: 1, y: 2, width: 3, height: 4 }, 1)).toBe(true);
  expect(panels.show(S1, 'usage-panel', { x: 10, y: 20, width: 300, height: 400 }, 1.25)).toBe(
    true,
  );
  expect(first.visible).toBe(true);
  expect(first.bounds).toEqual({ x: 10, y: 20, width: 300, height: 400 });
  expect(first.webContents.setZoomFactor).toHaveBeenCalledWith(1.25);
  expect(second.visible).toBe(false);
  panels.hide(S1, 'usage-panel');
  expect(first.visible).toBe(false);
  expect(panels.show(S1, 'missing', { x: 0, y: 0, width: 1, height: 1 }, 1)).toBe(false);
});

test('closing a panel ends its page and stops watching its files', () => {
  const panels = host();
  panels.open(S1, site());
  const [view] = created as [FakeView];
  panels.close(S1, 'usage-panel');
  expect(view.webContents.closed).toBe(true);
  expect(detached).toEqual([view]);
  expect(watching[0]!.stopped).toBe(true);
  expect(panels.get(S1, 'usage-panel')).toBeUndefined();
  expect(panels.byContents(view.webContents.id)).toBeUndefined();
  expect(panels.site('usage-panel')).toBeUndefined();

  // The window's page started again: every panel goes with its tabs.
  panels.open(S1, site());
  panels.open(S2, site());
  panels.reset();
  expect(created.every((each) => each.webContents.closed)).toBe(true);
});

test('each panel that closes is told of, for what it left behind; the open ones are listed', () => {
  const panels = host();
  const closed: string[] = [];
  panels.onClosed((panel) => closed.push(`${panel.sessionId} ${panel.site.plugin}`));
  panels.open(S1, site());
  panels.open(S2, site());
  expect(panels.list().map((panel) => panel.sessionId)).toEqual([S1, S2]);
  panels.close(S1, 'usage-panel');
  // Core describes it differently: the old page closes as the new one opens.
  const changed = panels.open(S2, site('usage-panel', ['session.read', 'session.send']));
  panels.stop(changed, '插件现在不能用');
  // What is closed already closes no more.
  panels.close(S1, 'usage-panel');
  panels.stop(changed, '插件现在不能用');
  expect(closed).toEqual([`${S1} usage-panel`, `${S2} usage-panel`, `${S2} usage-panel`]);
  expect(panels.list()).toEqual([]);
});

test('a page that crashes or fails to load is stopped and its tab is told why', () => {
  const panels = host();
  const states: PluginPanelState[] = [];
  panels.onState((state) => states.push(state));
  panels.open(S1, site());
  const [crashing] = created as [FakeView];
  crashing.webContents.emit('render-process-gone', {}, { reason: 'clean-exit' });
  expect(states).toEqual([]);
  crashing.webContents.emit('render-process-gone', {}, { reason: 'crashed' });
  expect(states).toEqual([
    { sessionId: S1, plugin: 'usage-panel', stopped: '面板停止运行了（crashed）' },
  ]);
  expect(crashing.webContents.closed).toBe(true);
  expect(problems).toContain('面板停止运行了（crashed）');

  panels.open(S1, site());
  const failing = created[1]!;
  // A load the page replaced itself, or a frame's, is not the panel failing.
  failing.webContents.emit(
    'did-fail-load',
    {},
    -3,
    'ERR_ABORTED',
    'aura-plugin://usage-panel/',
    true,
  );
  failing.webContents.emit('did-fail-load', {}, -105, 'ERR_NAME_NOT_RESOLVED', 'https://x/', false);
  expect(states).toHaveLength(1);
  failing.webContents.emit(
    'did-fail-load',
    {},
    -6,
    'ERR_FILE_NOT_FOUND',
    'aura-plugin://usage-panel/index.html',
    true,
  );
  expect(states.at(-1)?.stopped).toBe(
    '面板没有打开：ERR_FILE_NOT_FOUND（aura-plugin://usage-panel/index.html）',
  );
  expect(failing.webContents.closed).toBe(true);
});

test('a page stays on its own origin and opens no windows; what it tried goes to the log', () => {
  const panels = host();
  panels.open(S1, site());
  const [view] = created as [FakeView];
  const leave = { url: 'https://example.com/', preventDefault: vi.fn() };
  view.webContents.emit('will-navigate', leave);
  expect(leave.preventDefault).toHaveBeenCalled();
  const home = { url: 'aura-plugin://usage-panel/other.html', preventDefault: vi.fn() };
  view.webContents.emit('will-navigate', home);
  expect(home.preventDefault).not.toHaveBeenCalled();
  const redirect = { url: 'aura-plugin://other-plugin/index.html', preventDefault: vi.fn() };
  view.webContents.emit('will-redirect', redirect);
  expect(redirect.preventDefault).toHaveBeenCalled();
  expect(view.webContents.opening?.()).toEqual({ action: 'deny' });
  expect(problems).toEqual([
    '页面想离开面板，跳到 https://example.com/，已拦下',
    '页面想离开面板，跳到 aura-plugin://other-plugin/index.html，已拦下',
    '页面想打开新窗口，已拦下',
  ]);

  // Errors the page logs are kept for `plugins.logs`; its other messages are not.
  view.webContents.emit('console-message', {
    level: 'warning',
    message: 'slow',
    sourceId: '',
    lineNumber: 0,
  });
  view.webContents.emit('console-message', {
    level: 'error',
    message: 'TypeError: x is undefined',
    sourceId: 'aura-plugin://usage-panel/app.js',
    lineNumber: 3,
  });
  expect(problems.at(-1)).toBe('TypeError: x is undefined（aura-plugin://usage-panel/app.js:3）');
  expect(problems).toHaveLength(4);
});

test('a panel being written reloads once its files have been quiet a moment', () => {
  vi.useFakeTimers();
  const panels = host();
  panels.open(S1, site());
  const [view] = created as [FakeView];
  const [watched] = watching as [(typeof watching)[number]];
  expect(watched.site.folder).toBe('C:\\plugins\\usage-panel');
  watched.changed();
  vi.advanceTimersByTime(RELOAD_QUIET_MS - 50);
  watched.changed();
  watched.changed();
  vi.advanceTimersByTime(RELOAD_QUIET_MS - 50);
  expect(view.webContents.reload).not.toHaveBeenCalled();
  vi.advanceTimersByTime(60);
  expect(view.webContents.reload).toHaveBeenCalledTimes(1);

  // A change that arrives as the panel closes reloads nothing.
  watched.changed();
  panels.close(S1, 'usage-panel');
  vi.advanceTimersByTime(RELOAD_QUIET_MS * 2);
  expect(view.webContents.reload).toHaveBeenCalledTimes(1);
});
