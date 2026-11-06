import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import type { AgentInboxProjection, AgentPendingMessage } from '@aura/contracts/generated';

import { InboxPanel } from '../InboxPanel';
import { PendingAdditions } from '../PendingAdditions';
import { SendingMessage } from '../SendingMessage';
import { quoteParts, withoutQuoteMarks } from './quote-parts';

/** A message with quotes, shown before the conversation has it: its quotes look like quotes. */
const QUOTED = '> 第一段\n>\n> 第二段\n\n这里改一下\n\n顺便跑测试';

afterEach(cleanup);

test('a message is split into its quotes and its words; one without quotes is not', () => {
  expect(quoteParts(QUOTED)).toEqual([
    { quote: true, text: '第一段\n\n第二段' },
    { quote: false, text: '这里改一下' },
    { quote: false, text: '顺便跑测试' },
  ]);
  expect(quoteParts('没有引用\n\n> 不在开头也算')).toEqual([
    { quote: false, text: '没有引用' },
    { quote: true, text: '不在开头也算' },
  ]);
  expect(quoteParts('a > b\n\nc')).toBeUndefined();
  expect(withoutQuoteMarks(QUOTED)).toBe('第一段\n\n第二段\n\n这里改一下\n\n顺便跑测试');
  expect(withoutQuoteMarks('a > b')).toBe('a > b');
});

test('the message being sent shows its quote as a quote', () => {
  const { container } = render(
    <SendingMessage sending={{ text: QUOTED, files: [], accepted: false }} />,
  );
  expect(container.querySelector('blockquote')?.textContent).toBe('第一段\n\n第二段');
  expect(container.textContent).toContain('这里改一下');
  expect(container.textContent).not.toContain('>');
});

const pending = (text: string, destination: AgentPendingMessage['destination']) =>
  ({
    message_id: 'evt_1',
    created_at: '2026-11-05T00:00:00Z',
    destination,
    text,
  }) as AgentPendingMessage;

test('an addition waiting for the model shows its quote as a quote', () => {
  const inbox = {
    session_id: 'ses_1',
    revision: '1',
    paused: false,
    messages: [pending(QUOTED, { lane: 'addition', turn_task_id: 'trn_1' })],
  } as AgentInboxProjection;
  const { container } = render(
    <PendingAdditions inbox={inbox} turnTaskId="trn_1" onChanged={vi.fn()} />,
  );
  expect(container.querySelector('blockquote')?.textContent).toBe('第一段\n\n第二段');
  expect(container.textContent).not.toContain('>');
});

test('a queued message’s one line leaves the quote marks out', () => {
  const inbox = {
    session_id: 'ses_1',
    revision: '1',
    paused: false,
    messages: [pending(QUOTED, { lane: 'next_turn' })],
  } as AgentInboxProjection;
  render(<InboxPanel activeTurn={undefined} inbox={inbox} onChanged={vi.fn()} />);
  const line = screen.getByRole('button', { name: /第一段/u });
  expect(line.textContent).toContain('第一段 第二段 这里改一下 顺便跑测试');
  expect(line.textContent).not.toContain('>');
});
