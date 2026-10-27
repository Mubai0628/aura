/**
 * The tabs shared with Aura, as Core last listed them. Shared tabs sit in their window's "Aura"
 * tab group; a tab that moves, retitles or closes is reported to Core, and a tab Core no longer
 * lists leaves the group and is no longer debugged.
 */
import type { Tab } from './chrome-api.js';
import type { Pages } from './page/pages.js';
import type { ExtensionMessage, ShareView } from './protocol.js';

export const GROUP_TITLE = 'Aura';

export interface ShareDeps {
  tabs: {
    get(tabId: number): Promise<Tab>;
    group(options: { tabIds: number[]; groupId?: number }): Promise<number>;
    ungroup(tabIds: number[]): Promise<void>;
  };
  tabGroups: {
    query(info: { title?: string; windowId?: number }): Promise<{ id: number; windowId: number }[]>;
    update(groupId: number, properties: { title?: string; color?: string }): Promise<unknown>;
  };
  pages: Pick<Pages, 'forget'>;
  send(message: ExtensionMessage): boolean;
}

export class SharedTabs {
  readonly #deps: ShareDeps;
  #shares: ShareView[] = [];
  readonly #grouped = new Set<number>();

  constructor(deps: ShareDeps) {
    this.#deps = deps;
  }

  get shares(): readonly ShareView[] {
    return this.#shares;
  }

  isShared(tabId: number): boolean {
    return this.#shares.some((share) => share.tab_id === tabId);
  }

  /** Core's list, sent after every change and on every connection. */
  async update(shares: ShareView[]): Promise<void> {
    const listed = new Set(shares.map((share) => share.tab_id));
    const ended = this.#shares.filter((share) => !listed.has(share.tab_id));
    this.#shares = shares;
    for (const share of ended) await this.#leave(share.tab_id);
    for (const share of shares) {
      const tab = await this.#deps.tabs.get(share.tab_id).catch(() => null);
      // Chrome restarted or the tab closed while Aura was away.
      if (tab === null) {
        this.#deps.send({ type: 'tab_closed', tab_id: share.tab_id });
        continue;
      }
      if (!this.#grouped.has(share.tab_id)) await this.group(tab);
    }
  }

  /** Puts a tab into its window's "Aura" group. */
  async group(tab: Tab): Promise<void> {
    if (tab.id === undefined) return;
    const [existing] = await this.#deps.tabGroups.query({
      title: GROUP_TITLE,
      windowId: tab.windowId,
    });
    const groupId = await this.#deps.tabs.group(
      existing === undefined ? { tabIds: [tab.id] } : { tabIds: [tab.id], groupId: existing.id },
    );
    if (existing === undefined)
      await this.#deps.tabGroups.update(groupId, { title: GROUP_TITLE, color: 'blue' });
    this.#grouped.add(tab.id);
  }

  tabChanged(tabId: number, change: { url?: string; title?: string }, tab: Tab): void {
    if (!this.isShared(tabId) || (change.url === undefined && change.title === undefined)) return;
    this.#deps.send({
      type: 'tab_changed',
      tab_id: tabId,
      url: tab.url ?? change.url ?? '',
      title: tab.title ?? change.title ?? '',
    });
  }

  async tabRemoved(tabId: number): Promise<void> {
    this.#grouped.delete(tabId);
    await this.#deps.pages.forget(tabId);
    if (this.isShared(tabId)) this.#deps.send({ type: 'tab_closed', tab_id: tabId });
  }

  async #leave(tabId: number): Promise<void> {
    await this.#deps.pages.forget(tabId);
    if (!this.#grouped.delete(tabId)) return;
    await this.#deps.tabs.ungroup([tabId]).catch(() => undefined);
  }
}
