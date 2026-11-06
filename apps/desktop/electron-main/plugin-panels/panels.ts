/**
 * The open panels: one view per plugin and conversation, kept in the Aura window while its tab
 * exists (shown in the right sidebar, otherwise hidden). A page that crashes, fails to load or
 * tries to leave its own origin is stopped and says why; nothing it does reaches another panel.
 */
import type { BrowserBounds } from '../../shared/browser-api.js';
import type { PluginPanelState } from '../../shared/plugin-panel-api.js';
import { CallBudget } from './calls.js';
import { entryUrl, PANEL_SCHEME, type PanelSite } from './site.js';

type Listener = (...args: never[]) => void;

/** The parts of a panel's WebContents Main uses; tests pass a fake. */
export interface PanelContents {
  readonly id: number;
  loadURL(url: string): Promise<void>;
  reload(): void;
  close(): void;
  isDestroyed(): boolean;
  setZoomFactor(factor: number): void;
  setWindowOpenHandler(handler: () => { action: 'deny' }): void;
  on(event: string, listener: Listener): unknown;
}
export interface PanelView {
  readonly webContents: PanelContents;
  setBounds(bounds: BrowserBounds): void;
  setVisible(visible: boolean): void;
}
export interface PanelViews {
  /** A new view in the plugin's own partition, its preload given the declared capabilities. */
  create(site: PanelSite): PanelView;
  attach(view: PanelView): void;
  detach(view: PanelView): void;
  /** Calls `changed` when a file in the plugin's `panel/` folder changes; returns the stop. */
  watch(site: PanelSite, changed: () => void): () => void;
}

export interface OpenPanel {
  readonly sessionId: string;
  readonly site: PanelSite;
  readonly view: PanelView;
  readonly budget: CallBudget;
  unwatch?: () => void;
}

/** How long the folder stays quiet before an edited panel reloads. */
export const RELOAD_QUIET_MS = 300;

/** Something a panel's page did that belongs in its plugin's log. */
export type PanelProblem = (panel: OpenPanel, text: string) => void;

const keyOf = (sessionId: string, plugin: string) => `${sessionId}\u0000${plugin}`;
export const sameSite = (a: PanelSite, b: PanelSite) => JSON.stringify(a) === JSON.stringify(b);
// Main's URL parser gives a scheme it does not know an opaque origin, so the parts are compared.
const sameOrigin = (url: string, plugin: string) => {
  try {
    const parsed = new URL(url);
    return parsed.protocol === `${PANEL_SCHEME}:` && parsed.host === plugin;
  } catch {
    return false;
  }
};

export class PluginPanels {
  readonly #views: PanelViews;
  readonly #problem: PanelProblem;
  readonly #panels = new Map<string, OpenPanel>();
  readonly #byContents = new Map<number, OpenPanel>();
  readonly #listeners = new Set<(state: PluginPanelState) => void>();
  readonly #closed = new Set<(panel: OpenPanel) => void>();

  constructor(views: PanelViews, problem: PanelProblem) {
    this.#views = views;
    this.#problem = problem;
  }

