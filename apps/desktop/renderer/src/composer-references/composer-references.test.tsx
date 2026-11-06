import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { PathSearchMatch, SessionModelSelectorProjection } from '@aura/contracts/generated';
import { ResizableComposer } from '../ResizableComposer';
import { resetComposerDraftsForTests } from '../agent-session/composer-drafts';
import type { FileActionScope } from '../files/file-menu';
import { MessageReferencesLine } from './MessageReferences';
import { otherConversations } from './conversations';
import type { ConversationOption } from './useReferenceMenu';

const entry = (path: string, kind: PathSearchMatch['kind'] = 'file', ignored = false) => ({
  path,
  kind,
  ignored,
  generated: false,
  score: 1,
});
const WORKSPACE = [
  entry('src', 'directory'),
  entry('src/lib.rs'),
  entry('my docs/a b.md'),
  entry('文档/会话标题.md'),
  entry('target/debug.log', 'file', true),
  entry('linked', 'reparse_point'),
];
const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;

const find = vi.fn(),
  submit = vi.fn();
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  // A stand-in for Core's search: names containing the words, or everything for none.
  find.mockReset().mockImplementation(async ({ query }: { query: string }) => ({
    ok: true,
    value: {
      catalog_revision: '1',
      matches: WORKSPACE.filter((match) => !query || match.path.includes(query)),
    },
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: vi.fn().mockResolvedValue({ ok: true, value: { commands: [] } }),
        findWorkspacePaths: find,
        submitAgentMessage: submit,
      },
    },
  });
});
afterEach(cleanup);

/** `conversation`: an open one; `new`: a new conversation in a workspace; `none`: no workspace. */
function renderComposer(
  where: 'conversation' | 'new' | 'none' = 'conversation',
  conversations?: readonly ConversationOption[],
  sessionId = 'session',
) {
  const scope = { session: sessionId, workspace: 'workspace', root: 'C:\\w', notify: vi.fn() };
  const scoped = where === 'conversation';
  render(
    <ResizableComposer
      sessionId={sessionId}
      workspaceId={where === 'none' ? undefined : 'workspace'}
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
      fileScope={scoped ? (scope as unknown as FileActionScope) : undefined}
      conversations={conversations}
    />,
  );
  return screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement;
}
const list = () => screen.queryByRole('listbox', { name: '文件和文件夹' });
const shown = () =>
  within(list()!)
    .getAllByRole('option')
    .map((option) => option.textContent);

test('a typed "@" lists the workspace without what Git ignores, and filters as words follow', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '看 @');
  await waitFor(() => expect(list()).toBeTruthy());
  expect(shown()).toEqual(['src', 'src/lib.rs', 'my docs/a b.md', '文档/会话标题.md']);
  await user.keyboard('lib');
  await waitFor(() => expect(shown()).toEqual(['src/lib.rs']));
  expect(find).toHaveBeenLastCalledWith(
    expect.objectContaining({ session_id: 'session', workspace_id: 'workspace', query: 'lib' }),
  );
  // Chinese names by the first letters of their pinyin.
  await user.clear(box);
  await user.type(box, '@hhbt');
  await waitFor(() => expect(shown()).toEqual(['文档/会话标题.md']));
  // Not after a space, or pasted: ordinary text.
  await user.clear(box);
  await user.type(box, 'mail me@');
  expect(list()).toBeNull();
  await user.clear(box);
  await user.paste('@src');
  expect(list()).toBeNull();
});

test('a pick writes the path after "@", quoted when it has a space, and the words are sent as they are', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '看 @lib');
  await waitFor(() => expect(shown()).toEqual(['src/lib.rs']));
  await user.keyboard('{Enter}');
  expect(box.value).toBe('看 @src/lib.rs ');
  expect(list()).toBeNull();
  await user.type(box, '和 @a b');
  // The space ended the token: the list closed, and the words stay text.
  expect(list()).toBeNull();
  await user.clear(box);
  await user.type(box, '@a');
  await waitFor(() => expect(shown()).toContain('my docs/a b.md'));
  const option = within(list()!).getByText('my docs/a b.md');
  await user.pointer({ keys: '[MouseLeft]', target: option });
  expect(box.value).toBe('@"my docs/a b.md" ');
  await user.keyboard('{Escape}');
  await user.type(box, '看看{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '@"my docs/a b.md" 看看',
      destination: { lane: 'next_turn' },
    }),
  );
});

test('Escape closes the list and keeps what was typed', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@sr');
  await waitFor(() => expect(list()).toBeTruthy());
  await user.keyboard('{Escape}');
  expect(list()).toBeNull();
  expect(box.value).toBe('@sr');
});

test("a new conversation's composer lists its workspace before the conversation exists", async () => {
  const user = userEvent.setup();
  const box = renderComposer('new');
  await user.type(box, '@lib');
  await waitFor(() => expect(shown()).toEqual(['src/lib.rs']));
  expect(find).toHaveBeenLastCalledWith({ workspace_id: 'workspace', query: 'lib', limit: 100 });
});

