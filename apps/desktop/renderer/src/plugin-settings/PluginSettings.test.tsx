import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type {
  HookEntryView,
  McpServerView,
  PluginPreview,
  PluginView,
} from '@aura/contracts/generated';
import type { PluginUiChange } from '../../../shared/plugin-ui-api';
import { PluginSettings } from './PluginSettings';

const view = (name: string, extra: Partial<PluginView> = {}): PluginView => ({
  name,
  version: '1.0.0',
  description: `${name} 的说明`,
  author: null,
  homepage: null,
  layout: 'aura',
  state: 'ready',
  enabled: true,
  path: `C:\\Aura\\plugins\\packages\\${name}`,
  fingerprint: `${name}-fingerprint`,
  trusted_fingerprint: `${name}-fingerprint`,
  limits: { memory_mb: 2048, processes: 64 },
  counts: { skills: 1, commands: 2, servers: 1, hooks: 0 },
  problems: [],
  warnings: [],
  unsupported: [],
  pending_update: null,
  file_count: 6,
  bytes: 4096,
  ...extra,
});
const registry = (plugins: PluginView[], revision = 7) => ({
  ok: true,
  value: { revision, plugins, library: 'C:\\Aura\\plugins\\packages', issue: null },
});
const preview = (plugin: PluginView, pending = false): PluginPreview => ({
  plugin,
  pending,
  fingerprint: pending ? 'update-fingerprint' : (plugin.fingerprint ?? null),
  skills: [{ relative: 'guide', name: 'guide', description: '审查指南', problems: [] }],
  commands: [
    {
      name: 'commit',
      title: null,
      description: '写提交说明',
      argument_hint: null,
      warnings: ['allowed-tools 不生效：Aura 的命令不带任何权限'],
      problem: null,
    },
  ],
  servers: [
    {
      name: 'files',
      id: `${plugin.name}-files`,
      display_name: '文件',
      kind: 'stdio',
      target: 'C:\\Aura\\plugins\\packages\\kit\\bin\\server.exe',
      arguments: ['--root', '.'],
      secret_names: ['GITHUB_TOKEN'],
      sign_in: null,
      problems: [],
    },
  ],
  hooks: [],
  files: [
    { path: 'aura-plugin.toml', bytes: 120 },
    { path: 'bin/server.exe', bytes: 2048 },
  ],
});

