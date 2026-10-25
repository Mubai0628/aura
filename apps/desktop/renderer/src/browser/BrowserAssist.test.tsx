import { act, cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { BrowserPageState, BrowserUserCommand } from '../../../shared/browser-api';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { remainingText } from './assist-time';
import { BrowserAssist } from './BrowserAssist';
import { browserStore } from './browser-store';
import { browserTabs, resetBrowserTabsForTests } from './browser-tabs';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const model = (over: Partial<BrowserPageState> = {}): BrowserPageState => ({
  sessionId: S,
  pageId: 'model',
  url: 'https://accounts.example/login',
  title: 'Sign in',
  loading: false,
  canGoBack: false,
  canGoForward: false,
  agentActive: false,
  userInControl: false,
  closed: false,
  ...over,
});

afterEach(() => {
  cleanup();
  resetToolTabsForTests();
  resetBrowserTabsForTests();
});

let command: ReturnType<typeof vi.fn<(value: BrowserUserCommand) => Promise<unknown>>>;
beforeEach(() => {
  command = vi.fn(async () => ({ ok: true }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        browser: {
          command,
          pages: vi.fn(async () => []),
          onState: vi.fn(() => () => undefined),
          getSettings: vi.fn(),
          updateSettings: vi.fn(),
        },
      },
    },
  });
});

describe("the model's request for help on its browser page", () => {
  it('counts down in minutes and seconds', () => {
    expect(remainingText(161_000)).toBe('02:41');
    expect(remainingText(400)).toBe('00:01');
    expect(remainingText(-5)).toBe('00:00');
  });

  it("shows the model's words with the time left, and sends the user's answer", async () => {
    const user = userEvent.setup();
    act(() =>
      browserStore.put(
        model({ assist: { reason: '请登录测试账号', deadline: Date.now() + 161_000 } }),
      ),
    );
    render(<BrowserAssist sessionId={S} />);
    expect(screen.getByRole('region', { name: '浏览器等待你的操作' })).toBeTruthy();
    expect(screen.getByText('请登录测试账号')).toBeTruthy();
    expect(screen.getByRole('timer').textContent).toMatch(/剩余 02:4[01]/u);
    const revealed: string[] = [];
    const stop = toolTabs.onReveal((session) => revealed.push(session));
    await user.click(screen.getByRole('button', { name: '打开浏览器' }));
    // The browser's tab, on the model's page, with the sidebar.
    expect(revealed).toContain(S);
    expect(browserTabs.get(S).shown).toBe('model');
    const tabs = toolTabs.get(S);
    expect(tabs.tabs.find((tab) => tab.id === tabs.active)?.kind).toBe('browser');
    stop();
    await user.click(screen.getByRole('button', { name: '完成' }));
    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(command.mock.calls.map(([value]) => value)).toEqual([
      { kind: 'assist-done', sessionId: S, pageId: 'model' },
      { kind: 'assist-decline', sessionId: S, pageId: 'model' },
    ]);
  });

  it('is gone when nothing waits, or the page is closed', () => {
    act(() => browserStore.put(model()));
    const { container } = render(<BrowserAssist sessionId={S} />);
    expect(container.textContent).toBe('');
    act(() =>
      browserStore.put(
        model({
          closed: true,
          notice: { kind: 'closed', text: '页面已关闭' },
          assist: { reason: 'x', deadline: 1 },
        }),
      ),
    );
    expect(container.textContent).toBe('');
  });
});
