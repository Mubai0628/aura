import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';
afterEach(cleanup);

function install() {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            mode: 'read_only',
            backend: { id: 'agent-core-broker-v1', class: 'core_broker', guarantees: [] },
            operation: {
              content: {
                action_kind: 'workspace_init.complete',
                action: {
                  input: { project_structure: 'the raw submission must not be dumped' },
                  files: [
                    {
                      path: 'docs/agent-guides/style.md',
                      content: '# 代码风格\n\n使用 rustfmt。\n',
                      previous: null,
                    },
                    {
                      path: 'AGENTS.md',
                      content: '# Repository Guidelines\n\n## Project Structure\n',
                      previous: '# 旧的说明\n',
                    },
                  ],
                },
                resources: [],
                capabilities: ['workspace_file_write', 'workspace_file_create'],
                credentials: [],
              },
            },
            requested_capabilities: ['workspace_file_write', 'workspace_file_create'],
            reasons: [],
            approval: {
              state: 'pending',
              expires_at: '2026-10-17T00:00:00Z',
              allowed_decisions: ['allow_once', 'deny'],
            },
          },
        }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
}

test('the /init approval previews the exact files and offers one answer only', async () => {
  install();
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  expect(await screen.findByText(/一次写入以下 2 个文件/u)).toBeTruthy();
  expect(screen.getByText('新建 docs/agent-guides/style.md')).toBeTruthy();
  expect(screen.getByText('替换 AGENTS.md')).toBeTruthy();
  expect(screen.getByText(/# Repository Guidelines/u)).toBeTruthy();
  expect(screen.getByText(/# 旧的说明/u)).toBeTruthy();
  expect(screen.queryByText(/the raw submission must not be dumped/u)).toBeNull();
  expect(screen.getByRole('button', { name: '仅此一次' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: '本会话允许' })).toBeNull();
  expect(screen.queryByRole('button', { name: '本工作区允许' })).toBeNull();
  expect(screen.getByText(/不会成为本会话或工作区的后续写入授权/u)).toBeTruthy();
});

test('a command save shows its two files, and a removal shows what goes, each answered once', async () => {
  const view = (action_kind: string, action: unknown) => ({
    ok: true,
    value: {
      mode: 'full_access',
      backend: { id: 'agent-core-broker-v1', class: 'core_broker', guarantees: [] },
      operation: {
        content: {
          action_kind,
          action,
          resources: [],
          capabilities: ['workspace_file_write'],
          credentials: [],
        },
      },
      requested_capabilities: ['workspace_file_write'],
      reasons: [],
      approval: {
        state: 'pending',
        expires_at: '2026-10-17T00:00:00Z',
        allowed_decisions: ['allow_once', 'deny'],
      },
    },
  });
  const show = async (action_kind: string, action: unknown) => {
    Object.defineProperty(window, 'aura', {
      configurable: true,
      value: {
        v1: {
          getAgentApproval: async () => view(action_kind, action),
          resolveAgentApproval: async () => ({ ok: true, value: {} }),
        },
      },
    });
    render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  };
  await show('commands.save', {
    input: { name: 'review' },
    created: true,
    files: [
      { path: '.aura/commands/review/command.toml', content: 'name = "review"', previous: null },
      { path: '.aura/commands/review/template.md', content: '请审查 $ARGUMENTS', previous: null },
    ],
  });
  expect(await screen.findByText(/新建工作区命令 \/review/u)).toBeTruthy();
  expect(screen.getByText('新建 .aura/commands/review/template.md')).toBeTruthy();
  expect(screen.getByText(/以后每次保存或删除命令都会再询问/u)).toBeTruthy();
  expect(screen.queryByRole('button', { name: '本会话允许' })).toBeNull();
  cleanup();
  await show('commands.delete', {
    input: { name: 'old', revision: 'r' },
    folder: '.aura/commands/old',
    manifest: 'name = "old"',
    template: '旧模板',
  });
  expect(await screen.findByText(/删除工作区命令 \/old：移除 .aura\/commands\/old/u)).toBeTruthy();
});
