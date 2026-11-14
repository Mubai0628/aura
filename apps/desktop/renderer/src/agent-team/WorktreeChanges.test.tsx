import type { AgentTeamMemberView, ExecutionApprovalView } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ApprovalPanel } from '../agent-session/ApprovalPanel';
import { AgentRecord } from './AgentRecord';
import { resetAgentTeamsForTests } from './team-store';
import { WorktreeChanges } from './WorktreeChanges';

// A sub-agent working in its own worktree: its branch where it is shown, with 看改动, and the
// same changes on the card that asks before its worktree is merged or discarded.

const list = vi.fn();
const changes = vi.fn();
const getApproval = vi.fn();
beforeEach(() => {
  list.mockReset();
  changes.mockReset();
  getApproval.mockReset();
  class QuietObserver {
    observe(): void {
      /* nothing is ever in view */
    }
    disconnect(): void {
      /* nothing to release */
    }
  }
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: QuietObserver,
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listAgentTeam: list,
        readAgentTeamChanges: changes,
        getAgentApproval: getApproval,
        stopAgentTeamMember: vi.fn(),
        readAgentHistory: vi.fn().mockReturnValue(new Promise(() => {})),
        readAgentToolActivity: vi.fn().mockResolvedValue({ ok: true, value: { calls: [] } }),
      },
    },
  });
});
afterEach(() => {
  cleanup();
  resetAgentTeamsForTests();
});

const view = (more: Record<string, unknown> = {}) => ({
  ok: true,
  value: {
    agent: 'A1',
    branch: 'aura/agent/0a1b2c3d',
    state: 'active',
    files: [
      { path: 'src/lib.rs', kind: 'modified', added: 12, removed: 3 },
      { path: 'docs/new.md', kind: 'added', added: 40 },
      { path: 'old.txt', kind: 'deleted' },
    ],
    checks: ['cargo test -p demo: exit 0'],
    remaining: '文档还没更新',
    size_bytes: 3 * 1024 ** 3,
    ...more,
  },
});

const member = (worktree: Record<string, unknown>) =>
  ({
    agent: 'A1',
    name: '改代码',
    session_id: 'ses_A1',
    model_profile_id: 'mpr_flash',
    model: 'DeepSeek V4.1 Flash',
    state: 'reported',
    working: false,
    last_report: 'ready',
    approvals: [],
    tokens: 0,
    worktree,
  }) as unknown as AgentTeamMemberView;

async function shown(of: AgentTeamMemberView) {
  render(<AgentRecord parentSessionId="ses_main" member={of} />);
  await act(async () => {});
  return screen.getByRole('region', { name: `子 Agent 记录：${of.name}` });
}

test('a sub-agent in a worktree shows its branch, and 看改动 lists the files and its checks', async () => {
  changes.mockResolvedValue(view());
  const record = await shown(
    member({ branch: 'aura/agent/0a1b2c3d', state: 'active', size_bytes: 1024 ** 3 }),
  );
  expect(record.textContent).toContain('aura/agent/0a1b2c3d');
  expect(changes).not.toHaveBeenCalled();
  fireEvent.click(within(record).getByRole('button', { name: '看改动' }));
  const facts = await within(record).findByLabelText('工作副本的改动');
  expect(changes).toHaveBeenCalledWith({ session_id: 'ses_main', agent: 'A1' });
  for (const part of [
    'src/lib.rs',
    '+12',
    '-3',
    '新增',
    'docs/new.md',
    '删除',
    'old.txt',
    'cargo test -p demo: exit 0',
    '文档还没更新',
    '3.00 GB',
  ])
    expect(facts.textContent).toContain(part);
});

test('a merged worktree says so and has nothing left to look at', async () => {
  const record = await shown(member({ branch: 'aura/agent/0a1b2c3d', state: 'merged' }));
  expect(record.textContent).toContain('已合回');
  expect(within(record).queryByRole('button', { name: '看改动' })).toBeNull();
});

test("the conversation's own worktree is read without naming a sub-agent", async () => {
  changes.mockResolvedValue(view({ agent: undefined, files: [] }));
  render(<WorktreeChanges sessionId="ses_main" />);
  expect(await screen.findByText('没有改动')).toBeTruthy();
  expect(changes).toHaveBeenCalledWith({ session_id: 'ses_main' });
});

test('changes that cannot be read say so', async () => {
  changes.mockResolvedValue({
    ok: false,
    error: { code: 'agents.no_worktree', message: '这个会话没有工作副本。' },
  });
  render(<WorktreeChanges sessionId="ses_main" />);
  expect((await screen.findByRole('alert')).textContent).toContain('没有工作副本');
});

test('the card asking to merge a worktree shows its changes and checks', async () => {
  getApproval.mockResolvedValue({
    ok: true,
    value: {
      approval: {
        approval_id: 'apr_merge',
        revision: '1',
        state: 'pending',
        expires_at: '2026-11-07T01:00:00Z',
        allowed_decisions: ['allow_once', 'deny'],
      },
      operation: {
        content: {
          action_kind: 'agents.merge',
          action: { agent: 'A1' },
          resources: [],
          capabilities: ['agent_task_write', 'git_write', 'git_destructive'],
          purpose: '合回 A1 的改动',
        },
      },
      backend: { id: 'agent-core-broker-v1', class: 'core', guarantees: [] },
      mode: 'workspace_read_write',
      reasons: [],
      requested_capabilities: ['git_destructive'],
    } as unknown as ExecutionApprovalView,
  });
  changes.mockResolvedValue(view());
  render(<ApprovalPanel sessionId="ses_main" approvalId="apr_merge" onChanged={() => {}} />);
  expect(await screen.findByText('合回工作副本')).toBeTruthy();
  const shown = await screen.findByLabelText('工作副本的改动');
  expect(changes).toHaveBeenCalledWith({ session_id: 'ses_main', agent: 'A1' });
  expect(shown.textContent).toContain('src/lib.rs');
  expect(shown.textContent).toContain('cargo test -p demo: exit 0');
});
