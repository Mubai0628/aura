import type { PeerMailbox } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { ActivePeerWork } from './ActivePeerWork';

const view: PeerMailbox = {
  collaboration_enabled: true,
  budget_default: { limit: 32, revision: 0 },
  review_policy: { required: true, revision: 4 },
  entries: [
    {
      budget: { root_event_id: 'evt_root', limit: 32, used: 1, revision: 0, stopped: false },
      manages_run: false,
      review_text: '原问题',
      delivery: {
        delivery_id: 'evt_question',
        kind: 'question',
        source_session_id: 'ses_other',
        target_session_id: 'ses_current',
        state: 'queued',
      },
    },
  ],
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test('an outgoing delivery shows its quota in the header and all controls in one row', async () => {
  const item = view.entries[0]!;
  const outgoing = {
    ...item,
    manages_run: true,
    review_text: null,
    delivery: {
      ...item.delivery,
      kind: 'dispatch',
      source_session_id: 'ses_current',
      target_session_id: 'ses_other',
      state: 'started',
    },
  } as const;
  vi.stubGlobal('aura', {
    v1: {
      readPeerMailbox: vi
        .fn()
        .mockResolvedValue({ ok: true, value: { ...view, entries: [outgoing] } }),
    },
  });
  render(
    <ActivePeerWork
      sessionId="ses_current"
      names={new Map([['ses_other', '开发']])}
      onChanged={vi.fn()}
    />,
  );
  const header = (await screen.findByText('发往 开发')).closest('header');
  expect(header).not.toBeNull();
  expect(screen.getByText('额度 1 / 32').closest('header')).toBe(header);
  expect(screen.getByText('执行中').closest('header')).toBe(header);
  const actions = screen.getByRole('button', { name: '取消投递' }).parentElement;
  expect(screen.getByRole('button', { name: '增加本次额度' }).parentElement).toBe(actions);
  expect(screen.getByRole('button', { name: '停止本次全部协作' }).parentElement).toBe(actions);
});

test('editing a queued inbound question sends one bounded release, not a new dispatch', async () => {
  const manage = vi.fn().mockResolvedValue({ ok: true, value: { ...view, entries: [] } });
  vi.stubGlobal('aura', {
    v1: {
      readPeerMailbox: vi.fn().mockResolvedValue({ ok: true, value: view }),
      managePeerMailbox: manage,
    },
  });
  render(
    <ActivePeerWork
      sessionId="ses_current"
      names={new Map([['ses_other', '测试']])}
      onChanged={vi.fn()}
    />,
  );
  const editor = await screen.findByLabelText('接收内容');
  fireEvent.change(editor, { target: { value: '' } });
  expect(screen.getByRole('alert').textContent).toContain('请输入接收内容');
  expect(screen.getByRole('button', { name: '确认放行' })).toHaveProperty('disabled', true);
  fireEvent.change(editor, { target: { value: '用户核准后的问题' } });
  fireEvent.click(screen.getByRole('button', { name: '确认放行' }));
  await waitFor(() =>
    expect(manage).toHaveBeenCalledExactlyOnceWith({
      session_id: 'ses_current',
      view: 'active',
      action: { kind: 'release', delivery_id: 'evt_question', revised_text: '用户核准后的问题' },
    }),
  );
});

test('a terminal receipt has no history entry or replay action in the conversation', async () => {
  const item = view.entries[0]!;
  const entries = [
    {
      ...item,
      review_text: null,
      delivery: { ...item.delivery, state: 'unknown', reason: '原操作结果未知，先对账。' },
    },
  ];
  const read = vi.fn().mockResolvedValue({ ok: true, value: { ...view, entries } });
  vi.stubGlobal('aura', { v1: { readPeerMailbox: read } });
  render(<ActivePeerWork sessionId="ses_current" names={new Map()} onChanged={vi.fn()} />);
  await waitFor(() =>
    expect(read).toHaveBeenCalledWith({
      session_id: 'ses_current',
      before: null,
      view: 'active',
    }),
  );
  expect(screen.queryByRole('region', { name: '当前同级协作' })).toBeNull();
});

test('an exhausted active collaboration can still increase its original budget after deliveries settle', async () => {
  const exhausted = {
    delivery_id: 'evt_existing',
    budget: { root_event_id: 'evt_root', limit: 1, used: 1, revision: 3, stopped: false },
  };
  const active = { ...view, entries: [], active_budget: exhausted };
  const read = vi.fn().mockResolvedValue({ ok: true, value: active });
  const manage = vi
    .fn()
    .mockRejectedValueOnce(new Error('transport response lost'))
    .mockResolvedValueOnce({ ok: true, value: { ...view, entries: [] } });
  vi.stubGlobal('aura', { v1: { readPeerMailbox: read, managePeerMailbox: manage } });
  render(<ActivePeerWork sessionId="ses_current" names={new Map()} onChanged={vi.fn()} />);
  fireEvent.click(await screen.findByRole('button', { name: '增加本次额度' }));
  expect(screen.queryByRole('button', { name: '查看结果' })).toBeNull();
  const button = screen.getByRole('button', { name: '保存额度' });
  fireEvent.click(button);
  await screen.findByRole('alert');
  expect(button).toHaveProperty('disabled', true);
  fireEvent.click(button);
  expect(manage).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: '重新读取' }));
  await waitFor(() => expect(button).toHaveProperty('disabled', false));
  fireEvent.click(button);
  expect(manage).toHaveBeenLastCalledWith({
    session_id: 'ses_current',
    view: 'active',
    action: { kind: 'increase', delivery_id: 'evt_existing', expected_revision: 3, limit: 2 },
  });
});
