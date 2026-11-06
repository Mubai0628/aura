/**
 * Plugin panels in Main. A panel is a WebContentsView in the Aura window, in a partition of its
 * plugin's own that serves only the plugin's `panel/` files and lets nothing else out but the
 * origins the plugin declared; no Node, no permissions, no downloads, no other windows. The
 * renderer places it; the page's own calls come through the panel preload and act only in the
 * conversation the panel was opened in. Core is asked what to show each time a tab opens.
 */
import { watch } from 'node:fs';
import { join } from 'node:path';
import {
  ipcMain,
  session,
  WebContentsView,
  type BrowserWindow,
  type IpcMainInvokeEvent,
  type Session,
} from 'electron';
import {
  PANEL_CALL_CAPABILITIES,
  PANEL_CAPABILITIES_ARGUMENT,
  PLUGIN_PANEL_CALL_CHANNEL,
  PLUGIN_PANEL_COMMAND_CHANNEL,
  PLUGIN_PANEL_STATE_CHANNEL,
  type PanelCallResult,
  type PluginPanelResult,
} from '../../shared/plugin-panel-api.js';
import { pagePicture, type PicturedContents } from '../browser-host/page-picture.js';
import type { CoreSupervisor } from '../core-supervisor.js';
import type { PluginUiHub } from '../plugin-ui/index.js';
import { coreCall, declared, readPanelCall, shownItem } from './calls.js';
import { readPanelCommand, readPanelSite, shownMessage } from './commands.js';
import { PluginPanels, type OpenPanel, type PanelView } from './panels.js';
import { PANEL_RECHECK_MS, recheckPanels } from './recheck.js';
import {
  PANEL_SCHEME,
  panelOrigin,
  panelResponse,
  requestAllowed,
  type PanelSite,
} from './site.js';

export { PANEL_SCHEME_PRIVILEGES } from './site.js';

export interface PluginPanelHostOptions {
  window(): BrowserWindow | undefined;
  assertTrustedSender(event: IpcMainInvokeEvent): void;
  core(): Pick<CoreSupervisor, 'requestWithBudget'> | undefined;
  /** The panel pages' own preload. */
  preload: string;
  /** Where the panels' notices and status entries are shown. */
  ui: PluginUiHub;
  log(event: string, fields: Record<string, unknown>): void;
}

export interface PluginPanelHost {
  /** The window's page started again: its tabs, and so their panels, are gone. */
  reset(): void;
}

const CORE_BUDGET_MS = 15_000;
/** How many problems one panel may add to its plugin's log in a minute. */
const PROBLEMS_PER_MINUTE = 10;

