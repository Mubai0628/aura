import type { ApprovalView } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { SessionApprovalTray } from './SessionApprovalTray';

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test('only an expanded legacy tray presents the approval and its focused controls pause it', async () => {
  vi.useFakeTimers();
  const activity = vi.fn(async () => ({
    ok: true,
    value: { deadline_at: new Date(Date.now() + 60_000).toISOString() },
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { updateAgentWait: activity } },
  });
  const approvals = [
    {
      id: 'legacy-request',
      session_id: 'shared-session',
      title: '允许测试操作',
      actual_operation_summary: '写入测试文件',
      allowed_decisions: ['deny'],
    },
  ] as ApprovalView[];
  render(
    <SessionApprovalTray
      sessionId="shared-session"
      security={{ approvals, resolveApproval: vi.fn() }}
    />,
  );
  expect(activity).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: /1 项操作/ }));
  });
  expect(activity).toHaveBeenCalledWith(
    expect.objectContaining({
      target: { kind: 'legacy_approval', approval_id: 'legacy-request' },
      activity: 'present',
    }),
  );
  await act(async () => screen.getByRole('button', { name: '拒绝' }).focus());
  expect(activity).toHaveBeenLastCalledWith(expect.objectContaining({ activity: 'focus' }));
  await act(async () => screen.getByRole('button', { name: /1 项操作/ }).focus());
  expect(activity).toHaveBeenLastCalledWith(expect.objectContaining({ activity: 'blur' }));
});