test('without a workspace, the list says there is none to look in', async () => {
  const user = userEvent.setup();
  const box = renderComposer('none');
  await user.type(box, '@');
  expect(await screen.findByText('没有可查找的工作区')).toBeTruthy();
  expect(find).not.toHaveBeenCalled();
});

const CONVERSATIONS = [
  { id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV', title: '整理发布清单' },
  { id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW', title: 'Release notes' },
];
const conversationList = () => screen.queryByRole('listbox', { name: '会话' });
const chips = () => screen.queryByRole('group', { name: '引用' });

test('"@" lists the workspace\'s other conversations; one picked is a chip sent with the message', async () => {
  const user = userEvent.setup();
  const box = renderComposer('conversation', CONVERSATIONS);
  await user.type(box, '接着 @');
  await waitFor(() => expect(conversationList()).toBeTruthy());
  expect(
    within(conversationList()!)
      .getAllByRole('option')
      .map((option) => option.textContent),
  ).toEqual(['整理发布清单', 'Release notes']);
  // By the first letters of a Chinese title's pinyin, as files are.
  await user.keyboard('zlfb');
  await waitFor(() => expect(within(conversationList()!).getAllByRole('option')).toHaveLength(1));
  await user.keyboard('{Enter}');
  // The typed "@zlfb" went; the conversation is a chip, not words.
  expect(box.value).toBe('接着 ');
  expect(within(chips()!).getByText('整理发布清单')).toBeTruthy();
  // Not listed again once chosen.
  await user.type(box, '@');
  await waitFor(() => expect(conversationList()).toBeTruthy());
  expect(within(conversationList()!).queryByText('整理发布清单')).toBeNull();
  await user.keyboard('{Escape}');
  await user.keyboard('{Backspace}做{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '接着 做',
      destination: { lane: 'next_turn' },
      references: [{ kind: 'session', session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV' }],
    }),
  );
  await waitFor(() => expect(chips()).toBeNull());
});

test('a chip is still there after leaving the conversation and coming back, and only there', async () => {
  const user = userEvent.setup();
  let box = renderComposer('conversation', CONVERSATIONS);
  await user.type(box, '@zl');
  await waitFor(() => expect(conversationList()).toBeTruthy());
  await user.keyboard('{Enter}');
  expect(within(chips()!).getByText('整理发布清单')).toBeTruthy();
  // The settings page in between unmounts the conversation; another one has its own chips.
  cleanup();
  renderComposer('conversation', CONVERSATIONS, 'other');
  expect(chips()).toBeNull();
  cleanup();
  box = renderComposer('conversation', CONVERSATIONS);
  expect(within(chips()!).getByText('整理发布清单')).toBeTruthy();
  // Once sent it is gone, also after coming back.
  await user.type(box, '接着做{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith(
      expect.objectContaining({
        references: [{ kind: 'session', session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV' }],
      }),
    ),
  );
  await waitFor(() => expect(chips()).toBeNull());
  cleanup();
  renderComposer('conversation', CONVERSATIONS);
  expect(chips()).toBeNull();
});

test('a chip can be removed, and a message of chips alone is sent', async () => {
  const user = userEvent.setup();
  const box = renderComposer('conversation', CONVERSATIONS);
  await user.type(box, '@rel');
  await waitFor(() => expect(conversationList()).toBeTruthy());
  await user.keyboard('{Enter}');
  await user.type(box, '@zl');
  await waitFor(() => expect(conversationList()).toBeTruthy());
  await user.keyboard('{Enter}');
  await user.click(screen.getByRole('button', { name: '移除引用的会话 Release notes' }));
  expect(within(chips()!).queryByText('Release notes')).toBeNull();
  expect(box.value).toBe('');
  await user.type(box, '{Enter}');
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith({
      session_id: 'session',
      text: '',
      destination: { lane: 'next_turn' },
      references: [{ kind: 'session', session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV' }],
    }),
  );
});

test('the conversations offered leave out drafts and the conversation itself, and name unnamed ones', () => {
  expect(
    otherConversations(
      [
        { id: 'ses_a', state: 'idle', title: '当前' },
        { id: 'ses_b', state: 'idle', title: '  ' },
        { id: 'draft_1', state: 'draft', title: '新会话' },
        { id: 'ses_c', state: 'running', title: '另一个' },
      ],
      'ses_a',
    ),
  ).toEqual([
    { id: 'ses_b', title: '新会话' },
    { id: 'ses_c', title: '另一个' },
  ]);
  expect(otherConversations(undefined, undefined)).toEqual([]);
});

test('a message shows what it referred to, once each', () => {
  render(
    <MessageReferencesLine
      references={[
        { kind: 'file', label: 'src/lib.rs' },
        { kind: 'folder', label: 'docs' },
        { kind: 'folder', label: '.' },
        { kind: 'file', label: 'src/lib.rs' },
        { kind: 'session', label: '整理发布清单' },
      ]}
    />,
  );
  expect(
    screen.getByText('引用：src/lib.rs、docs/、工作区根目录、会话「整理发布清单」'),
  ).toBeTruthy();
  cleanup();
  const { container } = render(<MessageReferencesLine references={[]} />);
  expect(container.textContent).toBe('');
});
