import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { McpServerConfig, McpServerDetail, McpServerView } from '@aura/contracts/generated';
import { McpSettings } from './McpSettings';

const config = (id: string, extra: Partial<McpServerConfig> = {}): McpServerConfig => ({
  id,
  display_name: `${id} 服务器`,
  transport: { type: 'stdio', command: `C:\\Tools\\${id}.exe`, arguments: ['--stdio'] },
  enabled: true,
  trusted_fingerprint: `${id}-fingerprint`,
  startup_timeout_ms: 30_000,
  call_timeout_ms: 120_000,
  available_for_subagents: false,
  ...extra,
});
const view = (id: string, extra: Partial<McpServerView> = {}): McpServerView => ({
  config: config(id),
  state: 'ready',
  fingerprint: `${id}-fingerprint`,
  resolved_command: { path: `C:\\Tools\\${id}.exe`, sha256: `${id}-sha256` },
  problems: [],
  running_instances: 0,
  tool_count: 3,
  ...extra,
});
const registry = (servers: McpServerView[], revision = 5) => ({
  ok: true,
  value: { revision, servers },
});
const detail = (server: McpServerView, extra: Partial<McpServerDetail> = {}): McpServerDetail => ({
  server,
  tools: [
    {
      canonical_name: `mcp.${server.config.id}.echo`,
      wire_name: 'echo',
      title: null,
      description: 'Echo the text back',
    },
  ],
  skipped_tools: [],
  stderr: [],
  ...extra,
});