  onState(listener: (state: PluginPanelState) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  /** Each panel as it closes, for whatever it left behind (its status entry). */
  onClosed(listener: (panel: OpenPanel) => void): () => void {
    this.#closed.add(listener);
    return () => this.#closed.delete(listener);
  }

  /** The panels open now. */
  list(): OpenPanel[] {
    return [...this.#panels.values()];
  }

  /** The panel whose page made a call, by its WebContents. */
  byContents(id: number): OpenPanel | undefined {
    return this.#byContents.get(id);
  }

  /** What the plugin's panels are now: the newest site Core gave for it. */
  site(plugin: string): PanelSite | undefined {
    let found: PanelSite | undefined;
    for (const panel of this.#panels.values()) if (panel.site.plugin === plugin) found = panel.site;
    return found;
  }

  /** Opens the panel, or keeps the open one while Core still describes it the same way. */
  open(sessionId: string, site: PanelSite): OpenPanel {
    const key = keyOf(sessionId, site.plugin);
    const open = this.#panels.get(key);
    if (open !== undefined && sameSite(open.site, site) && !open.view.webContents.isDestroyed())
      return open;
    if (open !== undefined) this.close(sessionId, site.plugin);
    const view = this.#views.create(site);
    const panel: OpenPanel = { sessionId, site, view, budget: new CallBudget() };
    this.#panels.set(key, panel);
    this.#byContents.set(view.webContents.id, panel);
    this.#wire(panel);
    this.#views.attach(view);
    view.setVisible(false);
    void view.webContents.loadURL(entryUrl(site)).catch(() => undefined);
    // A panel being written reloads itself when its files change.
    let quiet: ReturnType<typeof setTimeout> | undefined;
    const unwatch = this.#views.watch(site, () => {
      clearTimeout(quiet);
      quiet = setTimeout(() => {
        if (!view.webContents.isDestroyed()) view.webContents.reload();
      }, RELOAD_QUIET_MS);
    });
    panel.unwatch = () => {
      clearTimeout(quiet);
      unwatch();
    };
    return panel;
  }

  /** Lays the panel over its tab's area; every other panel is hidden. */
  show(sessionId: string, plugin: string, bounds: BrowserBounds, zoom: number): boolean {
    const panel = this.#panels.get(keyOf(sessionId, plugin));
    if (panel === undefined || panel.view.webContents.isDestroyed()) return false;
    for (const other of this.#panels.values()) if (other !== panel) other.view.setVisible(false);
    panel.view.setBounds(bounds);
    panel.view.webContents.setZoomFactor(zoom);
    panel.view.setVisible(true);
    return true;
  }

  hide(sessionId: string, plugin: string): void {
    this.#panels.get(keyOf(sessionId, plugin))?.view.setVisible(false);
  }

  get(sessionId: string, plugin: string): OpenPanel | undefined {
    return this.#panels.get(keyOf(sessionId, plugin));
  }

  close(sessionId: string, plugin: string): void {
    const key = keyOf(sessionId, plugin);
    const panel = this.#panels.get(key);
    if (panel === undefined) return;
    this.#panels.delete(key);
    this.#byContents.delete(panel.view.webContents.id);
    panel.unwatch?.();
    this.#views.detach(panel.view);
    if (!panel.view.webContents.isDestroyed()) panel.view.webContents.close();
    for (const listener of this.#closed) listener(panel);
  }

  /** The window's page started again: its tabs are gone, so are their panels. */
  reset(): void {
    for (const panel of [...this.#panels.values()]) this.close(panel.sessionId, panel.site.plugin);
  }

  /** Stops the panel and tells its tab why; the tab offers to open it again. */
  stop(panel: OpenPanel, why: string): void {
    if (this.#panels.get(keyOf(panel.sessionId, panel.site.plugin)) !== panel) return;
    this.close(panel.sessionId, panel.site.plugin);
    for (const listener of this.#listeners)
      listener({ sessionId: panel.sessionId, plugin: panel.site.plugin, stopped: why });
  }

  #wire(panel: OpenPanel): void {
    const contents = panel.view.webContents;
    const { plugin } = panel.site;
    contents.setWindowOpenHandler(() => {
      this.#problem(panel, '页面想打开新窗口，已拦下');
      return { action: 'deny' };
    });
    const stayHome = (event: { preventDefault(): void; url?: string }, url?: string) => {
      const target = event.url ?? url ?? '';
      if (sameOrigin(target, plugin)) return;
      event.preventDefault();
      this.#problem(panel, `页面想离开面板，跳到 ${target.slice(0, 200)}，已拦下`);
    };
    contents.on('will-navigate', stayHome);
    contents.on('will-redirect', stayHome);
    contents.on('will-attach-webview', (event: { preventDefault(): void }) =>
      event.preventDefault(),
    );
    contents.on('will-prevent-unload', (event: { preventDefault(): void }) =>
      event.preventDefault(),
    );
    contents.on(
      'did-fail-load',
      (_event: unknown, code: number, description: string, url: string, mainFrame: boolean) => {
        // -3 is a load the page itself replaced; not a failure.
        if (!mainFrame || code === -3) return;
        const why = `面板没有打开：${description}（${url.slice(0, 200)}）`;
        this.#problem(panel, why);
        this.stop(panel, why);
      },
    );
    contents.on('render-process-gone', (_event: unknown, details: { reason: string }) => {
      if (details.reason === 'clean-exit') return;
      const why = `面板停止运行了（${details.reason}）`;
      this.#problem(panel, why);
      this.stop(panel, why);
    });
    contents.on(
      'console-message',
      (details: { level: string; message: string; sourceId: string; lineNumber: number }) => {
        if (details.level !== 'error') return;
        const where =
          details.sourceId === '' ? '' : `（${details.sourceId}:${details.lineNumber}）`;
        this.#problem(panel, `${details.message.slice(0, 2_000)}${where}`);
      },
    );
  }
}
