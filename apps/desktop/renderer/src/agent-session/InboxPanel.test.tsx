import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { AgentInboxProjection, AgentPendingMessage } from '@aura/contracts/generated';
import { InboxPanel } from './InboxPanel';

// A message sent while a task runs waits in the queue; there it can be
// appended to the running task at once, edited or deleted.

const message = (id: string, text: string, extra: Partial<AgentPendingMessage> = {}) =>
  ({
    message_id: `evt_${id}`,
    created_at: '2026-10-19T00:00:00Z',
    destination: { lane: 'next_turn' },
    text,
    ...extra,
  }) as AgentPendingMessage;
const inbox = (messages: AgentPendingMessage[]) =>
  ({ session_id: 'ses_1', revision: '3', paused: false, messages }) as AgentInboxProjection;
const mutate = vi.fn();
beforeEach(() => {
  mutate.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { mutateAgentInbox: mutate } },
  });
});
afterEach(cleanup);

const item = (text: string) => screen.getByText(text).closest('li') as HTMLElement;

test('a queued message is appended to the running task at once, edited or deleted', async () => {
  const user = userEvent.setup();
  render(
    <InboxPanel
      inbox={inbox([message('a', '补充：顺便跑测试')])}
      activeTurn="trn_1"
      onChanged={vi.fn()}
    />,
  );
  const row = item('补充：顺便跑测试');
  await user.click(within(row).getByRole('button', { name: '立即追加' }));
  expect(mutate).toHaveBeenLastCalledWith({
    session_id: 'ses_1',
    expected_revision: '3',
    action: { action: 'append', message_id: 'evt_a', turn_task_id: 'trn_1' },
  });
  await user.click(within(row).getByRole('button', { name: '删除' }));
  expect(mutate).toHaveBeenLastCalledWith({
    session_id: 'ses_1',
    expected_revision: '3',
    action: { action: 'remove', message_id: 'evt_a' },
  });
  await user.click(within(row).getByRole('button', { name: '编辑' }));
  expect(screen.getByRole('textbox', { name: '编辑排队消息' })).toBeTruthy();
});

test('a message with a chosen Skill waits for the next turn: it cannot be appended', () => {
  render(
    <InboxPanel
      inbox={inbox([
        message('s', '按笔记格式回答', {
          skills: [
            {
              name: 'notes',
              fingerprint: 'f',
              origin: 'agent_global',
              skill_id: 'skl_1',
            } as never,
          ],
        }),
      ])}
      activeTurn="trn_1"
      onChanged={vi.fn()}
    />,
  );
  const append = within(item('按笔记格式回答')).getByRole('button', { name: '立即追加' });
  expect(append).toHaveProperty('disabled', true);
});

// One line per message, read
// in full in place; every action an icon that keeps its name for screen readers and its tooltip.
test('the queue is a compact tray: one line per message, icons for actions, opened to read', async () => {
  const user = userEvent.setup();
  render(
    <InboxPanel
      inbox={inbox([message('a', '第一行\n\n第二段很长的说明'), message('b', '下一轮再做')])}
      activeTurn="trn_1"
      onChanged={vi.fn()}
    />,
  );
  const tray = screen.getByRole('region', { name: '消息队列' });
  const first = within(tray).getByRole('button', { name: '第一行 第二段很长的说明' });
  expect(first.getAttribute('aria-expanded')).toBe('false');
  const actions = within(tray).getAllByRole('button', {
    name: /^(编辑|删除|立即追加|暂停队列)$/u,
  });
  // The queue's pause, and three for each message.
  expect(actions).toHaveLength(7);
  for (const action of actions) {
    expect(action.textContent).toBe('');
    expect(action.getAttribute('title')).toBeTruthy();
  }
  // 删除 is a bin, no longer the × of closing.
  for (const remove of within(tray).getAllByRole('button', { name: '删除' }))
    expect(remove.querySelector('svg')?.getAttribute('data-icon')).toBe('trash');
  expect(first.closest('li')?.textContent).toContain('下一轮 · Normal');
  await user.click(first);
  expect(first.getAttribute('aria-expanded')).toBe('true');
  expect(first.closest('li')?.querySelector('.agent-inbox-full')?.textContent).toContain(
    '第二段很长的说明',
  );
  await user.click(first);
  expect(first.closest('li')?.querySelector('.agent-inbox-full')).toBeNull();
});
