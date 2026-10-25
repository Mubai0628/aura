import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { McpServerConfig, McpServerView } from '@aura/contracts/generated';
import { McpSettings } from './McpSettings';

const SECRET = 'typed-secret-value-0001';
const SECRET_ID = 'crd_01HZY3W5Q9G7X2V4B6N8M0K1P3';

const api = {
  listMcpServers: vi.fn(),
  previewMcpServer: vi.fn(),
  describeMcpServer: vi.fn(),
  saveMcpServer: vi.fn(),
  setMcpSecret: vi.fn(),
};
const registry = (servers: McpServerView[], revision = 5) => ({
  ok: true,
  value: { revision, servers },
});
const view = (config: McpServerConfig): McpServerView => ({
  config,
  state: 'not_trusted',
  fingerprint: `${config.id}-fingerprint`,
  problems: [],
  running_instances: 0,
});
const httpConfig = (auth?: McpServerConfig['transport']): McpServerConfig => ({
  id: 'web',
  display_name: 'web 服务器',
  transport: auth ?? { type: 'http', url: 'https://mcp.example.com/mcp', headers: [] },
  enabled: false,
  trusted_fingerprint: null,
  startup_timeout_ms: 30_000,
  call_timeout_ms: 120_000,
  available_for_subagents: false,
});