export function installPluginPanels(options: PluginPanelHostOptions): PluginPanelHost {
  const guarded = new Set<string>();
  const problems = new WeakMap<OpenPanel, number[]>();
  const ask = async (method: string, params: Record<string, unknown>): Promise<unknown> => {
    const core = options.core();
    if (core === undefined) throw new Error('plugin_panel.core_unavailable: Aura Core 还没有就绪');
    return core.requestWithBudget(method, params, CORE_BUDGET_MS);
  };
  // What a panel's page did wrong goes to its plugin's log, where `plugins.logs` reads it.
  const problem = (panel: OpenPanel, text: string) => {
    const now = Date.now();
    const recent = (problems.get(panel) ?? []).filter((at) => now - at < 60_000);
    if (recent.length >= PROBLEMS_PER_MINUTE) return;
    problems.set(panel, [...recent, now]);
    const params = { plugin: panel.site.plugin, session_id: panel.sessionId, text };
    void ask('agent.plugin_session.log', params).catch(() => undefined);
  };
  const guard = (plugin: string): Session => {
    const partition = `aura-plugin-panel-${plugin}`;
    const own = session.fromPartition(partition, { cache: false });
    if (guarded.has(partition)) return own;
    guarded.add(partition);
    own.setPermissionRequestHandler((_contents, _permission, done) => done(false));
    own.setPermissionCheckHandler(() => false);
    own.setDevicePermissionHandler(() => false);
    own.on('will-download', (event) => event.preventDefault());
    own.webRequest.onBeforeRequest((details, done) => {
      const site = panels.site(plugin);
      const allowed = site !== undefined && requestAllowed(details.url, site);
      if (!allowed) {
        const panel = panels.byContents(details.webContentsId ?? -1);
        if (panel !== undefined)
          problem(panel, `拦下了没在 [panel] network 里声明的地址：${details.url.slice(0, 200)}`);
      }
      done({ cancel: !allowed });
    });
    own.protocol.handle(PANEL_SCHEME, (request) => panelResponse(request, panels.site(plugin)));
    return own;
  };
  const panels = new PluginPanels(
    {
      create: (site: PanelSite) => {
        const view = new WebContentsView({
          webPreferences: {
            session: guard(site.plugin),
            preload: options.preload,
            sandbox: true,
            contextIsolation: true,
            nodeIntegration: false,
            nodeIntegrationInSubFrames: false,
            nodeIntegrationInWorker: false,
            webSecurity: true,
            allowRunningInsecureContent: false,
            webviewTag: false,
            navigateOnDragDrop: false,
            spellcheck: false,
            safeDialogs: true,
            additionalArguments: [`${PANEL_CAPABILITIES_ARGUMENT}${site.capabilities.join(',')}`],
          },
        });
        // The page draws its own background over the sidebar's.
        view.setBackgroundColor('#00000000');
        return view as unknown as PanelView;
      },
      attach: (view) =>
        options.window()?.contentView.addChildView(view as unknown as WebContentsView),
      detach: (view) => {
        // After a renderer crash the window is a new one, without the old window's views.
        try {
          options.window()?.contentView.removeChildView(view as unknown as WebContentsView);
        } catch {
          /* already gone with its window */
        }
      },
      watch: (site, changed) => {
        try {
          const watcher = watch(join(site.folder, 'panel'), { recursive: true }, () => changed());
          watcher.on('error', () => watcher.close());
          return () => watcher.close();
        } catch {
          return () => undefined;
        }
      },
    },
    problem,
  );
  panels.onState((state) => {
    const contents = options.window()?.webContents;
    if (contents !== undefined && !contents.isDestroyed())
      contents.send(PLUGIN_PANEL_STATE_CHANNEL, state);
  });

  const zoom = () => options.window()?.webContents.getZoomFactor() ?? 1;
  ipcMain.handle(
    PLUGIN_PANEL_COMMAND_CHANNEL,
    async (event, value: unknown): Promise<PluginPanelResult> => {
      options.assertTrustedSender(event);
      const command = readPanelCommand(value);
      if (command === undefined) return { ok: false, message: '无效的插件面板命令' };
      const { sessionId, plugin } = command;
      switch (command.kind) {
        case 'open':
          try {
            const answer = await ask('agent.plugin_session.panel', {
              plugin,
              session_id: sessionId,
            });
            const site = readPanelSite(answer, plugin);
            if (site === undefined) return { ok: false, message: '插件面板的信息不对，没有打开' };
            panels.open(sessionId, site);
            return { ok: true };
          } catch (error) {
            // A panel Core no longer offers does not stay open.
            panels.close(sessionId, plugin);
            return { ok: false, message: shownMessage(error, '插件面板没有打开') };
          }
        case 'show': {
          const scale = zoom();
          const { x, y, width, height } = command.bounds;
          const shown = panels.show(
            sessionId,
            plugin,
            {
              x: Math.round(x * scale),
              y: Math.round(y * scale),
              width: Math.max(1, Math.round(width * scale)),
              height: Math.max(1, Math.round(height * scale)),
            },
            scale,
          );
          return shown ? { ok: true } : { ok: false, message: '插件面板已关闭' };
        }
        case 'hide':
          panels.hide(sessionId, plugin);
          return { ok: true };
        case 'close':
          panels.close(sessionId, plugin);
          return { ok: true };
        case 'picture':
          return {
            ok: true,
            picture: await pagePicture(
              panels.get(sessionId, plugin)?.view.webContents as unknown as
                PicturedContents | undefined,
            ),
          };
      }
    },
  );

  ipcMain.handle(
    PLUGIN_PANEL_CALL_CHANNEL,
    async (event, value: unknown): Promise<PanelCallResult> => {
      const panel = panels.byContents(event.sender.id);
      const frame = event.senderFrame;
      // Only a panel page Main made, from its own top frame on its own origin.
      if (
        panel === undefined ||
        frame === null ||
        frame !== event.sender.mainFrame ||
        !frame.url.startsWith(`${panelOrigin(panel.site.plugin)}/`)
      )
        return { ok: false, message: 'plugin_panel.refused: 只有 Aura 打开的插件面板能调用' };
      const call = readPanelCall(value);
      if (call === undefined)
        return { ok: false, message: 'plugin_panel.invalid: 调用的内容不对，见写插件的 Skill' };
      if (!declared(call, panel.site.capabilities))
        return {
          ok: false,
          message: `plugin_session.capability_missing: 插件 ${panel.site.plugin} 没有声明 ${PANEL_CALL_CAPABILITIES[call.op]}`,
        };
      if (!panel.budget.take(call.op))
        return { ok: false, message: 'plugin_panel.too_many_calls: 调用太频繁，稍后再试' };
      const { plugin } = panel.site;
      if (call.op === 'notify' && !options.ui.noticeAllowed(plugin))
        return {
          ok: false,
          message: 'plugin_panel.too_many_notices: 提示太多了，10 秒内最多 3 条',
        };
      const { method, params } = coreCall(call, plugin, panel.sessionId);
      try {
        const answer = await ask(method, params);
        // What Core checked is shown; the page gets back what was shown.
        const item =
          call.op === 'notify' || call.op === 'status' ? shownItem(answer, plugin) : undefined;
        if (call.op === 'notify' && item?.text !== undefined)
          options.ui.notice({ plugin, text: item.text, tone: item.tone });
        if (call.op === 'status' && item !== undefined)
          options.ui.status(plugin, panelSource(panel), item.text, item.tone);
        return { ok: true, value: answer };
      } catch (error) {
        const message = callMessage(error);
        // The plugin can no longer be used: its panel stops, and its tab says why.
        if (/^plugin_session\.(?:not_ready|plugin_missing)\b/u.test(message))
          panels.stop(panel, shownMessage(error, '插件现在不能用'));
        return { ok: false, message };
      }
    },
  );

  // A panel's status entry goes with it.
  panels.onClosed((panel) => options.ui.drop(panel.site.plugin, panelSource(panel)));
  let rechecking = false;
  const recheck = setInterval(() => {
    if (rechecking || panels.list().length === 0) return;
    rechecking = true;
    void recheckPanels(panels, ask).finally(() => {
      rechecking = false;
    });
  }, PANEL_RECHECK_MS);
  recheck.unref?.();

  options.log('plugin_panels.installed', {});
  return { reset: () => panels.reset() };
}

/** Where a panel's status entry comes from: the panel in its conversation. */
function panelSource(panel: OpenPanel): string {
  return `panel:${panel.sessionId}`;
}

/** Core's refusal as the page reads it: with its code, so the page can act on it. */
function callMessage(error: unknown): string {
  const envelope =
    typeof error === 'object' && error !== null && 'envelope' in error
      ? (error as { envelope?: { message?: unknown } }).envelope
      : undefined;
  if (typeof envelope?.message === 'string') return envelope.message.slice(0, 2_000);
  if (error instanceof Error && /^plugin_panel\./u.test(error.message)) return error.message;
  return 'plugin_panel.failed: Aura 没有完成这次调用';
}
