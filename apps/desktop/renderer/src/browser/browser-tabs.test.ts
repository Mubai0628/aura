import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MODEL_PAGE_ID, type BrowserUserCommand } from '../../../shared/browser-api';
import { resetToolTabsForTests, tabLabels, toolTabs } from '../right-workspace/tool-tabs';
import {
  adoptModelPage,
  browserTabs,
  openInBrowser,
  resetBrowserTabsForTests,
  revealModelPage,
} from './browser-tabs';

// One 浏览器 tab in
// the sidebar; its pages, the model's first, are tabs of its own.

const S = 'ses_a';
let command: ReturnType<typeof vi.fn<(value: BrowserUserCommand) => Promise<unknown>>>;
beforeEach(() => {
  resetToolTabsForTests();
  resetBrowserTabsForTests();
  command = vi.fn(async () => ({ ok: true }));
  vi.stubGlobal('aura', { v1: { browser: { command } } });
});
afterEach(() => vi.unstubAllGlobals());

const sent = () => command.mock.calls.map(([value]) => value);
const activeKind = () => {
  const state = toolTabs.get(S);
  return state.tabs.find((tab) => tab.id === state.active)?.kind;
};

describe('the browser in the sidebar', () => {
  it('is one tab, and its pages open, show and close inside it', () => {
    toolTabs.open(S, 'browser');
    toolTabs.open(S, 'browser');
    expect(toolTabs.get(S).tabs.map((tab) => tab.kind)).toEqual(['browser']);
    const one = browserTabs.open(S);
    const two = browserTabs.open(S);
    const three = browserTabs.open(S);
    expect(browserTabs.get(S)).toEqual({ pages: [one, two, three], shown: three });
    browserTabs.show(S, two);
    browserTabs.close(S, two);
    expect(browserTabs.get(S)).toEqual({ pages: [one, three], shown: three });
    expect(sent()).toEqual([{ kind: 'close', sessionId: S, pageId: two }]);
    // A state of the closed page still on its way does not bring it back.
    expect(browserTabs.adopt(S, two)).toBe(false);
    browserTabs.closeAll(S);
    expect(browserTabs.get(S).pages).toEqual([]);
    expect(sent().map((value) => ('pageId' in value ? value.pageId : ''))).toEqual([
      two,
      one,
      three,
    ]);
  });

  it("puts the model's page first, shown with the sidebar the first time it opens", () => {
    const revealed: string[] = [];
    const stop = toolTabs.onReveal((session) => revealed.push(session));
    toolTabs.open(S, 'terminal');
    const mine = browserTabs.open(S);
    adoptModelPage(S);
    expect(browserTabs.get(S)).toEqual({ pages: [MODEL_PAGE_ID, mine], shown: MODEL_PAGE_ID });
    expect(tabLabels(toolTabs.get(S)).map((tab) => tab.label)).toEqual(['终端 1', '浏览器']);
    expect(activeKind()).toBe('browser');
    expect(revealed).toEqual([S]);
    browserTabs.show(S, mine);
    adoptModelPage(S);
    expect(browserTabs.get(S).shown).toBe(mine);
    expect(revealed).toEqual([S]);
    // Asked for from the conversation, it is shown.
    revealModelPage(S);
    expect(browserTabs.get(S).shown).toBe(MODEL_PAGE_ID);
    expect(revealed).toEqual([S, S]);
    // Closed, it comes back the next time the model opens it.
    browserTabs.close(S, MODEL_PAGE_ID);
    adoptModelPage(S);
    expect(browserTabs.get(S).pages).toEqual([MODEL_PAGE_ID, mine]);
    stop();
  });

  it('opens an address in a new page with the sidebar, and no page when Main refuses', async () => {
    await expect(openInBrowser(S, { address: 'example.com' })).resolves.toBeUndefined();
    const [page] = browserTabs.get(S).pages;
    expect(sent()).toEqual([
      { kind: 'open', sessionId: S, pageId: page },
      { kind: 'navigate', sessionId: S, pageId: page, input: 'example.com' },
    ]);
    expect(browserTabs.get(S).shown).toBe(page);
    expect(activeKind()).toBe('browser');
    command.mockImplementation(async (value) =>
      value.kind === 'open-file'
        ? { ok: false, message: '只能在内置浏览器中打开网页文件' }
        : { ok: true },
    );
    await expect(openInBrowser(S, { workspaceId: 'wsp_1', path: 'notes.txt' })).resolves.toBe(
      '只能在内置浏览器中打开网页文件',
    );
    expect(browserTabs.get(S).pages).toEqual([page]);
    const before = sent().at(-2);
    const refused = before !== undefined && 'pageId' in before ? before.pageId : '';
    expect(sent().at(-1)).toEqual({ kind: 'close', sessionId: S, pageId: refused });
  });
});
