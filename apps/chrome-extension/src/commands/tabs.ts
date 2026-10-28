/** Opening a tab for the conversation, moving a shared tab, closing one Aura opened, and handing a
 * tab to the user. */
import type {
  AssistReport,
  CloseReport,
  HistoryAction,
  OpenReport,
  PageReport,
} from '../protocol.js';
import { failed, settle, type CommandContext } from './context.js';
import { report, SNAPSHOT_CHARS } from './look.js';

const STILL_LOADING = '页面仍在加载，下面是当前的样子';

/** A new tab in the background: what the user is looking at does not change. */
export async function open(context: CommandContext, url: string): Promise<OpenReport> {
  const created = await context.tabs.create({ url, active: false });
  if (created.id === undefined) throw failed('Chrome 没有打开新标签页');
  await context.group(created);
  const loaded = await settle(context, created.id, 20_000);
  const page = await report(context, created.id, SNAPSHOT_CHARS, loaded ? null : STILL_LOADING);
  const tab = await context.tabs.get(created.id);
  return {
    tab: {
      tab_id: created.id,
      window_id: tab.windowId,
      url: tab.url ?? url,
      title: tab.title ?? '',
    },
    page,
  };
}

interface History {
  currentIndex?: number;
  entries?: { id: number }[];
}

export async function navigate(
  context: CommandContext,
  tabId: number,
  url: string | null,
  action: HistoryAction | null,
): Promise<PageReport> {
  const page = await context.pages.page(tabId);
  let notice: string | null = null;
  if (url !== null) {
    const { errorText } = await page.send<{ errorText?: string }>('Page.navigate', { url });
    if (errorText) notice = `打不开 ${url}：${errorText}`;
  } else if (action === 'reload') {
    await page.send('Page.reload');
  } else {
    const { currentIndex = 0, entries = [] } = await page.send<History>(
      'Page.getNavigationHistory',
    );
    const target = entries[currentIndex + (action === 'back' ? -1 : 1)];
    if (target === undefined)
      return report(
        context,
        tabId,
        SNAPSHOT_CHARS,
        action === 'back' ? '没有可以后退的页面' : '没有可以前进的页面',
      );
    await page.send('Page.navigateToHistoryEntry', { entryId: target.id });
  }
  const loaded = await settle(context, tabId, 20_000);
  return report(context, tabId, SNAPSHOT_CHARS, notice ?? (loaded ? null : STILL_LOADING));
}

export async function close(context: CommandContext, tabId: number): Promise<CloseReport> {
  await context.pages.forget(tabId);
  await context.tabs.remove(tabId);
  return { closed: true };
}

export async function assist(
  context: CommandContext,
  tabId: number,
  reason: string,
  timeoutSeconds: number,
): Promise<AssistReport> {
  // The tab is the user's meanwhile: Aura leaves it, and Chrome's debugging bar goes.
  await context.pages.pause(tabId);
  const outcome = await context.assist(tabId, reason, timeoutSeconds);
  if (outcome === 'closed') return { outcome, page: null };
  const page = await context.pages.page(tabId);
  // The user may have changed anything: refs from before name nothing now.
  page.refs.clear();
  await settle(context, tabId, 5_000);
  return { outcome, page: await report(context, tabId) };
}
