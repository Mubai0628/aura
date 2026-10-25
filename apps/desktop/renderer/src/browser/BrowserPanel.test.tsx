import { act, cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { BrowserPageState, BrowserUserCommand } from '../../../shared/browser-api';
import { resetToolTabsForTests } from '../right-workspace/tool-tabs';
import { BrowserPanel } from './BrowserPanel';
import { browserStore } from './browser-store';
import { browserTabs, resetBrowserTabsForTests } from './browser-tabs';

// One browser with its own row of pages — the model's first — a new page and
// a close button on each, a start page when none is open, and a button to fill the main area.

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const page = (over: Partial<BrowserPageState> = {}): BrowserPageState => ({
  sessionId: S,
  pageId: 'page-1',
  url: 'https://example.com/',
  title: 'Example',
  loading: false,
  canGoBack: false,
  canGoForward: false,
  agentActive: false,
  userInControl: false,
  closed: false,
  ...over,
});

let command: ReturnType<typeof vi.fn<(value: BrowserUserCommand) => Promise<unknown>>>;
beforeEach(() => {
  resetToolTabsForTests();
  resetBrowserTabsForTests();
  // A page Main opens starts blank; an address loads the example page.
  const loaded = new Set<string>();
  command = vi.fn(async (value: BrowserUserCommand) => {
    if (value.kind === 'navigate') {
      loaded.add(value.pageId);
      browserStore.put(page({ pageId: value.pageId }));
    }
    return value.kind === 'open'
      ? {
          ok: true,
          state: loaded.has(value.pageId)
            ? page({ pageId: value.pageId })
            : page({ pageId: value.pageId, url: 'about:blank', title: '' }),
        }
      : { ok: true };
  });
  vi.stubGlobal('aura', {
    v1: {
      browser: {
        command,
        pages: vi.fn(async () => []),
        onState: vi.fn(() => () => undefined),
      },
    },
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const tabs = () =>
  within(screen.getByRole('tablist', { name: '浏览器页面' }))
    .queryAllByRole('tab')
    .map((tab) => tab.textContent);

describe('the browser', () => {
  it('shows the start page with no page open; an address opens one', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel sessionId={S} visible />);
    expect(tabs()).toEqual([]);
    await user.type(screen.getByLabelText('打开网址'), 'example.com{Enter}');
    await vi.waitFor(() => expect(tabs()).toEqual(['Example']));
    expect(command.mock.calls.map(([value]) => value.kind).slice(0, 2)).toEqual([
      'open',
      'navigate',
    ]);
  });

  it('opens a new page with the keyboard on its start page, and closes a page with its ×', async () => {
    const user = userEvent.setup();
    render(<BrowserPanel sessionId={S} visible />);
    await user.click(screen.getByRole('button', { name: '新建页面' }));
    const field = await screen.findByLabelText('打开网址');
    expect(document.activeElement).toBe(field);
    expect(tabs()).toEqual(['新标签页']);
    const [opened] = browserTabs.get(S).pages;
    await user.click(screen.getByRole('button', { name: '关闭 新标签页' }));
    expect(command).toHaveBeenCalledWith({ kind: 'close', sessionId: S, pageId: opened });
    expect(tabs()).toEqual([]);
    // With no page left the browser shows its start page again, the keyboard left alone.
    expect(document.activeElement).not.toBe(screen.getByLabelText('打开网址'));
  });

  it("keeps the model's page first, marked as the model's", async () => {
    render(<BrowserPanel sessionId={S} visible />);
    browserTabs.open(S);
    act(() => browserStore.put(page({ pageId: 'model', title: 'Game', agentActive: true })));
    const row = within(screen.getByRole('tablist', { name: '浏览器页面' })).getAllByRole('tab');
    expect(row[0]?.getAttribute('aria-label')).toBe('模型的页面：Game');
    expect(row[0]?.getAttribute('aria-selected')).toBe('true');
    expect(await screen.findByText('Aura 正在操作')).toBeTruthy();
  });

  // Past
  // what the row can hold its pages scroll sideways under the wheel, the edges fade where pages
  // are hidden, and the page shown is kept in view.
  it('scrolls a row of many pages sideways and keeps the page shown in view', async () => {
    const original = Element.prototype.scrollIntoView;
    const scrolled = vi.fn();
    Element.prototype.scrollIntoView = scrolled;
    render(<BrowserPanel sessionId={S} visible />);
    act(() => {
      for (let n = 0; n < 8; n += 1) browserTabs.open(S);
    });
    const row = screen.getByRole('tablist', { name: '浏览器页面' });
    Object.defineProperty(row, 'clientWidth', { configurable: true, value: 300 });
    Object.defineProperty(row, 'scrollWidth', { configurable: true, value: 900 });
    act(() => {
      row.dispatchEvent(new Event('scroll'));
    });
    expect(row.closest('.browser-tabs')?.hasAttribute('data-overflow-right')).toBe(true);
    const wheel = new WheelEvent('wheel', { deltaY: 120, bubbles: true, cancelable: true });
    row.dispatchEvent(wheel);
    expect(row.scrollLeft).toBe(120);
    expect(wheel.defaultPrevented).toBe(true);
    expect(scrolled.mock.contexts.at(-1)).toBe(row.querySelector('.browser-tab.is-active'));
    Element.prototype.scrollIntoView = original;
  });

  // Even thirteen pages keep their titles; the real-window test measures width and scroll.
  it('keeps every page title when a narrow row holds thirteen pages', async () => {
    const width = vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get');
    width.mockReturnValue(1200);
    render(<BrowserPanel sessionId={S} visible />);
    act(() => {
      for (let n = 0; n < 3; n += 1) browserTabs.open(S);
    });
    const row = screen.getByRole('tablist', { name: '浏览器页面' });
    await vi.waitFor(() => expect(row.hasAttribute('data-compact')).toBe(false));
    width.mockReturnValue(300);
    act(() => {
      for (let n = 0; n < 10; n += 1) browserTabs.open(S);
    });
    await vi.waitFor(() => expect(row.hasAttribute('data-compact')).toBe(false));
    expect(within(row).getAllByRole('tab')).toHaveLength(13);
    expect(
      [...row.querySelectorAll('.browser-tab-title')].every((title) => title.textContent !== ''),
    ).toBe(true);
    width.mockRestore();
  });

  it('fills the main area from its own button', async () => {
    const user = userEvent.setup();
    const toggle = vi.fn();
    const { rerender } = render(<BrowserPanel onToggleExpanded={toggle} sessionId={S} visible />);
    await user.click(screen.getByRole('button', { name: '放大浏览器' }));
    expect(toggle).toHaveBeenCalledTimes(1);
    rerender(<BrowserPanel expanded onToggleExpanded={toggle} sessionId={S} visible />);
    expect(screen.getByRole('button', { name: '还原浏览器' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
  });
});
