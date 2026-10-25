import { beforeEach, expect, test, vi } from 'vitest';
import {
  createLocalConversation,
  discardEmptyConversations,
  getComposerDraft,
  resetComposerDraftsForTests,
  updateComposerDraft,
} from './composer-drafts';
import { firstMessageTitle, flushPendingTitles, rememberFirstTitle } from './session-titles';
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
});
test('untouched conversations never persist; typed drafts survive restart without assigning a title', () => {
  const empty = createLocalConversation('workspace');
  resetComposerDraftsForTests();
  expect(getComposerDraft(empty)).toBeUndefined();
  const id = createLocalConversation('workspace');
  updateComposerDraft(id, { text: '这是一段未发送的内容' });
  resetComposerDraftsForTests();
  expect(getComposerDraft(id)?.text).toBe('这是一段未发送的内容');
  expect(getComposerDraft(id)).not.toHaveProperty('title');
});
test('clearing an unsent draft discards it on leaving but preserves uncertain submissions', () => {
  const id = createLocalConversation('workspace');
  updateComposerDraft(id, { text: 'draft' });
  updateComposerDraft(id, { text: '' });
  expect(getComposerDraft(id)).toBeDefined();
  discardEmptyConversations();
  expect(getComposerDraft(id)).toBeUndefined();
  const unknown = createLocalConversation('workspace');
  updateComposerDraft(unknown, { needsReview: true });
  discardEmptyConversations();
  expect(getComposerDraft(unknown)?.needsReview).toBe(true);
});
test('only accepted first messages assign five Unicode characters and metadata retry never resends', async () => {
  expect(firstMessageTitle('  请帮我完成界面  ')).toBe('请帮我完成');
  expect(firstMessageTitle('😀一二三四五')).toBe('😀一二三四');
  const rename = vi.fn().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { renameSession: rename } },
  });
  rememberFirstTitle('session', '请帮我完成界面');
  await flushPendingTitles();
  expect(rename).toHaveBeenCalledWith({
    session_id: 'session',
    expected_title: '',
    title: '请帮我完成',
  });
  await flushPendingTitles();
  expect(rename).toHaveBeenCalledOnce();
});

test('a title accepted while another rename is pending is flushed in the same batch', async () => {
  let finish: (value: unknown) => void = () => undefined;
  const rename = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    )
    .mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { renameSession: rename } },
  });
  rememberFirstTitle('first', '第一条消息');
  rememberFirstTitle('second', '第二条消息');
  finish({ ok: true, value: {} });
  await flushPendingTitles();
  expect(rename).toHaveBeenCalledTimes(2);
  expect(rename).toHaveBeenLastCalledWith({
    session_id: 'second',
    expected_title: '',
    title: '第二条消息',
  });
});
