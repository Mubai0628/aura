import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';
afterEach(cleanup);

function install(deleting: boolean, project: boolean) {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            mode: deleting ? 'full_access' : 'read_only',
            backend: { id: 'memory-vault-broker-v1', class: 'core_broker', guarantees: [] },
            operation: {
              content: {
                action_kind: deleting ? 'memory.delete' : 'memory.update',
                action: {
                  scope: project ? 'project' : 'global',
                  titles: ['构建经验'],
                  input: { body: 'this body must not be dumped' },
                },
                resources: [
                  {
                    scope_kind: 'memory_vault',
                    scope: { kind: project ? 'project' : 'global' },
                    vault_identity: { algorithm: 'blake3', hex: 'b'.repeat(64) },
                  },
                ],
                capabilities: [deleting ? 'memory_delete' : 'memory_write'],
                credentials: [],
              },
            },
            requested_capabilities: [deleting ? 'memory_delete' : 'memory_write'],
            reasons: [],
            approval: {
              state: 'pending',
              expires_at: '2026-10-17T00:00:00Z',
              allowed_decisions: deleting
                ? ['allow_once', 'deny']
                : ['allow_once', 'allow_session', 'deny'],
            },
          },
        }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
}
test('ReadOnly memory approval names its global Vault and bound session range', async () => {
  install(false, false);
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  expect(await screen.findByText('全局记忆')).toBeTruthy();
  expect(screen.getByText('构建经验')).toBeTruthy();
  expect(screen.getByRole('button', { name: '本会话允许' })).toBeTruthy();
  expect(screen.queryByText('this body must not be dumped')).toBeNull();
});
test('deletion names the project Vault and never offers session or workspace consent', async () => {
  install(true, true);
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  expect(await screen.findByText('永久删除记忆文件')).toBeTruthy();
  expect(screen.getByText('当前项目记忆')).toBeTruthy();
  expect(screen.getByRole('button', { name: '仅此一次' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: '本会话允许' })).toBeNull();
  expect(screen.queryByRole('button', { name: '本工作区允许' })).toBeNull();
  // The card does not mention 「本会话允许」 when it does not offer it.
  expect(screen.queryByText(/「本会话允许」/u)).toBeNull();
  expect(screen.getByText('这类操作每次都会询问；批准只覆盖这一次调用。')).toBeTruthy();
});
