import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';

afterEach(cleanup);

const resolve = vi.fn(async () => ({ ok: true, value: {} }));

function install(decisions: string[], transport?: Record<string, unknown>) {
  resolve.mockClear();
  const servers = transport
    ? [{ config: { id: 'files', transport }, fingerprint: 'c'.repeat(64), state: 'ready' }]
    : [];
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            mode: 'read_only',
            backend: {
              id: 'agent-mcp-v1',
              class: 'core_broker',
              guarantees: ['authorized_writes_only', 'credential_isolation'],
            },
            operation: {
              content: {
                action_kind: 'mcp.files.echo',
                action: { text: '你好' },
                resources: [
                  {
                    scope_kind: 'mcp_server',
                    server_id: 'files',
                    config_fingerprint: { algorithm: 'blake3', hex: 'c'.repeat(64) },
                  },
                ],
                capabilities: ['external_extension_call'],
                credentials: [],
              },
            },
            requested_capabilities: ['external_extension_call'],
            reasons: ['policy.approval_required'],
            approval: {
              revision: 1,
              expires_at: '2026-10-17T00:00:00Z',
              state: 'pending',
              allowed_decisions: decisions,
            },
          },
        }),
        resolveAgentApproval: resolve,
        ...(transport
          ? { listMcpServers: async () => ({ ok: true, value: { revision: 1, servers } }) }
          : {}),
      },
    },
  });
}

test('an MCP call says the server runs as the user and what a turn answer covers', async () => {
  const user = userEvent.setup();
  install(['allow_for_current_turn', 'deny']);
  render(<ApprovalPanel sessionId="ses" approvalId="apr" onChanged={() => undefined} />);
  const facts = await screen.findByRole('region', { name: 'MCP 工具调用' });
  expect(facts.textContent).toContain('MCP 服务器「files」的工具 mcp.files.echo');
  expect(facts.textContent).toContain('配置指纹 cccccccccccc');
  expect(facts.textContent).toContain('以你的身份运行');
  expect(facts.textContent).toContain('作为外部内容交给模型');
  expect(facts.textContent).toContain('「本轮允许」覆盖这个服务器在本轮内的后续调用');
  expect(within(facts).getByText('调用参数')).toBeTruthy();
  // The generic session-reuse sentence does not apply to an MCP answer.
  expect(screen.queryByText(/「本会话允许」只覆盖这一类操作/u)).toBeNull();
  const buttons = screen
    .getAllByRole('button')
    .map((button) => button.textContent)
    .filter((text) => text === '本轮允许' || text === '拒绝' || text === '仅此一次');
  expect(buttons).toEqual(['本轮允许', '拒绝']);
  await user.click(screen.getByRole('button', { name: '本轮允许' }));
  expect(resolve).toHaveBeenCalledWith(
    expect.objectContaining({ decision: 'allow_for_current_turn', expected_revision: 1 }),
  );
});

test('an HTTP server says where the call goes instead of running as the user', async () => {
  install(['allow_for_current_turn', 'deny'], {
    type: 'http',
    url: 'https://mcp.example.com/mcp',
    headers: [],
  });
  render(<ApprovalPanel sessionId="ses" approvalId="apr" onChanged={() => undefined} />);
  const facts = await screen.findByRole('region', { name: 'MCP 工具调用' });
  await screen.findByText('https://mcp.example.com/mcp');
  expect(facts.textContent).toContain('经 Aura 的网络网关发到 https://mcp.example.com/mcp');
  expect(facts.textContent).toContain('不跟随跳转');
  expect(facts.textContent).not.toContain('以你的身份运行');
});
