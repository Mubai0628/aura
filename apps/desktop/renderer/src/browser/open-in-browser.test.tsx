import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import type { BrowserUserCommand } from '../../../shared/browser-api';
import { FileLinkContext } from '../files/file-links';
import { root, session, workspace } from '../files/files-fixture';
import { MarkdownBody } from '../markdown-body';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { browserTabs, resetBrowserTabsForTests } from './browser-tabs';

// A workspace web page and a web link the
// conversation shows open in Aura's own browser from their right-click menus.

const notify = vi.fn();
let command: ReturnType<typeof vi.fn<(value: BrowserUserCommand) => Promise<unknown>>>;
beforeEach(() => {
  resetToolTabsForTests();
  resetBrowserTabsForTests();
  notify.mockReset();
  command = vi.fn(async () => ({ ok: true }));
  vi.stubGlobal('aura', { v1: { browser: { command } } });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const message = (text: string) =>
  render(
    <FileLinkContext.Provider value={{ session, workspace, root, notify }}>
      <MarkdownBody text={text} />
    </FileLinkContext.Provider>,
  );
const browserShown = () => {
  const tabs = toolTabs.get(session);
  return tabs.tabs.find((tab) => tab.id === tabs.active)?.kind === 'browser';
};

test('a workspace web page opens in a new page of the browser', async () => {
  const user = userEvent.setup();
  message('入口是 `game/index.html`。');
  fireEvent.contextMenu(screen.getByRole('button', { name: '打开 game/index.html' }));
  const menu = screen.getByRole('menu', { name: 'game/index.html 的操作' });
  await user.click(within(menu).getByRole('menuitem', { name: '在内置浏览器打开' }));
  await vi.waitFor(() => expect(browserTabs.get(session).pages).toHaveLength(1));
  const [page] = browserTabs.get(session).pages;
  expect(command.mock.calls.map(([value]) => value)).toEqual([
    { kind: 'open', sessionId: session, pageId: page },
    {
      kind: 'open-file',
      sessionId: session,
      pageId: page,
      workspaceId: workspace,
      path: 'game/index.html',
    },
  ]);
  expect(browserShown()).toBe(true);
});

test('a web link opens in the browser too; what Main refuses is said', async () => {
  const user = userEvent.setup();
  message('参见[文档](https://example.com/docs)。');
  fireEvent.contextMenu(screen.getByRole('link', { name: '文档' }));
  expect(screen.getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
    '在默认浏览器中打开',
    '在内置浏览器打开',
    '复制链接',
  ]);
  command.mockImplementation(async (value) =>
    value.kind === 'navigate'
      ? { ok: false, message: 'example.com 已在浏览器设置中被阻止' }
      : { ok: true },
  );
  await user.click(screen.getByRole('menuitem', { name: '在内置浏览器打开' }));
  await vi.waitFor(() => expect(notify).toHaveBeenCalledWith('example.com 已在浏览器设置中被阻止'));
  expect(command).toHaveBeenCalledWith(
    expect.objectContaining({ kind: 'navigate', input: 'https://example.com/docs' }),
  );
  expect(browserTabs.get(session).pages).toEqual([]);
});
