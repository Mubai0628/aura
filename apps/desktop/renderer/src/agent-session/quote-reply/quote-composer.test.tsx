import type { SessionModelSelectorProjection } from '@aura/contracts/generated';
import { act, cleanup, render, renderHook, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ResizableComposer } from '../../ResizableComposer';
import {
  createLocalConversation,
  discardEmptyConversations,
  getComposerDraft,
  resetComposerDraftsForTests,
  updateComposerDraft,
} from '../composer-drafts';
import { MessageText } from '../MessageText';
import { useComposerSubmission } from '../useComposerSubmission';

// A passage replied to waits above the composer's words as a block with its reply and an ×. The
// blocks belong to their conversation and are kept with its draft. Sending puts them before the
// typed words as Markdown quotes: for a new turn, for the queue while a task runs, and for a Plan.

const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;
const submit = vi.fn();
const start = vi.fn();
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  start.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: vi.fn().mockResolvedValue({ ok: true, value: { commands: [] } }),
        listSkills: vi.fn().mockResolvedValue({
          ok: true,
          value: { revision: 1, skills: [], roots: [], issue: null },
        }),
        submitAgentMessage: submit,
        startAgent: start,
      },
    },
  });
});
afterEach(cleanup);

function renderComposer(running = false, sessionId = 'session') {
  render(
    <ResizableComposer
      sessionId={sessionId}
      activeTurn={running ? 'turn' : undefined}
      activeRunId={running ? 'run' : undefined}
      workspaceId="workspace"
      queuePending={false}
      onChanged={vi.fn()}
      onStop={vi.fn()}
      demoMode={false}
      modelSelector={selector}
      onOpenModelSettings={vi.fn()}
      onOpenPromptInspector={vi.fn()}
      onOpenSecuritySettings={vi.fn()}
      onOpenSettings={vi.fn()}
      onOpenPanel={vi.fn()}
      onSelectModel={vi.fn()}
      onSetPermissionMode={vi.fn()}
      permissionMode="read_only"
      promptInspector={undefined}
    />,
  );
  return screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement;
}

const quote = (id: string, text: string, reply = '') => ({ id, quote: text, reply });
const blocks = () => screen.queryByRole('list', { name: '引用回复' });

test('quoted passages wait above the words of their own conversation; × takes one out', async () => {
  const user = userEvent.setup();
  updateComposerDraft('session', {
    quotes: [quote('a', '模型说的第一句', '这里不对'), quote('b', '第二句')],
  });
  renderComposer();
  const items = within(blocks()!).getAllByRole('listitem');
  expect(items.map((item) => item.querySelector('blockquote')?.textContent)).toEqual([
    '模型说的第一句',
    '第二句',
  ]);
  expect(items[0]!.textContent).toContain('这里不对');
  await user.click(within(items[1]!).getByRole('button', { name: /^移除引用/u }));
  expect(within(blocks()!).getAllByRole('listitem')).toHaveLength(1);
  expect(getComposerDraft('session')?.quotes).toEqual([quote('a', '模型说的第一句', '这里不对')]);
  cleanup();
  renderComposer(false, 'other');
  expect(blocks()).toBeNull();
});

test('the blocks are kept with the conversation’s draft when Aura starts again', () => {
  updateComposerDraft('session', { quotes: [quote('a', '保留下来', '回复')] });
  resetComposerDraftsForTests();
  renderComposer();
  expect(within(blocks()!).getByRole('listitem').textContent).toContain('保留下来');
  // A conversation that holds only quotes is not an untouched one.
  const local = createLocalConversation('workspace');
  updateComposerDraft(local, { quotes: [quote('b', '只有引用')] });
  discardEmptyConversations();
  resetComposerDraftsForTests();
  expect(getComposerDraft(local)?.quotes).toHaveLength(1);
});

