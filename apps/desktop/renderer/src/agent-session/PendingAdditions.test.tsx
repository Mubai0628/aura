import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { AgentInboxProjection, AgentPendingMessage } from '@aura/contracts/generated';
import { InboxPanel } from './InboxPanel';
import { PendingAdditions } from './PendingAdditions';

//

const message = (id: string, text: string, lane: 'addition' | 'next_turn') =>
  ({
    message_id: `evt_${id}`,
    created_at: '2026-10-24T00:00:00Z',
    destination: lane === 'addition' ? { lane, turn_task_id: 'trn_1' } : { lane },
    text,
  }) as AgentPendingMessage;
const inbox = {
  session_id: 'ses_1',
  revision: '7',
  paused: false,
  messages: [
    message('a', '顺便把测试也跑一下', 'addition'),
    message('o', '另一个任务的', 'addition'),
    message('n', '下一轮再做', 'next_turn'),
  ],
} as AgentInboxProjection;
inbox.messages[1]!.destination = { lane: 'addition', turn_task_id: 'trn_2' } as never;

const mutate = vi.fn();
const writeClipboard = vi.fn();
beforeEach(() => {
  mutate.mockReset().mockResolvedValue({ ok: true, value: {} });
  writeClipboard.mockReset().mockResolvedValue({ ok: true, value: { completed: true } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { mutateAgentInbox: mutate, writeClipboard } },
  });
});
afterEach(cleanup);

test("the running task's appended message waits in the conversation as the user's message", async () => {
  const user = userEvent.setup();
  const changed = vi.fn();
  render(<PendingAdditions inbox={inbox} turnTaskId="trn_1" onChanged={changed} />);
  const waiting = screen.getByText('顺便把测试也跑一下').closest('article') as HTMLElement;
  expect(waiting.classList.contains('agent-user-message')).toBe(true);
  expect(within(waiting).getByText('追加信息')).toBeTruthy();
  expect(within(waiting).getByRole('status', { name: '等待模型接收' })).toBeTruthy();
  // Another task's addition, and the next turn's message, are not this turn's.
  expect(screen.queryByText('另一个任务的')).toBeNull();
  expect(screen.queryByText('下一轮再做')).toBeNull();

  await user.click(within(waiting).getByRole('button', { name: '复制消息' }));
  expect(writeClipboard).toHaveBeenLastCalledWith({ text: '顺便把测试也跑一下' });
  expect(within(waiting).getByRole('button', { name: '已复制' })).toBeTruthy();

  await user.click(within(waiting).getByRole('button', { name: '撤回到队列' }));
  expect(mutate).toHaveBeenLastCalledWith({
    session_id: 'ses_1',
    expected_revision: '7',
    action: { action: 'revoke', message_id: 'evt_a' },
  });
  expect(changed).toHaveBeenCalled();
});

test('a withdrawal Core refuses says so on the message', async () => {
  const user = userEvent.setup();
  mutate.mockResolvedValue({ ok: false, error: { message: '消息已被模型读取' } });
  render(<PendingAdditions inbox={inbox} turnTaskId="trn_1" onChanged={vi.fn()} />);
  await user.click(screen.getByRole('button', { name: '撤回到队列' }));
  expect(screen.getByRole('alert').textContent).toContain('消息已被模型读取');
});

test('the queue over the composer keeps only what waits for the next turn', () => {
  render(<InboxPanel inbox={inbox} activeTurn="trn_1" onChanged={vi.fn()} />);
  const tray = screen.getByRole('region', { name: '消息队列' });
  expect(within(tray).getByText('下一轮再做')).toBeTruthy();
  expect(within(tray).queryByText('顺便把测试也跑一下')).toBeNull();
  expect(within(tray).getByText(/待处理 · 1/u)).toBeTruthy();
});

test('with only appended messages waiting, there is no queue over the composer', () => {
  const appended = { ...inbox, messages: inbox.messages.slice(0, 1) } as AgentInboxProjection;
  render(<InboxPanel inbox={appended} activeTurn="trn_1" onChanged={vi.fn()} />);
  expect(screen.queryByRole('region', { name: '消息队列' })).toBeNull();
});
