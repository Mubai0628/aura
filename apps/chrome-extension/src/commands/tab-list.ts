/**
 * The user's ordinary tabs, for Aura's @ menu: their ids, titles and addresses, read when the user
 * opens the menu. Incognito windows, browser pages and store pages are never listed; no page is
 * read or debugged to list them.
 */
import type { TabsReport } from '../protocol.js';
import { shareable } from '../shareable.js';
import type { CommandContext } from './context.js';

export async function listTabs(context: CommandContext): Promise<TabsReport> {
  const tabs = await context.tabs.query({});
  return {
    tabs: tabs.flatMap((tab) =>
      tab.id === undefined || tab.incognito === true || !shareable(tab.url ?? '')
        ? []
        : [{ tab_id: tab.id, window_id: tab.windowId, url: tab.url ?? '', title: tab.title ?? '' }],
    ),
  };
}
