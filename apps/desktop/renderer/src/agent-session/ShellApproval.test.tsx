import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';

afterEach(cleanup);
test('S08 shows independent Shell authority, soft network and D2 lifetime', async () => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            mode: 'read_only',
            backend: {
              id: 'windows-user-compat-v1',
              class: 'native',
              guarantees: ['child_process_control', 'bounded_output'],
            },
            operation: {
              content: {
                action_kind: 'shell.cmd',
                action: { script: 'echo test' },
                resources: [],
                capabilities: ['process_execute', 'ordinary_network'],
              },
            },
            requested_capabilities: ['process_execute', 'ordinary_network'],
            reasons: [],
            approval: {
              expires_at: '2026-10-12T00:00:00Z',
              state: 'pending',
              allowed_decisions: ['allow_once', 'allow_session', 'deny'],
              scope: { native_session_consent: { shells: ['powershell', 'cmd'] } },
            },
          },
        }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  expect(
    await screen.findByText(/Shell 以你的 Windows 用户权限运行，不受系统级只读或目录限制/),
  ).toBeTruthy();
  expect(
    screen.getByText(/本次需批准：运行 Shell、Shell 联网。运行、联网与下载的批准分别记录/),
  ).toBeTruthy();
  expect(screen.getByText(/代理环境变量.*未强制/)).toBeTruthy();
  expect(screen.getByText(/Core 重启和模式切换后仍有效/)).toBeTruthy();
  expect(screen.queryByText(/重启 Core 后失效/)).toBeNull();
  expect(screen.queryByText(/目标网站/)).toBeNull();
});

/** A Shell approval that deletes for good shows the lines that delete, and an
 * approval that can be allowed once says nothing of allowing for the session. */
test('a Shell deletion shows the lines that delete and no session allowance', async () => {
  const script = [
    "$profile = Join-Path $env:TEMP 'cdp-profile'",
    'Start-Process chrome.exe -ArgumentList $args',
    'Remove-Item -Path $profile -Recurse -Force -ErrorAction SilentlyContinue',
  ].join('\n');
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            mode: 'workspace_read_write',
            backend: { id: 'windows-user-compat-v1', class: 'native', guarantees: [] },
            operation: {
              content: {
                action_kind: 'shell.powershell',
                action: { script },
                resources: [],
                capabilities: ['process_execute'],
                risks: ['permanent_file_deletion'],
                purpose: 'Launch a separate headless Chrome with CDP',
              },
            },
            requested_capabilities: ['process_execute'],
            reasons: [],
            approval: {
              expires_at: '2026-10-24T13:25:00Z',
              state: 'pending',
              allowed_decisions: ['allow_once', 'deny'],
              scope: {},
            },
          },
        }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  const deletions = await screen.findByRole('list', { name: '删除命令' });
  expect(deletions.textContent).toContain('第 3 行');
  expect(deletions.textContent).toContain('Remove-Item -Path $profile -Recurse -Force');
  expect(deletions.textContent).not.toContain('Start-Process');
  expect(screen.queryByText(/本会话允许」只限本会话/)).toBeNull();
});

/** A command that writes outside the workspace is said before anything else, with the lines
 * that name where, and can be allowed once only; a download alone offers the session answer
 * for later downloads. */
test('a write outside the workspace is said with its lines; a download offers the session answer', async () => {
  const approval = (risks: string[], requested: string[], decisions: string[]) => ({
    getAgentApproval: async () => ({
      ok: true,
      value: {
        mode: 'workspace_read_write',
        backend: { id: 'windows-user-compat-v1', class: 'native', guarantees: [] },
        operation: {
          content: {
            action_kind: 'shell.cmd',
            action: { script: 'echo build\necho x>C:\\Temp\\out.txt' },
            resources: [],
            capabilities: ['process_execute', ...requested],
            risks,
            purpose: 'Save the report',
          },
        },
        requested_capabilities: requested,
        reasons: [],
        approval: {
          expires_at: '2026-10-12T00:00:00Z',
          state: 'pending',
          allowed_decisions: decisions,
          scope: { native_session_consent: { shells: ['powershell', 'cmd'] } },
        },
      },
    }),
    resolveAgentApproval: async () => ({ ok: true, value: {} }),
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: approval(['outside_workspace_write'], ['process_execute'], ['allow_once', 'deny']),
    },
  });
  const { unmount } = render(
    <ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />,
  );
  expect(await screen.findByText(/这条命令会写工作区以外的位置；只能允许这一次/)).toBeTruthy();
  const lines = screen.getByRole('list', { name: '写到工作区以外的命令' });
  expect(lines.textContent).toContain('第 2 行');
  expect(lines.textContent).toContain('C:\\Temp\\out.txt');
  expect(lines.textContent).not.toContain('echo build');
  unmount();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: approval([], ['download_artifact'], ['allow_once', 'allow_session', 'deny']),
    },
  });
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  expect(await screen.findByText(/本次需批准：下载/)).toBeTruthy();
  expect(screen.getByText(/「本会话允许」覆盖本会话之后的下载命令/)).toBeTruthy();
  expect(screen.queryByText(/写工作区以外的位置/)).toBeNull();
});
