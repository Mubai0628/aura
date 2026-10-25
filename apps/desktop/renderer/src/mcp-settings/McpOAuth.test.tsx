import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { McpServerConfig, McpServerView } from '@aura/contracts/generated';
import { McpSettings } from './McpSettings';

const ADDRESS = 'http://127.0.0.1:7000/auth/authorize?state=abc';
const api = {
  listMcpServers: vi.fn(),
  previewMcpServer: vi.fn(),
  describeMcpServer: vi.fn(),
  saveMcpServer: vi.fn(),
  setMcpSecret: vi.fn(),
  signInMcpServer: vi.fn(),
  signOutMcpServer: vi.fn(),
};
const config: McpServerConfig = {
  id: 'web',
  display_name: 'web 服务器',
  transport: {
    type: 'http',
    url: 'https://mcp.example.com/mcp',
    headers: [],
    auth: { type: 'oauth' },
  },
  enabled: true,
  trusted_fingerprint: 'web-fingerprint',
  startup_timeout_ms: 30_000,
  call_timeout_ms: 120_000,
  available_for_subagents: false,
};
const view = (extra: Partial<McpServerView>): McpServerView => ({
  config,
  state: 'sign_in_required',
  fingerprint: 'web-fingerprint',
  problems: [],
  running_instances: 0,
  sign_in: { signed_in: false, pending: false, authorization_server: null, last_error: null },
  ...extra,
});
const registry = (servers: McpServerView[]) => ({ ok: true, value: { revision: 5, servers } });

beforeEach(() => {
  for (const mock of Object.values(api)) mock.mockReset();
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

test('a server that waits for a sign-in opens it and offers the link to copy', async () => {
  const user = userEvent.setup();
  api.listMcpServers.mockResolvedValue(registry([view({})]));
  api.signInMcpServer.mockResolvedValue({
    ok: true,
    value: { authorization_url: ADDRESS, authorization_server: '127.0.0.1' },
  });
  render(<McpSettings coreReady />);
  const group = await screen.findByRole('group', { name: 'web 服务器 的登录' });
  expect(group.textContent).toContain('需要在浏览器中登录');
  await user.click(within(group).getByRole('button', { name: '在浏览器中登录' }));
  expect(api.signInMcpServer).toHaveBeenCalledWith({ id: 'web' });
  expect(await screen.findByText(/已在浏览器中打开 127\.0\.0\.1 的登录页面/u)).toBeTruthy();
  expect(screen.getByText(ADDRESS, { selector: 'code' })).toBeTruthy();
});

test('a signed-in server can sign out, and a failed sign-in says why', async () => {
  const user = userEvent.setup();
  api.listMcpServers.mockResolvedValue(
    registry([
      view({
        state: 'ready',
        tool_count: 9,
        sign_in: { signed_in: true, pending: false, authorization_server: null, last_error: null },
      }),
    ]),
  );
  api.signOutMcpServer.mockResolvedValue(registry([view({})]));
  render(<McpSettings coreReady />);
  const group = await screen.findByRole('group', { name: 'web 服务器 的登录' });
  expect(group.textContent).toContain('已登录');
  await user.click(within(group).getByRole('button', { name: '退出登录' }));
  expect(api.signOutMcpServer).toHaveBeenCalledWith({ id: 'web' });
  expect(await screen.findByText('已退出 web 服务器 的登录')).toBeTruthy();
  cleanup();
  api.listMcpServers.mockResolvedValue(
    registry([
      view({
        sign_in: {
          signed_in: false,
          pending: false,
          authorization_server: null,
          last_error: '授权服务器拒绝了令牌请求（HTTP 400，invalid_grant）',
        },
      }),
    ]),
  );
  render(<McpSettings coreReady />);
  expect(await screen.findByText(/上次登录没有完成：授权服务器拒绝了令牌请求/u)).toBeTruthy();
});

test('the OAuth choice saves the sign-in way and no secret', async () => {
  const user = userEvent.setup();
  api.listMcpServers.mockResolvedValue(registry([]));
  api.saveMcpServer.mockImplementation(async ({ server }) => registry([view({ config: server })]));
  render(<McpSettings coreReady />);
  await user.click(await screen.findByRole('button', { name: '添加服务器' }));
  const form = screen.getByRole('form', { name: '添加 MCP 服务器' });
  await user.type(within(form).getByLabelText('服务器 ID'), 'web');
  await user.click(within(form).getByLabelText('HTTP 地址（Streamable HTTP）'));
  await user.type(within(form).getByLabelText('地址'), 'https://mcp.example.com/mcp');
  await user.click(within(form).getByLabelText('OAuth 登录（在浏览器中登录）'));
  expect(within(form).queryByLabelText('密钥')).toBeNull();
  // The ways to sign in are one group of choices; the OAuth fields are rows of their own
  // below it, not squeezed into the same line.
  const choices = within(form).getByRole('radio', { name: 'OAuth 登录（在浏览器中登录）' });
  expect(choices.closest('.mcp-choices')?.querySelectorAll('input[type="radio"]')).toHaveLength(4);
  const scopes = within(form).getByLabelText('权限（可选，空格分隔）');
  expect(scopes.closest('.mcp-auth-fields')).not.toBeNull();
  expect(scopes.closest('.mcp-choices')).toBeNull();
  await user.type(scopes, 'repo read:org');
  await user.click(within(form).getByRole('button', { name: '保存' }));
  expect(api.setMcpSecret).not.toHaveBeenCalled();
  expect(api.saveMcpServer).toHaveBeenCalledWith(
    expect.objectContaining({
      server: expect.objectContaining({
        transport: {
          type: 'http',
          url: 'https://mcp.example.com/mcp',
          headers: [],
          auth: { type: 'oauth', scopes: ['repo', 'read:org'] },
        },
      }),
    }),
  );
});
