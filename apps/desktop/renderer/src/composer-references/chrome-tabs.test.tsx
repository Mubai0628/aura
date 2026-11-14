import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { SessionModelSelectorProjection } from '@aura/contracts/generated';
import { ResizableComposer } from '../ResizableComposer';
import { resetComposerDraftsForTests } from '../agent-session/composer-drafts';
import type { FileActionScope } from '../files/file-menu';

const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;
const CART = 'https://shop.example/cart';
const TOO_OLD = 'Chrome 里的 Aura 扩展需要更新，才能在这里选标签页。';

const submit = vi.fn(),
  tabs = vi.fn(),
  handOverTab = vi.fn();
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  tabs.mockReset().mockResolvedValue({
    ok: true,
    value: {
      connected: true,
      tabs: [
        { tab_id: 7, title: 'Cart', url: CART },
        { tab_id: 8, title: '  ', url: 'https://docs.example/a' },
      ],
      issue: null,
    },
  });
  handOverTab.mockReset().mockResolvedValue({
    ok: true,
    value: {
      tab: 't1',
      title: 'Cart',
      url: CART,
      snapshot: 'button "Checkout" [ref=e1]',
      snapshot_truncated: false,
    },
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: vi.fn().mockResolvedValue({ ok: true, value: { commands: [] } }),
        findWorkspacePaths: vi.fn().mockResolvedValue({
          ok: true,
          value: { catalog_revision: '1', matches: [] },
        }),
        submitAgentMessage: submit,
        chrome: { tabs, handOverTab },
      },
    },
  });
});
afterEach(cleanup);

/** `conversation`: an open one; `new`: a new conversation, which has no session yet. */
function renderComposer(where: 'conversation' | 'new' = 'conversation') {
  const scope = { session: 'session', workspace: 'workspace', root: 'C:\\w', notify: vi.fn() };
  render(
    <ResizableComposer
      sessionId="session"
      workspaceId="workspace"
      queuePending={false}
      onChanged={vi.fn()}
      onStop={vi.fn()}
      demoMode={false}
      modelSelector={selector}
      onOpenModelSettings={vi.fn()}
      onOpenPromptInspector={vi.fn()}
      onOpenSecuritySettings={vi.fn()}
      onSelectModel={vi.fn()}
      onSetPermissionMode={vi.fn()}
      permissionMode="workspace_read_write"
      promptInspector={undefined}
      fileScope={where === 'conversation' ? (scope as unknown as FileActionScope) : undefined}
    />,
  );
  return screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement;
}
const group = () => screen.queryByRole('listbox', { name: 'Chrome 标签页' });
const options = () =>
  within(group()!)
    .getAllByRole('option')
    .map((option) => option.textContent);

test('a Chrome tab picked from "@" is shared when the message is sent, and goes as it reads, named as Chrome\'s tools take it', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(group()).toBeTruthy());
  expect(options()).toEqual([`Cart${CART}`, 'https://docs.example/ahttps://docs.example/a']);
  // By its title or its address.
  await user.keyboard('shop');
  await waitFor(() => expect(options()).toEqual([`Cart${CART}`]));
  await user.keyboard('{Enter}');
  expect(box.value).toBe('');
  expect(within(screen.getByRole('group', { name: '引用' })).getByText('Cart')).toBeTruthy();
  expect(handOverTab).not.toHaveBeenCalled();
  await user.type(box, '结账前帮我检查{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '结账前帮我检查',
      destination: { lane: 'next_turn' },
      attachments: [
        {
          kind: 'capture',
          name: 'Cart.txt',
          text: `Chrome tab "Cart" (${CART}; the user shared it with you as tab \`t1\`: the chrome tools act on it when given tab "t1"; it stays the user's, who can stop sharing it at any time, and chrome.close closes only tabs chrome.open opened) as it was when this message was sent:\n\nbutton "Checkout" [ref=e1]\n`,
        },
      ],
    }),
  );
  // Shared before the message goes, so the model's first step finds it.
  expect(handOverTab).toHaveBeenCalledWith({ session_id: 'session', tab_id: 7 });
  expect(handOverTab.mock.invocationCallOrder[0]).toBeLessThan(submit.mock.invocationCallOrder[0]!);
});

test('a Chrome tab that cannot be handed over keeps the message, saying why', async () => {
  handOverTab.mockResolvedValue({
    ok: false,
    error: {
      code: 'chrome.tab_gone',
      message: 'chrome.tab_gone: 这个标签页已经关闭，或不能交给 Aura。',
    },
  });
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@cart');
  await waitFor(() => expect(options()).toEqual([`Cart${CART}`]));
  await user.keyboard('{Enter}');
  await user.type(box, '看看{Enter}');
  await waitFor(() =>
    expect(screen.getByRole('alert').textContent).toContain(
      '这个标签页已经关闭，或不能交给 Aura。',
    ),
  );
  expect(submit).not.toHaveBeenCalled();
  expect(box.value).toBe('看看');
});

test('an extension too old to list tabs says so in the list; without Chrome or a conversation there are none', async () => {
  tabs.mockResolvedValue({ ok: true, value: { connected: true, tabs: [], issue: TOO_OLD } });
  const user = userEvent.setup();
  let box = renderComposer();
  await user.type(box, '@');
  expect(await screen.findByText(TOO_OLD)).toBeTruthy();
  expect(screen.getByText('Chrome 标签页')).toBeTruthy();
  expect(group()).toBeNull();
  cleanup();
  localStorage.clear();
  resetComposerDraftsForTests();
  tabs.mockResolvedValue({ ok: true, value: { connected: false, tabs: [], issue: null } });
  box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(tabs).toHaveBeenCalledTimes(2));
  expect(screen.queryByText('Chrome 标签页')).toBeNull();
  cleanup();
  localStorage.clear();
  resetComposerDraftsForTests();
  tabs.mockClear();
  box = renderComposer('new');
  await user.type(box, '@');
  await waitFor(() => expect(screen.getByText('没有相符的文件或文件夹')).toBeTruthy());
  expect(tabs).not.toHaveBeenCalled();
});
