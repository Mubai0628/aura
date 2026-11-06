import type { AgentSessionProjection } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from '../Conversation';
import { getComposerDraft, resetComposerDraftsForTests } from '../composer-drafts';

// Text selected in the conversation offers 回复 beside it and in its right-click menu. 回复 opens a
// card at the selection: the quote, and a box for the reply. Enter puts both into the composer of
// this conversation; Shift+Enter starts a new line; Escape or a press outside the card cancels.

const PASSAGE = '另外，如果你要的是一张能用的鲸鱼娘图，第 2 条才靠谱。';
const ANSWER = `三条路，你选。\n\n${PASSAGE}`;
const ASKED = '那你再用代码生成一张我看看';
const bytes = (text: string) => new TextEncoder().encode(text).length;

const writeClipboard = vi.fn();
/** Where the selection is on screen; jsdom lays nothing out. */
let selectionBox = { left: 200, top: 300, right: 600, bottom: 340, width: 400, height: 40 };
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  selectionBox = { left: 200, top: 300, right: 600, bottom: 340, width: 400, height: 40 };
  Object.defineProperty(Range.prototype, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({ ...selectionBox, x: selectionBox.left, y: selectionBox.top }),
  });
  writeClipboard
    .mockReset()
    .mockResolvedValue({ ok: true, value: { completed: true, message: '已复制' } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        writeClipboard,
        readAgentToolActivity: vi.fn(async () => ({
          ok: true,
          value: { session_id: 'session', next_before_call_id: null, calls: [] },
        })),
      },
    },
  });
});
afterEach(() => {
  cleanup();
  document.getSelection()?.removeAllRanges();
  delete (Range.prototype as Partial<Range>).getBoundingClientRect;
});

const step = (id: string, text: string) => ({
  model_step_id: id,
  model_profile_id: null,
  context_snapshot_id: 'mex_1',
  text,
  reasoning: '',
  text_total_bytes: bytes(text),
  reasoning_total_bytes: 0,
  partial: false,
  stream_sequence: null,
  tool_call_ids: [],
  at: '2026-11-05T00:00:00Z',
});
const projection = {
  session_id: 'session',
  runtime_instance_id: 'runtime',
  active: null,
  turns: [
    {
      turn: {
        mode: 'normal',
        state: { state: 'completed', final_report: ANSWER },
        session_id: 'session',
        turn_task_id: 'turn',
        revision: '3',
        root_user_event_id: 'root',
        created_at: '2026-11-05T00:00:00Z',
        updated_at: '2026-11-05T00:00:09Z',
      },
      final_text_source: { kind: 'assistant_step', model_step_id: 'answer' },
      user_messages: [
        { event_id: 'root', text: ASKED, total_bytes: bytes(ASKED), origin: { origin: 'submit' } },
      ],
      assistant_messages: [step('answer', ANSWER)],
      goal: null,
    },
  ],
} as unknown as AgentSessionProjection;

function show() {
  render(
    <>
      <Conversation
        sessionId="session"
        projection={projection}
        viewportRef={{ current: null }}
        onChanged={vi.fn()}
        loading={false}
        error={undefined}
        loadOlder={vi.fn()}
        loadTurnOlder={vi.fn()}
      />
      <p>会话外的文字</p>
      <textarea id="aura-composer" aria-label="输入消息" />
    </>,
  );
}

/** Selects from the start of `from` to the end of `to`, as a drag of the mouse would. */
function select(from: Node, to: Node = from) {
  const range = document.createRange();
  range.setStart(from, 0);
  range.setEnd(to, to.childNodes.length);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
}

/** Lets the selection's own events arrive (jsdom sends them a moment later, as browsers do). */
const settle = () => act(() => new Promise<void>((resolve) => setTimeout(resolve, 20)));
const replyButton = () => screen.queryByRole('button', { name: '回复' });
const quotes = () => getComposerDraft('session')?.quotes ?? [];

test('a passage selected in the answer offers 回复 above it; it goes when the selection does', async () => {
  show();
  const passage = await screen.findByText(PASSAGE);
  select(passage);
  const button = await screen.findByRole('button', { name: '回复' });
  const floating = button.closest('[data-quote-reply]') as HTMLElement;
  expect(parseFloat(floating.style.top)).toBeLessThan(selectionBox.top);
  document.getSelection()!.removeAllRanges();
  await waitFor(() => expect(replyButton()).toBeNull());
  // Near the top of what can be seen there is no room above: it goes below.
  selectionBox = { left: 200, top: 4, right: 600, bottom: 44, width: 400, height: 40 };
  select(passage);
  const below = (await screen.findByRole('button', { name: '回复' })).closest(
    '[data-quote-reply]',
  ) as HTMLElement;
  expect(parseFloat(below.style.top)).toBeGreaterThan(selectionBox.bottom);
});

test('the user’s own message can be replied to as well', async () => {
  show();
  select(await screen.findByText(ASKED));
  expect(await screen.findByRole('button', { name: '回复' })).toBeTruthy();
});

test('nothing is offered while the mouse is still selecting; it comes when released', async () => {
  show();
  const passage = await screen.findByText(PASSAGE);
  fireEvent.pointerDown(passage, { button: 0 });
  select(passage);
  await settle();
  expect(replyButton()).toBeNull();
  fireEvent.pointerUp(passage, { button: 0 });
  expect(await screen.findByRole('button', { name: '回复' })).toBeTruthy();
});

