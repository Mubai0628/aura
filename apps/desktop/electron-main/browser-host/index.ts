/**
 * Aura's built-in browser in Main. Pages are WebContentsViews in the Aura window under one
 * persistent profile; the renderer shows and drives the user's view of them, and Core (B2)
 * drives a session's model page. The settings come from Core and are read again after a save.
 */
import {
  BrowserWindow,
  ipcMain,
  session,
  WebContentsView,
  type IpcMainInvokeEvent,
} from 'electron';
import {
  BROWSER_COMMAND_CHANNEL,
  BROWSER_PAGES_CHANNEL,
  BROWSER_STATE_CHANNEL,
  MODEL_PAGE_ID,
  type BrowserCommandResult,
  type BrowserUserCommand,
} from '../../shared/browser-api.js';
import type { CoreSupervisor } from '../core-supervisor.js';
import { parseUserCommand } from './commands.js';
import { DevToolsWindows, type DevToolsHostWindow, type DevToolsPage } from './devtools-window.js';
import { modelPages } from './model-pages.js';
import { BrowserPages, type PageView, type PageViews } from './pages.js';
import { BROWSER_PARTITION, guardProfile, type ProfileSession } from './profile.js';
import { parsedRules } from './site-rules.js';
import { openWorkspacePage } from './workspace-page.js';

export interface BrowserHostOptions {
  window(): BrowserWindow | undefined;
  assertTrustedSender(event: IpcMainInvokeEvent): void;
  core():
    | Pick<CoreSupervisor, 'requestAgent' | 'requestWithBudget' | 'refreshWorkspaceSessionSnapshot'>
    | undefined;
  log(event: string, fields: Record<string, unknown>): void;
}

export interface BrowserHost {
  readonly pages: BrowserPages;
  /** A `browser.settings.*` result passing through Main. */
  applySettings(snapshot: unknown): void;
  /** Core has become ready: load rules before any page is opened. */
  refreshSettings(): Promise<void>;
  /** Takes Core's commands for the model's pages from this supervisor's connection. */
  bindCore(supervisor: object): void;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const strings = (value: unknown): string[] | undefined =>
  Array.isArray(value) && value.every((item) => typeof item === 'string') ? value : undefined;

function createView(): PageView {
  const view = new WebContentsView({
    webPreferences: {
      partition: BROWSER_PARTITION,
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      nodeIntegrationInSubFrames: false,
      nodeIntegrationInWorker: false,
      webSecurity: true,
      allowRunningInsecureContent: false,
      // A page off screen keeps running (a game loop, a timer the model waits for).
      backgroundThrottling: false,
      spellcheck: false,
      navigateOnDragDrop: false,
      safeDialogs: true,
      webviewTag: false,
    },
  });
  view.setBackgroundColor('#ffffff');
  return view as unknown as PageView;
}

export function installBrowserHost(options: BrowserHostOptions): BrowserHost {
  const views: PageViews = {
    create: createView,
    attach: (view) =>
      options.window()?.contentView.addChildView(view as unknown as WebContentsView),
    detach: (view) =>
      options.window()?.contentView.removeChildView(view as unknown as WebContentsView),
  };
  const pages = new BrowserPages(views);
  const devtools = new DevToolsWindows((title) => {
    const window = new BrowserWindow({
      width: 1100,
      height: 760,
      title,
      show: false,
      autoHideMenuBar: true,
      backgroundColor: '#ffffff',
      webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true },
    });
    // The DevTools page names itself "DevTools"; the window keeps saying whose they are.
    window.on('page-title-updated', (event) => event.preventDefault());
    return window as unknown as DevToolsHostWindow;
  });
  guardProfile(session.fromPartition(BROWSER_PARTITION) as unknown as ProfileSession, pages);
  pages.onState((state) => {
    const contents = options.window()?.webContents;
    if (contents !== undefined && !contents.isDestroyed())
      contents.send(BROWSER_STATE_CHANNEL, state);
  });

