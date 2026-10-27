import { describe, expect, it } from 'vitest';
import type { Tab } from './chrome-api.js';
import type { ExtensionMessage, ShareView } from './protocol.js';
import { SharedTabs } from './shares.js';

const tab = (id: number, windowId = 1): Tab => ({
  id,
  windowId,
  url: `https://site${id}.example/`,
  title: `Tab ${id}`,
  active: false,
  groupId: -1,
});

const share = (tab_id: number): ShareView => ({
  tab_id,
  session_id: 'ses_one',
  session_title: 'Docs',
  origin: `https://site${tab_id}.example`,
  state: 'active',
  scope: 'session',
});

function harness(existing: number[]) {
  const sent: ExtensionMessage[] = [];
  const grouped: { tabIds: number[]; groupId?: number }[] = [];
  const ungrouped: number[][] = [];
  const forgotten: number[] = [];
  const groups: { id: number; windowId: number }[] = [];
  const shared = new SharedTabs({
    tabs: {
      get: async (id) => {
        if (!existing.includes(id)) throw new Error('No tab with id');
        return tab(id);
      },
      group: async (options) => {
        grouped.push(options);
        return options.groupId ?? 50;
      },
      ungroup: async (ids) => void ungrouped.push(ids),
    },
    tabGroups: {
      query: async () => groups,
      update: async (id, properties) => {
        groups.push({ id, windowId: 1 });
        return properties;
      },
    },
    pages: { forget: async (id) => void forgotten.push(id) },
    send: (message) => {
      sent.push(message);
      return true;
    },
  });
  return { shared, sent, grouped, ungrouped, forgotten };
}

describe('the shared tabs', () => {
  it('sit in one "Aura" group per window, and leave it when the share ends', async () => {
    const h = harness([7, 8]);
    await h.shared.update([share(7), share(8)]);
    expect(h.grouped).toEqual([{ tabIds: [7] }, { tabIds: [8], groupId: 50 }]);
    await h.shared.update([share(8)]);
    expect(h.ungrouped).toEqual([[7]]);
    expect(h.forgotten).toEqual([7]);
  });

  it('report a tab Core still lists but Chrome no longer has', async () => {
    const h = harness([]);
    await h.shared.update([share(9)]);
    expect(h.sent).toEqual([{ type: 'tab_closed', tab_id: 9 }]);
  });

  it('tell Core when a shared tab moves, retitles or closes, and nothing about other tabs', async () => {
    const h = harness([7]);
    await h.shared.update([share(7)]);
    h.shared.tabChanged(
      7,
      { url: 'https://pay.example/' },
      { ...tab(7), url: 'https://pay.example/' },
    );
    h.shared.tabChanged(7, { status: 'loading' } as { url?: string }, tab(7));
    h.shared.tabChanged(3, { title: 'Elsewhere' }, tab(3));
    await h.shared.tabRemoved(7);
    await h.shared.tabRemoved(3);
    expect(h.sent).toEqual([
      { type: 'tab_changed', tab_id: 7, url: 'https://pay.example/', title: 'Tab 7' },
      { type: 'tab_closed', tab_id: 7 },
    ]);
  });
});