beforeEach(() => {
  for (const mock of Object.values(api)) mock.mockReset();
  api.listMcpServers.mockResolvedValue(registry([]));
  api.saveMcpServer.mockImplementation(async ({ server }) => registry([view(server)], 6));
  api.setMcpSecret.mockResolvedValue({ ok: true, value: { secret_id: SECRET_ID } });
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

async function addForm() {
  const user = userEvent.setup();
  render(<McpSettings coreReady />);
  await user.click(await screen.findByRole('button', { name: '添加服务器' }));
  const form = screen.getByRole('form', { name: '添加 MCP 服务器' });
  await user.type(within(form).getByLabelText('服务器 ID'), 'web');
  return { user, form };
}

test('an HTTP secret goes only on its own channel; the configuration names its ID', async () => {
  const { user, form } = await addForm();
  await user.click(within(form).getByLabelText('HTTP 地址（Streamable HTTP）'));
  await user.type(within(form).getByLabelText('地址'), 'https://mcp.example.com/mcp');
  await user.click(within(form).getByLabelText('密钥（Authorization: Bearer）'));
  const field = within(form).getByLabelText('密钥') as HTMLInputElement;
  expect(field.type).toBe('password');
  await user.type(field, SECRET);
  await user.click(within(form).getByRole('button', { name: '保存' }));
  expect(api.setMcpSecret).toHaveBeenCalledTimes(1);
  const [request, bytes] = api.setMcpSecret.mock.calls[0] as [unknown, Uint8Array];
  expect(request).toEqual({ server_id: 'web', secret_id: null });
  expect(new TextDecoder().decode(bytes)).toBe(SECRET);
  expect(field.value).toBe(''); // cleared once the bytes are taken
  expect(api.saveMcpServer).toHaveBeenCalledWith(
    expect.objectContaining({
      server: expect.objectContaining({
        transport: {
          type: 'http',
          url: 'https://mcp.example.com/mcp',
          headers: [],
          auth: { type: 'secret', secret_id: SECRET_ID },
        },
      }),
    }),
  );
  expect(JSON.stringify(api.saveMcpServer.mock.calls)).not.toContain(SECRET);
});

test('a secret the server needs is asked for before anything is saved', async () => {
  const { user, form } = await addForm();
  await user.click(within(form).getByLabelText('HTTP 地址（Streamable HTTP）'));
  await user.type(within(form).getByLabelText('地址'), 'https://mcp.example.com/mcp');
  await user.click(within(form).getByLabelText('密钥（放在指定的请求头）'));
  await user.type(within(form).getByLabelText('放密钥的请求头'), 'X-Api-Key');
  await user.click(within(form).getByRole('button', { name: '保存' }));
  expect(await within(form).findByText('请填写认证密钥')).toBeTruthy();
  expect(api.setMcpSecret).not.toHaveBeenCalled();
  expect(api.saveMcpServer).not.toHaveBeenCalled();
});

test('editing keeps the saved secret unless a new one is typed', async () => {
  const user = userEvent.setup();
  const signed = httpConfig({
    type: 'http',
    url: 'https://mcp.example.com/mcp',
    headers: [],
    auth: { type: 'secret', secret_id: SECRET_ID, header: 'X-Api-Key' },
  });
  api.listMcpServers.mockResolvedValue(registry([view(signed)]));
  render(<McpSettings coreReady />);
  await user.click(await screen.findByRole('button', { name: '编辑' }));
  const form = screen.getByRole('form', { name: '编辑 web 服务器' });
  expect((within(form).getByLabelText('密钥') as HTMLInputElement).placeholder).toContain('已设置');
  await user.click(within(form).getByRole('button', { name: '保存' }));
  expect(api.setMcpSecret).not.toHaveBeenCalled();
  expect(api.saveMcpServer).toHaveBeenCalledWith(
    expect.objectContaining({
      server: expect.objectContaining({ transport: signed.transport }),
      replaces: 'web',
    }),
  );
});

test("a program's secret variable is written on the secret channel and named by ID", async () => {
  const { user, form } = await addForm();
  await user.type(within(form).getByLabelText('命令'), 'C:\\Tools\\server.exe');
  await user.click(within(form).getByRole('button', { name: '添加变量' }));
  await user.type(within(form).getByLabelText('变量名'), 'GITHUB_TOKEN');
  await user.type(within(form).getByLabelText('GITHUB_TOKEN的值'), SECRET);
  await user.click(within(form).getByRole('button', { name: '保存' }));
  expect(api.setMcpSecret).toHaveBeenCalledWith(
    { server_id: 'web', secret_id: null },
    expect.anything(),
  );
  expect(api.saveMcpServer).toHaveBeenCalledWith(
    expect.objectContaining({
      server: expect.objectContaining({
        transport: expect.objectContaining({
          type: 'stdio',
          secret_environment: [{ name: 'GITHUB_TOKEN', secret_id: SECRET_ID }],
        }),
      }),
    }),
  );
  expect(JSON.stringify(api.saveMcpServer.mock.calls)).not.toContain(SECRET);
});

test('the trust dialog lists how a server signs in, and never a secret', async () => {
  const user = userEvent.setup();
  const keyed = httpConfig({
    type: 'http',
    url: 'https://mcp.example.com/mcp',
    headers: [],
    auth: { type: 'secret', secret_id: SECRET_ID, header: 'X-Api-Key' },
  });
  const signed = {
    ...httpConfig({
      type: 'http',
      url: 'https://login.example.com/mcp',
      headers: [],
      auth: { type: 'oauth', scopes: ['repo'] },
    }),
    id: 'login',
    display_name: '登录服务器',
  };
  const program: McpServerView = {
    ...view({
      ...httpConfig({
        type: 'stdio',
        command: 'C:\\Tools\\server.exe',
        secret_environment: [{ name: 'GITHUB_TOKEN', secret_id: SECRET_ID }],
      }),
      id: 'tool',
      display_name: '程序服务器',
    }),
    resolved_command: { path: 'C:\\Tools\\server.exe', sha256: 'ab'.repeat(32) },
  };
  api.listMcpServers.mockResolvedValue(registry([view(keyed), view(signed), program]));
  render(<McpSettings coreReady />);
  const list = await screen.findByRole('list', { name: 'MCP 服务器' });
  const dialog = async (name: string) => {
    const rows = within(list).getAllByRole('button', { name: '查看并信任' });
    await user.click(rows[['web 服务器', '登录服务器', '程序服务器'].indexOf(name)] as HTMLElement);
    return screen.getByRole('alertdialog', { name: `信任 ${name}` });
  };
  expect((await dialog('web 服务器')).textContent).toContain(
    '认证：密钥（请求头 X-Api-Key），由 Aura 的网络网关附加',
  );
  expect((await dialog('登录服务器')).textContent).toContain(
    '认证：OAuth 登录（客户端 ID：自动注册；权限：repo）',
  );
  const shown = await dialog('程序服务器');
  expect(shown.textContent).toContain('带密钥的环境变量：GITHUB_TOKEN');
  expect(shown.textContent).not.toContain(SECRET_ID);
});
