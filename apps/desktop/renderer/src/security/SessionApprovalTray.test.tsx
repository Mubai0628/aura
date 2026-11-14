import type { ApprovalView } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { SessionApprovalTray } from './SessionApprovalTray';

// An answer from the approval tray that did not take effect says why and what to do.

afterEach(cleanup);

const approval = {
  id: 'apr_1',
  session_id: 'ses',
  title: '运行命令',
  actual_operation_summary: 'git status',
  allowed_decisions: ['allow_once', 'deny'],
  request_revision: '1',
} as unknown as ApprovalView;

const answer = async (reason: unknown) => {
  const resolveApproval = vi.fn().mockRejectedValue(reason);
  render(
    <SessionApprovalTray sessionId="ses" security={{ approvals: [approval], resolveApproval }} />,
  );
  await userEvent.click(screen.getByRole('button', { name: /操作等待你确认/u }));
  await userEvent.click(screen.getByRole('button', { name: '仅此一次' }));
  return (await screen.findByRole('alert')).textContent;
};

test('an approval already answered or expired says the choice did not take effect', async () => {
  for (const code of [
    'approval.stale_request',
    'approval.already_resolved',
    'approval.not_found',
  ]) {
    expect(await answer(Object.assign(new Error('approval request is stale'), { code }))).toBe(
      '这项审批已经处理过或已过期，你的选择没有生效；请看最新的审批。',
    );
    cleanup();
  }
});

test('an answer that did not reach Core says why and that the approval still waits', async () => {
  expect(await answer(new Error('Aura Core pipe disconnected'))).toBe(
    '你的选择没有提交（与 Aura Core 的连接断开了），这项审批仍在等你确认；请再选一次。',
  );
});
