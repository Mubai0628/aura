import { afterEach, describe, expect, it, vi } from 'vitest';

import type { BrowserPageState } from '../../../shared/browser-api';
import { resetToolTabsForTests, tabLabels, toolTabs } from '../right-workspace/tool-tabs';
import { browserStore } from './browser-store';
import { browserTabs, resetBrowserTabsForTests } from './browser-tabs';

const state = (
  sessionId: string,
  pageId: string,
  over: Partial<BrowserPageState> = {},
): BrowserPageState => ({
  sessionId,
  pageId,
  url: 'http://127.0.0.1:9/game',
  title: 'Game',
  loading: false,
  canGoBack: false,
  canGoForward: false,
  agentActive: true,
  userInControl: false,
  closed: false,
  ...over,
});

afterEach(() => {
  resetToolTabsForTests();
  resetBrowserTabsForTests();
});

describe("the model's page in the sidebar", () => {
  // One 浏览器 tab; the model's page is the first of its own pages.
  it('joins the browser the first time it opens in a conversation, shown with the sidebar', () => {
    const revealed: string[] = [];
    const stop = toolTabs.onReveal((session) => revealed.push(session));
    toolTabs.open('ses_a', 'terminal');
    browserStore.put(state('ses_a', 'model'));
    const tabs = toolTabs.get('ses_a');
    expect(tabLabels(tabs).map(({ label }) => label)).toEqual(['终端 1', '浏览器']);
    expect(tabs.tabs.find((tab) => tab.id === tabs.active)?.kind).toBe('browser');
    expect(browserTabs.get('ses_a')).toEqual({ pages: ['model'], shown: 'model' });
    expect(revealed).toEqual(['ses_a']);
    // Later states of the same page, and the user's own pages as they change, add nothing.
    toolTabs.activate('ses_a', tabs.tabs[0]!.id);
    browserStore.put(state('ses_a', 'model', { agentActive: false }));
    browserStore.put(state('ses_a', 'page-7'));
    expect(browserTabs.get('ses_a').pages).toEqual(['model']);
    expect(toolTabs.get('ses_a').active).toBe(tabs.tabs[0]!.id);
    expect(revealed).toEqual(['ses_a']);
    stop();
  });

  it("brings back the user's pages Main still holds when the window starts", async () => {
    vi.stubGlobal('aura', {
      v1: {
        browser: {
          command: vi.fn(),
          onState: vi.fn(() => () => undefined),
          pages: vi.fn(async () => [
            state('ses_b', 'page-1', { agentActive: false }),
            state('ses_b', 'page-2', { closed: true, notice: { kind: 'closed', text: 'x' } }),
          ]),
        },
      },
    });
    const stop = browserStore.subscribe(() => undefined);
    await vi.waitFor(() => expect(browserTabs.get('ses_b').pages).toEqual(['page-1']));
    expect(toolTabs.get('ses_b').tabs).toEqual([]);
    stop();
    vi.unstubAllGlobals();
  });
});