const api = {
  listPlugins: vi.fn(),
  previewPlugin: vi.fn(),
  importPlugin: vi.fn(),
  addDevelopmentPlugin: vi.fn(),
  trustPlugin: vi.fn(),
  untrustPlugin: vi.fn(),
  setPluginEnabled: vi.fn(),
  discardPluginUpdate: vi.fn(),
  uninstallPlugin: vi.fn(),
  selectPluginDirectory: vi.fn(),
  selectPluginArchive: vi.fn(),
  openPluginDirectory: vi.fn(),
  listMcpServers: vi.fn(),
  listHooks: vi.fn(),
  listHookInvocations: vi.fn(),
};
beforeEach(() => {
  for (const mock of Object.values(api)) mock.mockReset();
  api.listMcpServers.mockResolvedValue({ ok: true, value: { revision: 1, servers: [] } });
  api.listHooks.mockResolvedValue({ ok: true, value: { revision: 1, entries: [] } });
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

const card = async (name: string) => {
  const heading = await screen.findByText(name, { selector: 'strong' });
  return heading.closest('li') as HTMLElement;
};
/** The plugin's dialog, opened with its row's 配置. */
const details = async (name: string) => {
  await userEvent.click(within(await card(name)).getByRole('button', { name: `配置 ${name}` }));
  return screen.getByRole('dialog', { name });
};

test('an imported plugin is reviewed, then trusted at exactly the shown content', async () => {
  const kit = view('kit', { state: 'not_trusted', trusted_fingerprint: null });
  api.listPlugins.mockResolvedValue(registry([kit]));
  api.previewPlugin.mockResolvedValue({ ok: true, value: preview(kit) });
  api.trustPlugin.mockResolvedValue(registry([view('kit')], 8));
  render(<PluginSettings coreReady />);
  const row = await card('kit');
  expect(within(row).getByText('未信任，内容都不提供')).toBeTruthy();
  expect(within(row).queryByRole('button', { name: '停用' })).toBeNull();
  // The row is one line; what the plugin brings is in its dialog.
  expect(within(row).queryByText(/Skill 1 · 命令 2 · MCP 1/u)).toBeNull();
  await userEvent.click(within(row).getByRole('button', { name: '查看并信任' }));
  expect(api.previewPlugin).toHaveBeenCalledWith({ name: 'kit', pending: false });
  const dialog = screen.getByRole('dialog', { name: 'kit' });
  expect(within(dialog).getByText(/Skill 1 · 命令 2 · MCP 1/u)).toBeTruthy();
  const shown = await within(dialog).findByRole('region', { name: /的内容$/u });
  // The card shows a description in three lines at most; the review shows all of it.
  expect(within(shown).getByText('kit 的说明')).toBeTruthy();
  expect(within(shown).getByText('/commit')).toBeTruthy();
  expect(within(shown).getByText(/allowed-tools 不生效/u)).toBeTruthy();
  expect(within(shown).getByText(/需要你在 Aura 里输入：GITHUB_TOKEN/u)).toBeTruthy();
  // The one sentence about trusting, next to the trust button and nowhere else.
  expect(within(shown).getByText(/以你的身份运行.*只信任你了解的插件/u)).toBeTruthy();
  await userEvent.click(within(shown).getByRole('button', { name: '信任并启用' }));
  expect(api.trustPlugin).toHaveBeenCalledWith({
    name: 'kit',
    pending: false,
    expected_fingerprint: 'kit-fingerprint',
    expected_revision: 7,
  });
  // Said in the dialog, where the button was.
  expect((await within(dialog).findByRole('status')).textContent).toContain('已信任并启用 kit');
  await userEvent.click(within(dialog).getByRole('button', { name: '关闭' }));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(within(await card('kit')).getByRole('button', { name: '停用' })).toBeTruthy();
});

test('a waiting update is reviewed and trusted as the update, or discarded', async () => {
  const kit = view('kit', {
    pending_update: {
      version: '2.0.0',
      fingerprint: 'update-fingerprint',
      counts: { skills: 1, commands: 3, servers: 1, hooks: 0 },
      problems: [],
      changed_servers: ['files'],
    },
  });
  api.listPlugins.mockResolvedValue(registry([kit]));
  api.previewPlugin.mockResolvedValue({ ok: true, value: preview(kit, true) });
  api.trustPlugin.mockResolvedValue(registry([view('kit', { version: '2.0.0' })], 8));
  api.discardPluginUpdate.mockResolvedValue(registry([view('kit')], 8));
  render(<PluginSettings coreReady />);
  expect(within(await card('kit')).getByText('有新版本')).toBeTruthy();
  const dialog = await details('kit');
  expect(within(dialog).getByText(/新版本 2.0.0 已导入/u)).toBeTruthy();
  expect(within(dialog).getByText('你改过配置、新版本也改了的服务器：files')).toBeTruthy();
  await userEvent.click(within(dialog).getByRole('button', { name: '查看新版本' }));
  expect(api.previewPlugin).toHaveBeenCalledWith({ name: 'kit', pending: true });
  const shown = await within(dialog).findByRole('region', { name: /的内容$/u });
  expect(within(shown).getByText(/新版本 2.0.0（当前 1.0.0）/u)).toBeTruthy();
  await userEvent.click(within(shown).getByRole('button', { name: '信任并更新' }));
  expect(api.trustPlugin).toHaveBeenCalledWith(
    expect.objectContaining({ pending: true, expected_fingerprint: 'update-fingerprint' }),
  );
  expect(await within(dialog).findByText('已更新并信任 kit')).toBeTruthy();
});

test('switching, untrusting and uninstalling say what happens to what it brings', async () => {
  api.listPlugins.mockResolvedValue(registry([view('kit')]));
  api.setPluginEnabled.mockResolvedValue(
    registry([view('kit', { state: 'disabled', enabled: false })], 8),
  );
  api.uninstallPlugin.mockResolvedValue(registry([], 9));
  render(<PluginSettings coreReady />);
  const row = await card('kit');
  await userEvent.click(within(row).getByRole('button', { name: '停用' }));
  expect(api.setPluginEnabled).toHaveBeenCalledWith({
    name: 'kit',
    enabled: false,
    expected_revision: 7,
  });
  expect((await screen.findByRole('status')).textContent).toContain(
    '已停用 kit；它带来的内容都已撤下',
  );
  const dialog = await details('kit');
  await userEvent.click(within(dialog).getByRole('button', { name: '卸载' }));
  const confirm = within(dialog).getByRole('alertdialog', { name: '卸载 kit' });
  expect(within(confirm).getByText(/密钥与登录/u)).toBeTruthy();
  await userEvent.click(within(confirm).getByRole('button', { name: '卸载' }));
  expect(api.uninstallPlugin).toHaveBeenCalledWith({ name: 'kit', expected_revision: 8 });
  await waitFor(() => expect(screen.getByText(/还没有插件/u)).toBeTruthy());
  // The plugin is gone, and its dialog with it.
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('an uninstalled plugin’s dialog stays closed when the list has a plugin of that name again', async () => {
  api.listPlugins.mockResolvedValue(registry([view('kit')]));
  // Imported again at once: the list after the uninstall still has it.
  api.uninstallPlugin.mockResolvedValue(registry([view('kit')], 8));
  render(<PluginSettings coreReady />);
  const dialog = await details('kit');
  await userEvent.click(within(dialog).getByRole('button', { name: '卸载' }));
  const confirm = within(dialog).getByRole('alertdialog', { name: '卸载 kit' });
  await userEvent.click(within(confirm).getByRole('button', { name: '卸载' }));
  expect(await screen.findByText(/已卸载 kit/u)).toBeTruthy();
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('import goes through Main dialogs, and a refusal shows Core’s own sentence', async () => {
  api.listPlugins.mockResolvedValue(registry([]));
  api.selectPluginArchive.mockResolvedValue({
    ok: true,
    value: { path: 'C:\\Downloads\\kit.zip' },
  });
  api.importPlugin.mockResolvedValue({
    ok: false,
    error: {
      message:
        'plugins.invalid: 不是插件：最外层没有 aura-plugin.toml 或 .claude-plugin/plugin.json',
    },
  });
  render(<PluginSettings coreReady />);
  await screen.findByText(/还没有插件/u);
  await userEvent.click(screen.getByRole('button', { name: '导入 ZIP…' }));
  expect(api.importPlugin).toHaveBeenCalledWith({
    path: 'C:\\Downloads\\kit.zip',
    expected_revision: 7,
  });
  expect((await screen.findByRole('alert')).textContent).toContain(
    '不是插件：最外层没有 aura-plugin.toml 或 .claude-plugin/plugin.json',
  );
  api.selectPluginDirectory.mockResolvedValue({ ok: true, value: { path: null } });
  await userEvent.click(screen.getByRole('button', { name: '导入文件夹…' }));
  expect(api.importPlugin).toHaveBeenCalledTimes(1);
});

test('a plugin that cannot be used shows why and offers no trust', async () => {
  api.listPlugins.mockResolvedValue(
    registry([
      view('broken', {
        state: 'invalid',
        version: null,
        fingerprint: null,
        trusted_fingerprint: null,
        problems: ['schema_version 是 3；这个版本的 Aura 只认 1 和 2，请更新 Aura'],
        unsupported: ['子 Agent（agents）：Aura 现在没有子 Agent'],
      }),
    ]),
  );
  render(<PluginSettings coreReady />);
  const row = await card('broken');
  expect(within(row).getByRole('alert').textContent).toContain('只认 1');
  expect(within(row).queryByRole('button', { name: '查看并信任' })).toBeNull();
  const dialog = await details('broken');
  expect(within(dialog).getByText(/Aura 不使用：子 Agent/u)).toBeTruthy();
  expect(within(dialog).queryByRole('button', { name: '查看并信任' })).toBeNull();
  expect(within(dialog).getByRole('button', { name: '卸载' })).toBeTruthy();
});

const server = (id: string, extra: Partial<McpServerView>): McpServerView =>
  ({
    config: {
      id,
      display_name: `${id} 服务器`,
      transport: { type: 'stdio', command: 'C:\\kit\\server.exe', arguments: [] },
    },
    plugin: 'kit',
    state: 'ready',
    problems: [],
    running_instances: 0,
    ...extra,
  }) as McpServerView;

test('a plugin shows how its servers and Hooks are running, and reads Hook calls when opened', async () => {
  api.listPlugins.mockResolvedValue(registry([view('kit')]));
  api.listMcpServers.mockResolvedValue({
    ok: true,
    value: {
      revision: 3,
      servers: [
        server('kit-files', { tool_count: 5, running_instances: 1 }),
        server('kit-web', { state: 'failed', last_error: '程序退出（代码 7）' }),
        server('mine', { plugin: null }),
      ],
    },
  });
  const hook: HookEntryView = {
    path: 'C:\\kit\\hooks\\guard',
    plugin: 'kit',
    enabled: true,
    state: 'ready',
    problems: [],
    definition: { name: 'guard', events: ['tool.before_call'] } as NonNullable<
      HookEntryView['definition']
    >,
  };
  api.listHooks.mockResolvedValue({ ok: true, value: { revision: 2, entries: [hook] } });
  api.listHookInvocations.mockResolvedValue({ ok: true, value: { invocations: [] } });
  const open = vi.fn();
  render(<PluginSettings coreReady onOpenSection={open} />);
  const running = await within(await details('kit')).findByRole('region', { name: '运行情况' });
  expect(within(running).getByText('可用 · 5 个工具')).toBeTruthy();
  expect(within(running).getByText('运行中 1')).toBeTruthy();
  expect(within(running).getByRole('alert').textContent).toContain('程序退出（代码 7）');
  expect(within(running).queryByText('mine 服务器')).toBeNull();
  expect(within(running).getByText('guard')).toBeTruthy();
  expect(api.listHookInvocations).not.toHaveBeenCalled();
  await userEvent.click(within(running).getByText('最近调用'));
  await waitFor(() => expect(api.listHookInvocations).toHaveBeenCalledTimes(1));
  // Going to the MCP page closes the dialog first.
  await userEvent.click(within(running).getAllByRole('button', { name: '在 MCP 页查看' })[0]!);
  expect(open).toHaveBeenCalledWith('mcp');
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('a folder a plugin is written in is added where it is and shown as in development', async () => {
  api.listPlugins.mockResolvedValue(registry([]));
  api.selectPluginDirectory.mockResolvedValue({ ok: true, value: { path: 'C:\\work\\usage' } });
  api.addDevelopmentPlugin.mockResolvedValue(
    registry(
      [
        view('usage', {
          state: 'not_trusted',
          trusted_fingerprint: null,
          path: 'C:\\work\\usage',
          development: 'C:\\work\\usage',
          counts: { skills: 0, commands: 0, servers: 0, hooks: 0, panels: 1, services: 1 },
        }),
      ],
      8,
    ),
  );
  render(<PluginSettings coreReady />);
  await screen.findByText(/还没有插件/u);
  await userEvent.click(screen.getByRole('button', { name: '添加开发中的插件…' }));
  expect(api.addDevelopmentPlugin).toHaveBeenCalledWith({
    path: 'C:\\work\\usage',
    expected_revision: 7,
  });
  expect(api.importPlugin).not.toHaveBeenCalled();
  expect(within(await card('usage')).getByText('开发中')).toBeTruthy();
  expect((await screen.findByRole('status')).textContent).toContain('已添加开发中的插件');
  const dialog = await details('usage');
  expect(within(dialog).getByText('文件夹 C:\\work\\usage')).toBeTruthy();
  expect(within(dialog).getByText('面板 1 · 后台服务 1')).toBeTruthy();
});

test('the review lists the capabilities, panel, service and settings a plugin declares', async () => {
  const usage = view('usage', { state: 'not_trusted', trusted_fingerprint: null });
  api.listPlugins.mockResolvedValue(registry([usage]));
  api.previewPlugin.mockResolvedValue({
    ok: true,
    value: {
      ...preview(usage),
      capabilities: ['session.read', 'notify'],
      panel: {
        panel: { title: '本会话用量', entry: 'panel/index.html', network: [] },
        problems: [],
      },
      service: {
        service: {
          command: 'C:\\work\\usage\\bin\\bot.exe',
          arguments: ['--quiet'],
          network: ['wss://api.example.com'],
        },
        problems: ['后台服务的程序 bin/bot.exe 不存在'],
      },
      options: [
        { key: 'app_id', title: 'App ID', kind: 'text', required: true, description: null },
        { key: 'secret', title: '密钥', kind: 'secret', required: true, description: null },
      ],
    } satisfies PluginPreview,
  });
  render(<PluginSettings coreReady />);
  await userEvent.click(within(await card('usage')).getByRole('button', { name: '查看并信任' }));
  const dialog = screen.getByRole('dialog', { name: 'usage' });
  const shown = await within(dialog).findByRole('region', { name: /的内容$/u });
  const capabilities = within(shown).getByRole('region', { name: '会话能力' });
  expect(within(capabilities).getByText('session.read')).toBeTruthy();
  expect(within(capabilities).getByText('notify')).toBeTruthy();
  const panel = within(shown).getByRole('region', { name: '面板' });
  expect(within(panel).getByText('本会话用量')).toBeTruthy();
  expect(within(panel).getByText('不联网')).toBeTruthy();
  const service = within(shown).getByRole('region', { name: '后台服务' });
  expect(within(service).getByText('运行 C:\\work\\usage\\bin\\bot.exe --quiet')).toBeTruthy();
  expect(within(service).getByText('声明连接：wss://api.example.com')).toBeTruthy();
  expect(within(service).getByRole('alert').textContent).toContain('bin/bot.exe 不存在');
  const options = within(shown).getByRole('region', { name: '插件设置' });
  expect(within(options).getAllByText('（必填）')).toHaveLength(2);
  expect(within(options).getAllByText('（保密）')).toHaveLength(1);
});

// The button that showed a plugin's content hides it again; the content has
// no close button of its own.
test('the content button also hides the content it showed', async () => {
  const kit = view('kit');
  api.listPlugins.mockResolvedValue(registry([kit]));
  api.previewPlugin.mockResolvedValue({ ok: true, value: preview(kit) });
  render(<PluginSettings coreReady />);
  const dialog = await details('kit');
  await userEvent.click(within(dialog).getByRole('button', { name: '查看内容' }));
  const shown = await within(dialog).findByRole('region', { name: /的内容$/u });
  expect(within(shown).queryByRole('button', { name: '关闭' })).toBeNull();
  await userEvent.click(within(dialog).getByRole('button', { name: '收起' }));
  expect(within(dialog).queryByRole('region', { name: /的内容$/u })).toBeNull();
  expect(within(dialog).getByRole('button', { name: '查看内容' })).toBeTruthy();
});

// A plugin's status entry shows in its own row, and follows what Main sends.
test("a plugin's row shows the status entry the plugin shows now", async () => {
  // The page listens for statuses and for its services' changes.
  const listeners: ((change: PluginUiChange) => void)[] = [];
  const push = (change: PluginUiChange) =>
    act(() => {
      for (const listener of listeners) listener(change);
    });
  const pluginUi = {
    statuses: () =>
      Promise.resolve([{ plugin: 'qq-bridge', text: 'QQ 网关已连接', tone: 'success' as const }]),
    onChange: (listener: (change: PluginUiChange) => void) => {
      listeners.push(listener);
      return () => undefined;
    },
  };
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { ...api, pluginUi } },
  });
  api.listPlugins.mockResolvedValue(registry([view('qq-bridge'), view('kit')]));
  render(<PluginSettings coreReady />);
  const bridge = await card('qq-bridge');
  const line = (await within(bridge).findByText('QQ 网关已连接')).closest('p');
  expect(line?.className).toBe('plugin-status-line state-success');
  expect(within(await card('kit')).queryByText(/网关/u)).toBeNull();
  push({ kind: 'statuses', statuses: [{ plugin: 'qq-bridge', text: '重连中', tone: 'warning' }] });
  expect(within(bridge).getByText('重连中')).toBeTruthy();
  push({ kind: 'statuses', statuses: [] });
  expect(within(bridge).queryByText('重连中')).toBeNull();
});
