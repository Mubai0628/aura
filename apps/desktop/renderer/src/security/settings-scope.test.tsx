import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { PermissionDefaultsCard } from './PermissionDefaultsCard';
import { SessionApprovalTray } from './SessionApprovalTray';

afterEach(cleanup);
test('default permission saves to Core without changing a session', async () => {
  const getPermissionDefaults = vi
    .fn()
    .mockResolvedValue({ ok: true, value: { revision: 4, mode: 'read_only' } });
  const setPermissionDefaults = vi
    .fn()
    .mockResolvedValue({ ok: true, value: { revision: 5, mode: 'workspace_read_write' } });
  const setSessionPermissionMode = vi.fn();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { getPermissionDefaults, setPermissionDefaults, setSessionPermissionMode } },
  });
  const notify = vi.fn();
  render(<PermissionDefaultsCard onNotify={notify} />);
  await waitFor(() =>
    expect(screen.getByRole<HTMLInputElement>('radio', { name: /只读/u }).checked).toBe(true),
  );
  // No explanation on the card; the save notice says only new sessions change.
  expect(screen.queryByText(/为新会话选择起始权限|单个会话的权限与审批/u)).toBeNull();
  await userEvent.click(screen.getByRole('radio', { name: /工作区读写/u }));
  await userEvent.click(screen.getByRole('button', { name: '保存默认权限' }));
  await waitFor(() => expect(notify).toHaveBeenCalledTimes(1));
  expect(setPermissionDefaults).toHaveBeenCalledWith({
    expected_revision: 4,
    mode: 'workspace_read_write',
  });
  expect(setSessionPermissionMode).not.toHaveBeenCalled();
});

test('approval tray is scoped, initially compact, and disappears without session pending work', async () => {
  const resolveApproval = vi.fn().mockResolvedValue(undefined);
  const approval = {
    id: 'approval',
    session_id: 'session-a',
    title: '修改文件',
    actual_operation_summary: '修改工作区中的 hello.txt',
    source_summary: 'fixture',
    allowed_decisions: ['allow_once', 'deny'] as const,
    request_revision: 3,
    created_at: '2026-09-22T00:00:00Z',
    risk_flags: [],
  };
  const approvals = [
    { ...approval, allowed_decisions: [...approval.allowed_decisions] },
    {
      ...approval,
      id: 'other',
      session_id: 'session-b',
      title: '不属于此会话',
      allowed_decisions: [...approval.allowed_decisions],
    },
  ];
  const view = render(
    <SessionApprovalTray sessionId="session-a" security={{ approvals, resolveApproval }} />,
  );
  expect(screen.getByText('1 项操作等待你确认')).toBeTruthy();
  expect(screen.queryByText('修改文件')).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: '1 项操作等待你确认' }));
  expect(screen.queryByText('不属于此会话')).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: '仅此一次' }));
  expect(resolveApproval).toHaveBeenCalledWith(approvals[0], 'allow_once');
  view.rerender(
    <SessionApprovalTray
      sessionId="session-a"
      security={{ approvals: approvals.slice(1), resolveApproval }}
    />,
  );
  expect(screen.queryByText(/等待你确认/u)).toBeNull();
});
