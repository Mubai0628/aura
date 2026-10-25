import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import type { MemorySessionConsentView } from '@aura/contracts/generated';
import { SessionMemoryGrants } from './SessionMemoryGrants';

afterEach(cleanup);
const grant = (id: string, kind: 'global' | 'project', inForce = true) =>
  ({
    grant_id: id,
    grant_revision: '1',
    in_force: inForce,
    issued_at: '2026-10-16T08:00:00Z',
    revoked_at: inForce ? null : '2026-10-16T09:00:00Z',
    scope: {
      session_id: 'session',
      scope: kind === 'global' ? { kind } : { kind, workspace_id: 'workspace' },
    },
  }) as unknown as MemorySessionConsentView;

test('lists each Vault answer and withdraws the exact revision', async () => {
  const list = vi
    .fn()
    .mockResolvedValue({ ok: true, value: [grant('g1', 'project'), grant('g2', 'global', false)] });
  const revoke = vi.fn(async () => {
    list.mockResolvedValue({ ok: true, value: [grant('g1', 'project', false)] });
    return { ok: true, value: grant('g1', 'project', false) };
  });
  Object.assign(window, {
    aura: { v1: { listMemorySessionConsents: list, revokeMemorySessionConsent: revoke } },
  });
  render(<SessionMemoryGrants sessionId="session" open />);
  expect(await screen.findByText(/本项目记忆库 · 写入记忆 · 生效中/u)).toBeTruthy();
  expect(screen.getByText(/全局记忆库 · 写入记忆 · 已撤销/u)).toBeTruthy();
  // The section lists the approvals; their rules were shown when they were given.
  expect(screen.queryByText(/只读会话里选择|记忆库位置改变后/u)).toBeNull();
  expect(list).toHaveBeenCalledWith({ session_id: 'session', include_inactive: true });
  // A withdrawn answer has nothing left to revoke.
  expect(screen.getAllByRole('button', { name: '撤销记忆批准' })).toHaveLength(1);
  await userEvent.setup().click(screen.getByRole('button', { name: '撤销记忆批准' }));
  await waitFor(() =>
    expect(revoke).toHaveBeenCalledWith({
      session_id: 'session',
      grant_id: 'g1',
      expected_revision: '1',
    }),
  );
  expect(await screen.findByText(/本项目记忆库 · 写入记忆 · 已撤销/u)).toBeTruthy();
  expect(screen.queryByRole('button', { name: '撤销记忆批准' })).toBeNull();
});

test('a changed answer is reread instead of being retried blindly', async () => {
  const list = vi.fn().mockResolvedValue({ ok: true, value: [grant('g1', 'global')] });
  const revoke = vi.fn().mockResolvedValue({
    ok: false,
    error: { code: 'permission.grant_state_changed', message: 'changed' },
  });
  Object.assign(window, {
    aura: { v1: { listMemorySessionConsents: list, revokeMemorySessionConsent: revoke } },
  });
  render(<SessionMemoryGrants sessionId="session" open />);
  await userEvent.setup().click(await screen.findByRole('button', { name: '撤销记忆批准' }));
  expect((await screen.findByRole('alert')).textContent).toContain('该批准状态已变化，已重新读取');
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
});

test('a closed dialog reads nothing', () => {
  const list = vi.fn();
  Object.assign(window, { aura: { v1: { listMemorySessionConsents: list } } });
  render(<SessionMemoryGrants sessionId="session" open={false} />);
  expect(list).not.toHaveBeenCalled();
});
