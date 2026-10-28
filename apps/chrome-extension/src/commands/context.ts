/** What a command needs from the extension around it; tests pass fakes. */
import type { Tab, WindowsApi } from '../chrome-api.js';
import type { Pages } from '../page/pages.js';
import { CommandError, type AssistOutcome } from '../protocol.js';

export interface CommandContext {
  tabs: {
    get(tabId: number): Promise<Tab>;
    create(properties: { url: string; active?: boolean }): Promise<Tab>;
    remove(tabId: number): Promise<void>;
  };
  windows: WindowsApi;
  pages: Pages;
  /** Puts a tab Aura opened into its window's "Aura" tab group. */
  group(tab: Tab): Promise<void>;
  /** Hands the tab to the user and waits for how that ends. */
  assist(tabId: number, reason: string, timeoutSeconds: number): Promise<AssistOutcome>;
  /** Aborted when Core no longer wants the command. */
  signal: AbortSignal;
  sleep(ms: number): Promise<void>;
  now(): number;
}

/** A command that did nothing: Core records that it changed nothing. */
export const refused = (message: string) => new CommandError('chrome.refused', message);
/** A command that failed partway: Core records that it may have acted. */
export const failed = (message: string) => new CommandError('chrome.failed', message);

export function stopIfCancelled(context: CommandContext): void {
  if (context.signal.aborted) throw new CommandError('chrome.cancelled', '这一步已取消。');
}

/** Waits for the tab to stop loading, at most `limit`; a short quiet moment either way. */
export async function settle(
  context: CommandContext,
  tabId: number,
  limit: number,
): Promise<boolean> {
  const end = context.now() + limit;
  await context.sleep(120);
  let tab = await context.tabs.get(tabId);
  while (tab.status !== 'complete' && context.now() < end && !context.signal.aborted) {
    await context.sleep(100);
    tab = await context.tabs.get(tabId);
  }
  await context.sleep(150);
  return tab.status === 'complete';
}