  let appliedSettings = 0;
  let refreshing: Promise<void> | undefined;
  const applySettings = (snapshot: unknown): void => {
    if (!isRecord(snapshot) || !isRecord(snapshot.settings)) return;
    const {
      enabled,
      allowed_sites: allowed,
      blocked_sites: blocked,
      debugging,
    } = snapshot.settings;
    const allowedSites = strings(allowed);
    const blockedSites = strings(blocked);
    if (typeof enabled !== 'boolean' || allowedSites === undefined || blockedSites === undefined)
      return;
    appliedSettings += 1;
    pages.setPolicy({
      enabled: enabled && snapshot.issue === null,
      allowed: parsedRules(allowedSites),
      blocked: parsedRules(blockedSites),
      debugging: debugging === true && snapshot.issue === null,
    });
  };
  const refresh = (): Promise<void> => {
    if (refreshing !== undefined) return refreshing;
    const before = appliedSettings;
    const pending = (async () => {
      try {
        const core = options.core();
        if (core === undefined) throw new Error('Aura Core is not ready');
        const snapshot = await core.requestAgent('browser.settings.get', {});
        // A settings save that arrived while this read waited is newer than its answer.
        if (before === appliedSettings) applySettings(snapshot);
      } catch {
        if (before === appliedSettings)
          pages.setPolicy({ enabled: false, allowed: [], blocked: [] });
      }
    })();
    refreshing = pending;
    void pending.then(() => {
      if (refreshing === pending) refreshing = undefined;
    });
    return pending;
  };

  // Results go back on Core's Main-only route through the supervisor's one generic request; its
  // file is at its size ceiling, so the browser adds no method of its own there.
  const models = modelPages(pages, options.core, options.log, refresh, () => {
    const contents = options.window()?.webContents;
    if (contents !== undefined && !contents.isDestroyed()) contents.focus();
  });
  const zoom = () => options.window()?.webContents.getZoomFactor() ?? 1;
  const run = async (command: BrowserUserCommand): Promise<BrowserCommandResult> => {
    if (command.kind === 'clear-data') {
      const profile = session.fromPartition(BROWSER_PARTITION);
      await Promise.all([profile.clearStorageData(), profile.clearCache()]);
      return { ok: true };
    }
    const { sessionId, pageId } = command;
    switch (command.kind) {
      case 'open':
        await refresh();
        if (!pages.policy.enabled) return { ok: false, message: '内置浏览器已在设置中关闭' };
        return { ok: true, state: pages.open(sessionId, pageId) };
      case 'show': {
        if (!pages.policy.enabled) return { ok: false, message: '内置浏览器已在设置中关闭' };
        const scale = zoom();
        const { x, y, width, height } = command.bounds;
        return {
          ok: true,
          state: pages.show(sessionId, pageId, {
            x: Math.round(x * scale),
            y: Math.round(y * scale),
            width: Math.max(1, Math.round(width * scale)),
            height: Math.max(1, Math.round(height * scale)),
          }),
        };
      }
      case 'open-file':
        await refresh();
        if (!pages.policy.enabled) return { ok: false, message: '内置浏览器已在设置中关闭' };
        return openWorkspacePage(pages, options.core(), command);
      case 'hide':
        pages.hide(sessionId, pageId);
        return { ok: true };
      case 'navigate': {
        const refused = pages.navigate(sessionId, pageId, command.input);
        return refused === undefined ? { ok: true } : { ok: false, message: refused };
      }
      case 'back':
      case 'forward':
      case 'reload':
      case 'stop':
        pages.history(sessionId, pageId, command.kind);
        return { ok: true };
      case 'close':
        pages.close(sessionId, pageId);
        return { ok: true };
      case 'take-control':
      case 'release-control':
        pages.setUserControl(sessionId, pageId, command.kind === 'take-control');
        // Handing the page back answers the model's request that waits for it.
        if (command.kind === 'release-control' && pageId === MODEL_PAGE_ID)
          models.answerAssist(sessionId, 'done');
        return { ok: true };
      case 'dismiss-notice':
        pages.dismissNotice(sessionId, pageId);
        return { ok: true };
      case 'devtools': {
        // The page's own DevTools, for the user; only an open page of
        // Aura's browser, never Aura's own window. In a window of their own.
        const contents = pages.contents(sessionId, pageId);
        if (contents === undefined || contents.isDestroyed())
          return { ok: false, message: '这个页面已关闭' };
        devtools.open(
          contents as unknown as DevToolsPage,
          options.window() as unknown as DevToolsHostWindow | undefined,
        );
        return { ok: true };
      }
      case 'assist-done':
      case 'assist-decline':
        return models.answerAssist(sessionId, command.kind === 'assist-done' ? 'done' : 'declined')
          ? { ok: true }
          : { ok: false, message: '模型已不再等待' };
    }
  };

  ipcMain.handle(BROWSER_COMMAND_CHANNEL, async (event, value: unknown) => {
    options.assertTrustedSender(event);
    const command = parseUserCommand(value);
    return command === undefined ? { ok: false, message: '无效的浏览器命令' } : run(command);
  });
  ipcMain.handle(BROWSER_PAGES_CHANNEL, (event) => {
    options.assertTrustedSender(event);
    return pages.states();
  });
  return { pages, applySettings, refreshSettings: refresh, bindCore: models.bind };
}
