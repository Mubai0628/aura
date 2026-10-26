import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { NewSessionView } from './NewSessionView';
import {
  createLocalConversation,
  resetComposerDraftsForTests,
  useComposerDrafts,
} from './agent-session/composer-drafts';
import { resetSendingForTests } from './agent-session/sending';
vi.mock('./agent-session/useDraftModelChoices', () => ({
  useDraftModelChoices: () => ({
    choices: [{ id: 'model', label: 'Flash', provider: 'Provider' }],
    defaultId: 'model',
    loading: false,
  }),
}));
vi.mock('./model-projection-store', () => ({
  readSessionModelSelector: async () => ({
    payload: { binding_revision: '0', selected_model_profile_id: null },
  }),
}));
const create = vi.fn(),
  bind = vi.fn(),
  submit = vi.fn(),
  start = vi.fn(),
  rename = vi.fn(),
  accepted = vi.fn();
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  resetSendingForTests();
  create.mockReset().mockResolvedValue({ ok: true, value: { session_id: 'server-session' } });
  bind.mockReset().mockResolvedValue({ ok: true, value: { revision: '1' } });
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  start.mockReset().mockResolvedValue({ ok: true, value: {} });
  rename.mockReset().mockResolvedValue({ ok: true, value: {} });
  accepted.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        createSession: create,
        setSessionModelBinding: bind,
        submitAgentMessage: submit,
        startAgent: start,
        renameSession: rename,
        getPermissionDefaults: async () => ({ ok: true, value: { mode: 'read_only' } }),
        // The task modes are built-in commands of Core's catalog.
        listCommands: async () => ({
          ok: true,
          value: {
            commands: ['plan', 'goal'].map((name) => ({
              name,
              title: name,
              description: name,
              kind: 'immediate',
              source: 'builtin',
              plugin: null,
              argument_hint: null,
              template: null,
              enabled: true,
              issue: null,
              location: null,
              revision: null,
            })),
          },
        }),
      },
    },
  });
});
afterEach(cleanup);
function Fixture({ id }: { id: string }) {
  const draft = useComposerDrafts().find((item) => item.id === id)!;
  return (
    <NewSessionView
      draft={draft}
      onOpenSettings={vi.fn()}
      onAccepted={accepted}
      onInspect={vi.fn()}
    />
  );
}
test('typing saves the draft while keeping New Session; first send binds then submits and names five characters', async () => {
  const user = userEvent.setup();
  const id = createLocalConversation('workspace');
  render(<Fixture id={id} />);
  await user.type(screen.getByRole('textbox', { name: '输入消息' }), '请帮我完成测试');
  expect(screen.getByRole('heading', { name: '新会话' })).toBeTruthy();
  expect(create).not.toHaveBeenCalled();
  expect(localStorage.getItem('aura.composer-drafts.v1')).toContain('请帮我完成测试');
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  await waitFor(() => expect(accepted).toHaveBeenCalledWith(id, 'server-session'));
  expect(create).toHaveBeenCalledWith({ workspace_id: 'workspace', title: '' });
  expect(bind).toHaveBeenCalledWith({
    session_id: 'server-session',
    model_profile_id: 'model',
    expected_revision: '0',
  });
  expect(submit).toHaveBeenCalledWith({
    session_id: 'server-session',
    text: '请帮我完成测试',
    destination: { lane: 'next_turn' },
  });
  expect(bind.mock.invocationCallOrder[0]).toBeLessThan(submit.mock.invocationCallOrder[0]!);
  expect(rename).toHaveBeenCalledWith({
    session_id: 'server-session',
    expected_title: '',
    title: '请帮我完成',
  });
});

test('a file dropped on the new conversation joins the composer before the session exists', async () => {
  const user = userEvent.setup();
  Object.assign(window.aura.v1, { attachmentPath: (file: File) => `C:\\work\\${file.name}` });
  const id = createLocalConversation('workspace');
  render(<Fixture id={id} />);
  fireEvent.drop(screen.getByRole('heading', { name: '从一个想法开始' }), {
    dataTransfer: {
      types: ['Files'],
      files: [new File(['%PDF'], 'new.pdf', { type: 'application/pdf' })],
    },
  });
  expect(await screen.findByText('new.pdf')).toBeTruthy();
  expect(create).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  await waitFor(() =>
    expect(submit).toHaveBeenCalledWith(
      expect.objectContaining({
        session_id: 'server-session',
        attachments: [{ kind: 'file', path: 'C:\\work\\new.pdf' }],
      }),
    ),
  );
});
// /plan chooses Plan; slash text not chosen from the menu stays text.
test('Plan comes from /plan, and slash text not chosen from the menu is sent as typed', async () => {
  const user = userEvent.setup();
  const id = createLocalConversation('workspace');
  render(<Fixture id={id} />);
  expect(screen.queryByRole('combobox', { name: '任务模式' })).toBeNull();
  const box = screen.getByRole('textbox', { name: '输入消息' });
  await user.type(box, '/plan');
  await user.keyboard('{Enter}');
  expect(screen.getByText('Plan', { selector: '.composer-mode-chip *' })).toBeTruthy();
  await user.type(box, '/goal 是普通文字');
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  await waitFor(() =>
    expect(start).toHaveBeenCalledWith({
      session_id: 'server-session',
      model_profile_id: 'model',
      text: '/goal 是普通文字',
      mode: 'plan',
      goal_budget: null,
    }),
  );
  expect(submit).not.toHaveBeenCalled();
});
test('binding rejection does not send or pause a message; retry uses the same materialized session', async () => {
  bind.mockResolvedValueOnce({
    ok: false,
    error: { message: 'configuration.credential_required' },
  });
  const user = userEvent.setup();
  const id = createLocalConversation('workspace');
  render(<Fixture id={id} />);
  await user.type(screen.getByRole('textbox', { name: '输入消息' }), '保留内容');
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    '模型配置尚未就绪，请在设置中检查凭据与模型。',
  );
  expect(submit).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  await waitFor(() => expect(submit).toHaveBeenCalledOnce());
  expect(create).toHaveBeenCalledOnce();
});

// The message shows the moment it is sent, while the conversation is still being made.
test('the first message shows at once while the conversation is made, and comes back if refused', async () => {
  let made: (value: unknown) => void = () => undefined;
  create.mockReturnValueOnce(new Promise((resolve) => (made = resolve)));
  submit.mockResolvedValueOnce({ ok: false, error: { message: 'agent.model_unavailable' } });
  const user = userEvent.setup();
  const id = createLocalConversation('workspace');
  render(<Fixture id={id} />);
  const box = screen.getByRole('textbox', { name: '输入消息' });
  await user.type(box, '你好');
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  const shown = await screen.findByText('你好', { selector: '.agent-sending *' });
  expect(shown.closest('article')?.textContent).toContain('发送中');
  expect(screen.queryByRole('heading', { name: '从一个想法开始' })).toBeNull();
  expect((box as HTMLTextAreaElement).value).toBe('');
  made({ ok: true, value: { session_id: 'server-session' } });
  expect(await screen.findByRole('alert')).toBeTruthy();
  expect(screen.queryByText('发送中')).toBeNull();
  expect((box as HTMLTextAreaElement).value).toBe('你好');
  expect(screen.getByRole('heading', { name: '从一个想法开始' })).toBeTruthy();
  expect(accepted).not.toHaveBeenCalled();
});