test('sending puts the quotes before the typed words as Markdown quotes, then clears them', async () => {
  const user = userEvent.setup();
  updateComposerDraft('session', {
    quotes: [quote('a', '第一行\n第二行', '这里要改'), quote('b', '另一段')],
  });
  const box = renderComposer();
  await user.type(box, '按这两处修改');
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  expect(submit).toHaveBeenCalledWith({
    session_id: 'session',
    text: '> 第一行\n> 第二行\n\n这里要改\n\n> 另一段\n\n按这两处修改',
    destination: { lane: 'next_turn' },
  });
  await waitFor(() => expect(blocks()).toBeNull());
  expect(getComposerDraft('session')?.quotes ?? []).toEqual([]);
  expect(box.value).toBe('');
});

test('while a task runs, quotes alone can be sent and wait in the queue as one message', async () => {
  const user = userEvent.setup();
  updateComposerDraft('session', { quotes: [quote('a', '只引用这一句')] });
  renderComposer(true);
  // Nothing typed, yet something to send: the button queues it instead of stopping the task.
  expect(screen.queryByRole('button', { name: '停止当前任务' })).toBeNull();
  await user.click(screen.getByRole('button', { name: '加入下一轮队列' }));
  expect(submit).toHaveBeenCalledWith({
    session_id: 'session',
    text: '> 只引用这一句',
    destination: { lane: 'next_turn' },
  });
});

test('a message that was not sent keeps its quotes and its words', async () => {
  const user = userEvent.setup();
  submit.mockResolvedValueOnce({
    ok: false,
    error: {
      category: 'validation',
      code: 'agent.rejected',
      message: '模型暂时不可用',
      details: {},
      terminal: 'failed',
    },
  });
  updateComposerDraft('session', { quotes: [quote('a', '模型的话', '我的回复')] });
  const box = renderComposer();
  await user.type(box, '再试一次');
  await user.click(screen.getByRole('button', { name: '发送消息' }));
  await waitFor(() => expect(box.value).toBe('再试一次'));
  expect(within(blocks()!).getAllByRole('listitem')).toHaveLength(1);
  expect(getComposerDraft('session')?.quotes).toEqual([quote('a', '模型的话', '我的回复')]);
});

test('a Plan started with quotes carries them before its words', async () => {
  updateComposerDraft('plan', { quotes: [quote('a', '计划里的这一步', '先做这个')] });
  const { result } = renderHook(() =>
    useComposerSubmission('plan', undefined, 'model', false, vi.fn()),
  );
  act(() => {
    result.current.setDraft('出一个计划');
    result.current.setMode('plan');
  });
  await act(() => result.current.send());
  expect(start).toHaveBeenCalledWith(
    expect.objectContaining({ text: '> 计划里的这一步\n\n先做这个\n\n出一个计划', mode: 'plan' }),
  );
  expect(getComposerDraft('plan')?.quotes ?? []).toEqual([]);
});

/** Each line of a paragraph as it shows: the text between its line breaks. */
const lines = (paragraph: Element | null | undefined) =>
  [...(paragraph?.childNodes ?? [])]
    .reduce<string[]>(
      (shown, node) => {
        if (node.nodeName === 'BR') shown.push('');
        else shown[shown.length - 1] += node.textContent ?? '';
        return shown;
      },
      [''],
    )
    .map((line) => line.trim());

test('the sent message shows the quote as a quote and the rest as typed', () => {
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
  const text =
    '> 模型的第一行\n> 模型的第二行\n\n回复第一行\n回复第二行\n\n再看 C:\\Work\\app\\.cache';
  const { container } = render(
    <MessageText
      preview={text}
      role="user"
      sessionId="ses_01M5PF3QWJN5S3P0JFPVBSFY3R"
      source={{ kind: 'user', event_id: 'evt_01M5PF3QWJN5S3P0JFPVBSFY3R' }}
      totalBytes={new TextEncoder().encode(text).length}
    />,
  );
  const quoted = container.querySelector('blockquote');
  expect(lines(quoted?.querySelector('p'))).toEqual(['模型的第一行', '模型的第二行']);
  const typed = [...container.querySelectorAll('p')].filter((p) => !p.closest('blockquote'));
  expect(lines(typed[0])).toEqual(['回复第一行', '回复第二行']);
  expect(typed[1]?.textContent).toBe('再看 C:\\Work\\app\\.cache');
  vi.unstubAllGlobals();
});
