/**
 * An open panel is asked about again: a plugin switched off, untrusted, changed or uninstalled,
 * or a conversation deleted, stops its panels even while they are hidden and call nothing. A
 * panel Core now describes differently stops too and opens again as it is now.
 */
import { readPanelSite, shownMessage } from './commands.js';
import { sameSite, type PluginPanels } from './panels.js';

/** How often the open panels are asked about. */
export const PANEL_RECHECK_MS = 5_000;

/** Core's answers that mean the panel cannot be shown now. */
const GONE =
  /^plugin_session\.(?:not_ready|plugin_missing|panel_missing|panel_unusable|session_missing)\b/u;

export type CoreAsk = (method: string, params: Record<string, unknown>) => Promise<unknown>;

/** Core's refusal with its code, as Main received it. */
export function refusalOf(error: unknown): string {
  const envelope =
    typeof error === 'object' && error !== null && 'envelope' in error
      ? (error as { envelope?: { message?: unknown } }).envelope
      : undefined;
  if (typeof envelope?.message === 'string') return envelope.message;
  return error instanceof Error ? error.message : '';
}

export async function recheckPanels(
  panels: Pick<PluginPanels, 'list' | 'stop'>,
  ask: CoreAsk,
): Promise<void> {
  await Promise.all(
    panels.list().map(async (panel) => {
      try {
        const answer = await ask('agent.plugin_session.panel', {
          plugin: panel.site.plugin,
          session_id: panel.sessionId,
        });
        const site = readPanelSite(answer, panel.site.plugin);
        if (site !== undefined && !sameSite(site, panel.site))
          panels.stop(panel, '插件的面板改过了，重新打开就是改后的');
      } catch (error) {
        // Core busy or restarting says nothing about the plugin; the next round asks again.
        if (GONE.test(refusalOf(error))) panels.stop(panel, shownMessage(error, '插件现在不能用'));
      }
    }),
  );
}