test('a selection reaching outside the conversation, or inside a field, offers nothing', async () => {
  show();
  const passage = await screen.findByText(PASSAGE);
  select(passage, screen.getByText('会话外的文字'));
  await settle();
  expect(replyButton()).toBeNull();
  const field = document.createElement('div');
  field.setAttribute('contenteditable', 'true');
  field.textContent = '可以编辑的文字';
  screen.getByLabelText('会话消息').append(field);
  select(field);
  await settle();
  expect(replyButton()).toBeNull();
  // The same passage on its own is offered.
  select(passage);
  expect(await screen.findByRole('button', { name: '回复' })).toBeTruthy();
  field.remove();
});

test('right-clicking the selected text offers 回复 and keeps 复制', async () => {
  const user = userEvent.setup();
  show();
  const passage = await screen.findByText(PASSAGE);
  select(passage);
  await screen.findByRole('button', { name: '回复' });
  fireEvent.contextMenu(passage, { clientX: 320, clientY: 310 });
  const menu = screen.getByRole('menu', { name: '所选内容的操作' });
  expect(
    within(menu)
      .getAllByRole('menuitem')
      .map((item) => item.textContent),
  ).toEqual(['回复', '复制']);
  await user.click(within(menu).getByRole('menuitem', { name: '复制' }));
  await waitFor(() => expect(writeClipboard).toHaveBeenCalledWith({ text: PASSAGE }));
  // The menu key gives no pointer: the menu opens under the selection.
  select(passage);
  await screen.findByRole('button', { name: '回复' });
  fireEvent.contextMenu(document.body);
  const keyed = screen.getByRole('menu', { name: '所选内容的操作' });
  expect([keyed.style.left, keyed.style.top]).toEqual(['200px', '340px']);
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('menu')).toBeNull();
  select(passage);
  await screen.findByRole('button', { name: '回复' });
  fireEvent.contextMenu(passage, { clientX: 320, clientY: 310 });
  await user.click(screen.getByRole('menuitem', { name: '回复' }));
  const card = screen.getByRole('dialog', { name: '引用回复' });
  expect(within(card).getByText(PASSAGE)).toBeTruthy();
});

test('回复 opens a card with the quote; Enter puts the quote and the reply into the composer', async () => {
  const user = userEvent.setup();
  show();
  select(await screen.findByText(PASSAGE));
  await user.click(await screen.findByRole('button', { name: '回复' }));
  const card = screen.getByRole('dialog', { name: '引用回复' });
  expect(within(card).getByText(PASSAGE).closest('blockquote')).not.toBeNull();
  const box = within(card).getByRole('textbox', { name: '回复' }) as HTMLTextAreaElement;
  expect(document.activeElement).toBe(box);
  expect(replyButton()).toBeNull();
  await user.keyboard('第一句{Shift>}{Enter}{/Shift}第二句');
  expect(box.value).toBe('第一句\n第二句');
  expect(quotes()).toEqual([]);
  await user.keyboard('{Enter}');
  expect(screen.queryByRole('dialog', { name: '引用回复' })).toBeNull();
  expect(quotes()).toEqual([{ id: expect.any(String), quote: PASSAGE, reply: '第一句\n第二句' }]);
  // The words go on in the composer.
  expect(document.activeElement).toBe(screen.getByRole('textbox', { name: '输入消息' }));
  // Another passage adds another block; a reply may be left empty.
  select(await screen.findByText(ASKED));
  await user.click(await screen.findByRole('button', { name: '回复' }));
  await user.keyboard('{Enter}');
  expect(quotes().map((quote) => [quote.quote, quote.reply])).toEqual([
    [PASSAGE, '第一句\n第二句'],
    [ASKED, ''],
  ]);
});

test('while an input method composes, Enter and Escape are its own', async () => {
  const user = userEvent.setup();
  show();
  select(await screen.findByText(PASSAGE));
  await user.click(await screen.findByRole('button', { name: '回复' }));
  const box = screen.getByRole('textbox', { name: '回复' });
  fireEvent.keyDown(box, { key: 'Enter', isComposing: true });
  fireEvent.keyDown(box, { key: 'Escape', isComposing: true });
  expect(screen.getByRole('dialog', { name: '引用回复' })).toBeTruthy();
  expect(quotes()).toEqual([]);
});

test('Escape cancels and gives the selection back; so does a press outside the card', async () => {
  const user = userEvent.setup();
  show();
  const passage = await screen.findByText(PASSAGE);
  select(passage);
  await user.click(await screen.findByRole('button', { name: '回复' }));
  await user.keyboard('不要了');
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('dialog', { name: '引用回复' })).toBeNull();
  expect(quotes()).toEqual([]);
  expect(document.getSelection()?.toString()).toBe(PASSAGE);
  await user.click(await screen.findByRole('button', { name: '回复' }));
  const card = screen.getByRole('dialog', { name: '引用回复' });
  await user.keyboard('也不要了');
  fireEvent.pointerDown(within(card).getByRole('textbox', { name: '回复' }));
  expect(screen.getByRole('dialog', { name: '引用回复' })).toBe(card);
  fireEvent.pointerDown(screen.getByText('会话外的文字'));
  expect(screen.queryByRole('dialog', { name: '引用回复' })).toBeNull();
  expect(quotes()).toEqual([]);
});
