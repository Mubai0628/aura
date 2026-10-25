import type { ExecutionApprovalView } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';

const get = vi.fn();
beforeEach(() => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { getAgentApproval: get } },
  });
});
afterEach(cleanup);

function approval(action: string, input: unknown): ExecutionApprovalView {
  return {
    approval: {
      approval_id: 'apr_fixture',
      revision: '1',
      state: 'pending',
      expires_at: '2026-10-15T01:00:00Z',
      allowed_decisions: ['allow_once', 'deny'],
    },
    operation: {
      content: { action_kind: action, action: input, resources: [], capabilities: ['git_write'] },
    },
    backend: {
      id: 'agent-native-git-v1',
      class: 'native',
      guarantees: ['child_process_control', 'bounded_output'],
    },
    mode: 'read_only',
    reasons: [],
    requested_capabilities: ['git_write'],
  } as unknown as ExecutionApprovalView;
}

test('a Git commit approval states the selected files and truthful Native execution scope', async () => {
  get.mockResolvedValue({
    ok: true,
    value: approval('git.commit', {
      message: '修复本机夹具',
      paths: ['汉字 space.txt'],
      staged_only: false,
    }),
  });
  render(<ApprovalPanel sessionId="fixture" approvalId="approval" onChanged={() => {}} />);
  expect(await screen.findByText('与在终端运行 git 相同，Aura 不隔离它的文件写入')).toBeTruthy();
  expect(screen.getByText('修复本机夹具')).toBeTruthy();
  expect(screen.getByText('汉字 space.txt')).toBeTruthy();
  expect(screen.getByText(/不跳过 hook/)).toBeTruthy();
  expect(screen.queryByText(/覆盖这一类操作在本会话内的后续调用/)).toBeNull();
});

test('a branch approval shows its start and switch choice without claiming file isolation', async () => {
  get.mockResolvedValue({
    ok: true,
    value: approval('git.branch', {
      kind: 'create',
      name: 'feature/local',
      start: 'main',
      switch: true,
    }),
  });
  render(<ApprovalPanel sessionId="fixture" approvalId="approval" onChanged={() => {}} />);
  expect(await screen.findByText('新建并切换分支')).toBeTruthy();
  expect(screen.getByText('feature/local')).toBeTruthy();
  expect(screen.getByText('main')).toBeTruthy();
  expect(screen.getByText('与在终端运行 git 相同，Aura 不隔离它的文件写入')).toBeTruthy();
});
