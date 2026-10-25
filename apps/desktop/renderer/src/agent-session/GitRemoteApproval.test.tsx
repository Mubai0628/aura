import type { ExecutionApprovalView } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { GitRemoteApprovalFacts } from './GitRemoteApprovalFacts';
afterEach(cleanup);
function view(force: boolean): ExecutionApprovalView {
  return {
    operation: {
      content: {
        action_kind: 'git.push',
        resources: [
          {
            scope_kind: 'git_remote_destination',
            facts: {
              remote: 'origin',
              local_branch: 'main',
              target: { remote_url: 'https://fixture.invalid/repo.git', remote_branch: 'review' },
              outgoing_count: 3,
              outgoing_subjects: ['修复一', '修复二'],
              force_with_lease: force ? 'a'.repeat(40) : null,
            },
          },
        ],
      },
    },
  } as unknown as ExecutionApprovalView;
}
test('push approval shows the resolved destination, direction and last-fetch summary', () => {
  render(<GitRemoteApprovalFacts view={view(false)} />);
  expect(screen.getByText('origin · https://fixture.invalid/repo.git')).toBeTruthy();
  expect(screen.getByText('main → review')).toBeTruthy();
  expect(screen.getByText('3 个（按最近一次 fetch 计算）')).toBeTruthy();
  expect(screen.getByText('修复一')).toBeTruthy();
  expect(screen.getByText(/只覆盖这个物理仓库/)).toBeTruthy();
  expect(screen.getByText(/Git 自己联网/)).toBeTruthy();
});
test('force approval displays the exact lease and excludes session reuse', () => {
  render(<GitRemoteApprovalFacts view={view(true)} />);
  expect(screen.getByText('a'.repeat(40))).toBeTruthy();
  expect(screen.getByText(/每次单独批准，不能使用会话批准/)).toBeTruthy();
  expect(screen.queryByText(/只覆盖这个物理仓库/)).toBeNull();
});