const api = {
  listMcpServers: vi.fn(),
  previewMcpServer: vi.fn(),
  describeMcpServer: vi.fn(),
  saveMcpServer: vi.fn(),
  trustMcpServer: vi.fn(),
  untrustMcpServer: vi.fn(),
  setMcpServerEnabled: vi.fn(),
  removeMcpServer: vi.fn(),
  refreshMcpServer: vi.fn(),
  setMcpSecret: vi.fn(),
};
const servers = () => [
  view('files'),
  view('notes', { config: config('notes', { enabled: false }), state: 'disabled' }),
  view('fresh', {
    config: config('fresh', { enabled: false, trusted_fingerprint: null }),
    state: 'not_trusted',
    tool_count: null,
  }),
  view('moved', { state: 'config_changed', fingerprint: 'moved-new-fingerprint' }),
];
beforeEach(() => {
  for (const mock of Object.values(api)) mock.mockReset();
  api.listMcpServers.mockResolvedValue(registry(servers()));
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

const rowOf = (name: string) =>
  screen.getByText(name, { selector: 'strong' }).closest('li') as HTMLElement;

test('lists servers with their state, and only a trusted server offers a switch', async () => {
  render(<McpSettings coreReady />);
  expect(await screen.findByText('files 服务器', { selector: 'strong' })).toBeTruthy();
  expect(within(rowOf('files 服务器')).getByText('可用 · 3 个工具')).toBeTruthy();
  expect(within(rowOf('notes 服务器')).getByText('已停用')).toBeTruthy();
  expect(within(rowOf('fresh 服务器')).getByText('未信任，不会启动')).toBeTruthy();
  expect(within(rowOf('moved 服务器')).getByText('配置已变化，需要重新信任')).toBeTruthy();
  // The page has no warning; the trust box carries one sentence.
  expect(screen.queryByRole('note')).toBeNull();
  expect(within(rowOf('files 服务器')).getByRole('button', { name: '停用' })).toBeTruthy();
  expect(within(rowOf('notes 服务器')).getByRole('button', { name: '启用' })).toBeTruthy();
  // An untrusted or changed server is trusted first, never switched on as if it would run.
  expect(within(rowOf('fresh 服务器')).queryByRole('button', { name: '启用' })).toBeNull();
  expect(within(rowOf('fresh 服务器')).getByRole('button', { name: '查看并信任' })).toBeTruthy();
  expect(within(rowOf('moved 服务器')).queryByRole('button', { name: '停用' })).toBeNull();
  expect(
    within(rowOf('moved 服务器')).getByRole('button', { name: '查看并重新信任' }),
  ).toBeTruthy();
});

test('trust shows what will run and trusts exactly the shown fingerprint', async () => {
  const user = userEvent.setup();
  api.trustMcpServer.mockResolvedValue(
    registry([view('files'), view('fresh', { state: 'starting', tool_count: null })], 6),
  );
  render(<McpSettings coreReady />);
  await screen.findByText('fresh 服务器', { selector: 'strong' });
  await user.click(within(rowOf('fresh 服务器')).getByRole('button', { name: '查看并信任' }));
  const dialog = screen.getByRole('alertdialog', { name: '信任 fresh 服务器' });
  expect(dialog.textContent).toContain('C:\\Tools\\fresh.exe');
  expect(dialog.textContent).toContain('SHA-256 fresh-sha256');
  expect(dialog.textContent).toContain('参数：--stdio');
  expect(dialog.textContent).toContain('能做你能做的事');
  await user.click(within(dialog).getByRole('button', { name: '信任并启用' }));
  expect(api.trustMcpServer).toHaveBeenCalledWith({
    id: 'fresh',
    expected_fingerprint: 'fresh-fingerprint',
    expected_revision: 5,
  });
  expect(await screen.findByText('已信任并启用 fresh 服务器，正在列出工具')).toBeTruthy();
  expect(within(rowOf('fresh 服务器')).getByText('正在启动并列出工具…')).toBeTruthy();
});

test('adding a server checks it, saves it untrusted and never trusts it on save', async () => {
  const user = userEvent.setup();
  api.previewMcpServer.mockResolvedValue({
    ok: true,
    value: {
      fingerprint: 'new-fingerprint',
      resolved_command: { path: 'C:\\Tools\\git-mcp.exe', sha256: 'abc' },
      problems: [],
    },
  });
  const added = view('git', {
    config: config('git', { enabled: false, trusted_fingerprint: null }),
    state: 'not_trusted',
  });
  api.saveMcpServer.mockResolvedValue(registry([...servers(), added], 6));
  render(<McpSettings coreReady />);
  await screen.findByText('files 服务器', { selector: 'strong' });
  await user.click(screen.getByRole('button', { name: '添加服务器' }));
  const form = screen.getByRole('form', { name: '添加 MCP 服务器' });
  await user.type(within(form).getByLabelText('服务器 ID'), 'git');
  await user.type(within(form).getByLabelText('显示名'), 'git 服务器');
  await user.type(within(form).getByLabelText('命令'), 'C:\\Tools\\git-mcp.exe');
  await user.type(within(form).getByLabelText('参数（每行一个）'), '--repo{enter}.');
  await user.click(within(form).getByRole('button', { name: '检查' }));
  expect(await within(form).findByText('配置可以使用。')).toBeTruthy();
  expect(within(form).getByText('C:\\Tools\\git-mcp.exe', { selector: 'code' })).toBeTruthy();
  await user.click(within(form).getByRole('button', { name: '保存' }));
  expect(api.saveMcpServer).toHaveBeenCalledWith({
    server: expect.objectContaining({
      id: 'git',
      display_name: 'git 服务器',
      transport: {
        type: 'stdio',
        command: 'C:\\Tools\\git-mcp.exe',
        arguments: ['--repo', '.'],
        working_directory: null,
        environment: [],
      },
      enabled: false,
      trusted_fingerprint: null,
    }),
    replaces: null,
    expected_revision: 5,
  });
  expect(await screen.findByText('已保存 git；信任后才会启动')).toBeTruthy();
  expect(api.trustMcpServer).not.toHaveBeenCalled();
});

test('editing keeps the server ID and names the server it replaces', async () => {
  const user = userEvent.setup();
  api.saveMcpServer.mockResolvedValue(registry(servers(), 6));
  render(<McpSettings coreReady />);
  await screen.findByText('files 服务器', { selector: 'strong' });
  await user.click(within(rowOf('files 服务器')).getByRole('button', { name: '编辑' }));
  const form = screen.getByRole('form', { name: '编辑 files 服务器' });
  expect(within(form).getByLabelText('服务器 ID')).toHaveProperty('disabled', true);
  const name = within(form).getByLabelText('显示名');
  await user.clear(name);
  await user.type(name, '文件');
  await user.click(within(form).getByRole('button', { name: '保存' }));
  expect(api.saveMcpServer).toHaveBeenCalledWith(
    expect.objectContaining({
      server: expect.objectContaining({ id: 'files', display_name: '文件', enabled: true }),
      replaces: 'files',
      expected_revision: 5,
    }),
  );
  expect(await screen.findByText('已保存 files')).toBeTruthy();
});

test('switching, untrusting and removing name the shown revision; removal asks first', async () => {
  const user = userEvent.setup();
  api.setMcpServerEnabled.mockResolvedValue(registry(servers(), 6));
  api.untrustMcpServer.mockResolvedValue(registry(servers(), 7));
  api.removeMcpServer.mockResolvedValue(registry(servers().slice(1), 8));
  render(<McpSettings coreReady />);
  await screen.findByText('files 服务器', { selector: 'strong' });
  await user.click(within(rowOf('files 服务器')).getByRole('button', { name: '停用' }));
  expect(api.setMcpServerEnabled).toHaveBeenCalledWith({
    id: 'files',
    expected_revision: 5,
    enabled: false,
  });
  expect(await screen.findByText('已停用 files 服务器，正在运行的实例已停止')).toBeTruthy();
  await user.click(within(rowOf('files 服务器')).getByRole('button', { name: '撤销信任' }));
  expect(api.untrustMcpServer).toHaveBeenCalledWith({ id: 'files', expected_revision: 6 });
  await user.click(within(rowOf('files 服务器')).getByRole('button', { name: '移除' }));
  const confirm = screen.getByRole('alertdialog', { name: '移除 files 服务器' });
  expect(confirm.textContent).toContain('程序文件不会被删除');
  expect(api.removeMcpServer).not.toHaveBeenCalled();
  await user.click(within(confirm).getByRole('button', { name: '移除' }));
  expect(api.removeMcpServer).toHaveBeenCalledWith({ id: 'files', expected_revision: 7 });
  await waitFor(() =>
    expect(screen.queryByText('files 服务器', { selector: 'strong' })).toBeNull(),
  );
});

test("a refusal shows Core's sentence and reads the list again", async () => {
  const user = userEvent.setup();
  api.setMcpServerEnabled.mockResolvedValue({
    ok: false,
    error: { message: 'mcp.revision_changed: 服务器列表已被修改，请刷新后再操作' },
  });
  render(<McpSettings coreReady />);
  await screen.findByText('files 服务器', { selector: 'strong' });
  await user.click(within(rowOf('files 服务器')).getByRole('button', { name: '停用' }));
  expect(await screen.findByText('服务器列表已被修改，请刷新后再操作')).toBeTruthy();
  await waitFor(() => expect(api.listMcpServers).toHaveBeenCalledTimes(2));
});

test('testing the connection lists the tools, and a failed start shows its error output', async () => {
  const user = userEvent.setup();
  const files = view('files');
  api.refreshMcpServer.mockResolvedValueOnce({ ok: true, value: detail(files) });
  api.describeMcpServer.mockResolvedValue({ ok: true, value: detail(files) });
  render(<McpSettings coreReady />);
  await screen.findByText('files 服务器', { selector: 'strong' });
  await user.click(within(rowOf('files 服务器')).getByRole('button', { name: '测试连接' }));
  expect(await screen.findByText('files 服务器 连接成功，列出 1 个工具')).toBeTruthy();
  const panel = await screen.findByRole('region', { name: 'files 服务器 的工具' });
  expect(within(panel).getByText('mcp.files.echo')).toBeTruthy();

  const failed = view('files', { state: 'failed', last_error: '服务器进程已退出（退出码 3）' });
  api.refreshMcpServer.mockResolvedValueOnce({
    ok: true,
    value: detail(failed, { tools: [], stderr: ['fixture: exiting at start'] }),
  });
  api.describeMcpServer.mockResolvedValue({
    ok: true,
    value: detail(failed, { tools: [], stderr: ['fixture: exiting at start'] }),
  });
  await user.click(within(rowOf('files 服务器')).getByRole('button', { name: '测试连接' }));
  expect(
    await screen.findByText('files 服务器 没有连接成功：服务器进程已退出（退出码 3）'),
  ).toBeTruthy();
});

test('nothing is offered before Core is ready', () => {
  render(<McpSettings coreReady={false} />);
  expect(screen.getByText('Core 尚未就绪，MCP 管理暂不可用。')).toBeTruthy();
  expect(screen.getByRole('button', { name: '添加服务器' })).toHaveProperty('disabled', true);
  expect(api.listMcpServers).not.toHaveBeenCalled();
});

test('an HTTP server is added by its address and its trust says where calls go', async () => {
  const user = userEvent.setup();
  const url = 'http://127.0.0.1:7000/mcp';
  const http = config('web', {
    transport: { type: 'http', url, headers: [{ name: 'X-Team', value: 'docs' }] },
    enabled: false,
    trusted_fingerprint: null,
  });
  const added = view('web', {
    config: http,
    state: 'not_trusted',
    resolved_command: null,
    tool_count: null,
  });
  api.previewMcpServer.mockResolvedValue({
    ok: true,
    value: { fingerprint: 'web-fingerprint', resolved_command: null, problems: [] },
  });
  api.saveMcpServer.mockResolvedValue(registry([...servers(), added], 6));
  render(<McpSettings coreReady />);
  await screen.findByText('files 服务器', { selector: 'strong' });
  await user.click(screen.getByRole('button', { name: '添加服务器' }));
  const form = screen.getByRole('form', { name: '添加 MCP 服务器' });
  await user.type(within(form).getByLabelText('服务器 ID'), 'web');
  await user.type(within(form).getByLabelText('显示名'), 'web 服务器');
  await user.click(within(form).getByLabelText('HTTP 地址（Streamable HTTP）'));
  await user.type(within(form).getByLabelText('地址'), url);
  await user.type(
    within(form).getByLabelText('请求头（每行 名字: 值，不要写密钥）'),
    'X-Team: docs',
  );
  await user.click(within(form).getByRole('button', { name: '检查' }));
  expect(await within(form).findByText(url, { selector: 'code' })).toBeTruthy();
  await user.click(within(form).getByRole('button', { name: '保存' }));
  expect(api.saveMcpServer).toHaveBeenCalledWith(
    expect.objectContaining({
      server: expect.objectContaining({
        id: 'web',
        transport: { type: 'http', url, headers: [{ name: 'X-Team', value: 'docs' }] },
        trusted_fingerprint: null,
      }),
    }),
  );
  await user.click(within(rowOf('web 服务器')).getByRole('button', { name: '查看并信任' }));
  const dialog = screen.getByRole('alertdialog', { name: '信任 web 服务器' });
  expect(dialog.textContent).toContain('信任后 Aura 会连接');
  expect(dialog.textContent).toContain(url);
  expect(dialog.textContent).toContain('请求头 X-Team: docs');
  expect(dialog.textContent).toContain('工具调用的参数会发到这个地址；只信任你了解的服务器。');
  expect(dialog.textContent).not.toContain('能做你能做的事');
});

test('a plugin’s server is managed with its plugin, and its secrets are entered here', async () => {
  const user = userEvent.setup();
  const secretId = 'crd_01M55FF84C9XFN71FZ0GS9967Y';
  const brought = view('kit-files', {
    plugin: 'kit',
    config: config('kit-files', {
      transport: {
        type: 'stdio',
        command: 'C:/Aura/plugins/packages/kit/bin/server.exe',
        arguments: [],
        secret_environment: [{ name: 'GITHUB_TOKEN', secret_id: secretId }],
      },
    }),
    state: 'invalid',
    problems: ['密钥还没有设置：GITHUB_TOKEN'],
  });
  api.listMcpServers.mockResolvedValue(registry([brought]));
  api.setMcpSecret.mockResolvedValue({ ok: true, value: { secret_id: secretId } });
  render(<McpSettings coreReady />);
  await screen.findByText('kit-files 服务器', { selector: 'strong' });
  const row = rowOf('kit-files 服务器');
  expect(within(row).getByText('插件 kit')).toBeTruthy();
  for (const name of ['编辑', '移除', '撤销信任', '停用', '启用'])
    expect(within(row).queryByRole('button', { name })).toBeNull();
  expect(within(row).getByText(/编辑与移除请到 设置 → 插件/u)).toBeTruthy();
  await user.type(within(row).getByLabelText('GITHUB_TOKEN'), 'token-value');
  await user.click(within(row).getByRole('button', { name: '保存' }));
  const [request, bytes] = api.setMcpSecret.mock.calls[0] as [unknown, Uint8Array];
  expect(request).toEqual({ server_id: 'kit-files', secret_id: secretId });
  expect(bytes.length).toBe('token-value'.length);
  expect((await screen.findByRole('status')).textContent).toContain('已保存 GITHUB_TOKEN');
});
