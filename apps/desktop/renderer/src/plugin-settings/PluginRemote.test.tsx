import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { PluginRemoteView, PluginView } from '@aura/contracts/generated';
import { PluginSettings } from './PluginSettings';

vi.mock('../workspace-session-store', () => ({
  useWorkspaceSessions: () => ({
    loading: false,
    projection: {
      payload: {
        workspaces: [
          {
            workspace_id: 'wsp_old',
            display_name: '旧项目',
            selected_root: 'C:\\Old',
            state: 'archived',
            revision: '5',
          },
          {
            workspace_id: 'wsp_live',
            display_name: '当前项目',
            selected_root: 'C:\\Live',
            state: 'online',
            revision: '2',
          },
          {
            workspace_id: 'wsp_chat',
            display_name: '聊天',
            selected_root: 'C:\\Chat',
            state: 'online',
            revision: '3',
          },
        ],
        sessions: [
          { session_id: 'ses_old', workspace_id: 'wsp_old', title: '旧会话', state: 'idle' },
          {
            session_id: 'ses_kept',
            workspace_id: 'wsp_live',
            title: '收起的会话',
            state: 'archived',
          },
          { session_id: 'ses_live', workspace_id: 'wsp_live', title: '进行中', state: 'idle' },
          { session_id: 'ses_chat', workspace_id: 'wsp_chat', title: '群里的', state: 'idle' },
        ],
      },
    },
  }),
}));

const REMOTE: PluginRemoteView = {
  channels: [{ id: 'qq-main', platform: 'qq', title: 'QQ 机器人' }],
  scope: {},
  allowed: [
    { channel: 'qq-main', id: 'friend', name: '老王' },
    { channel: 'qq-main', id: 'buddy', name: '' },
  ],
  remote_approval: false,
  unknown: [
    {
      channel: 'qq-main',
      id: 'stranger',
      name: '陌生人',
      chat: 'group',
      at: '2026-11-01T08:00:00Z',
    },
  ],
};

const bot = (remote?: PluginRemoteView): PluginView => ({
  name: 'chat-bot',
  version: '0.1.0',
  description: '把聊天平台接到 Aura',
  author: null,
  homepage: null,
  layout: 'aura',
  state: 'ready',
  enabled: true,
  path: 'C:\\Aura\\plugins\\packages\\chat-bot',
  fingerprint: 'bot-fingerprint',
  trusted_fingerprint: 'bot-fingerprint',
  limits: { memory_mb: 2048, processes: 64 },
  counts: { skills: 0, commands: 0, servers: 0, hooks: 0, services: 1, channels: 1 },
  problems: [],
  warnings: [],
  unsupported: [],
  pending_update: null,
  file_count: 3,
  bytes: 2048,
  service: { on: true, status: { state: 'running', since: '2026-11-01T08:00:00Z' } },
  ...(remote ? { remote } : {}),
});
const registry = (plugins: PluginView[], revision = 7) => ({
  ok: true,
  value: { revision, plugins, library: 'C:\\Aura\\plugins\\packages', issue: null },
});

const api = {
  listPlugins: vi.fn(),
  configurePlugin: vi.fn(),
  setPluginSecret: vi.fn(),
  listMcpServers: vi.fn(),
  listHooks: vi.fn(),
  pluginUi: { statuses: vi.fn(async () => []), onChange: vi.fn(() => () => undefined) },
};
beforeEach(() => {
  for (const mock of [api.listPlugins, api.configurePlugin]) mock.mockReset();
  api.listPlugins.mockResolvedValue(registry([bot(REMOTE)]));
  api.configurePlugin.mockResolvedValue(registry([bot(REMOTE)], 8));
  api.listMcpServers.mockResolvedValue({ ok: true, value: { revision: 1, servers: [] } });
  api.listHooks.mockResolvedValue({ ok: true, value: { revision: 1, entries: [] } });
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

/** The plugin's dialog, opened with its row's 配置. */
const dialog = async () => {
  const heading = await screen.findByText('chat-bot', { selector: 'strong' });
  const row = heading.closest('li') as HTMLElement;
  await userEvent.click(within(row).getByRole('button', { name: '配置 chat-bot' }));
  return screen.getByRole('dialog', { name: 'chat-bot' });
};
const remote = async () => within(await dialog()).getByRole('region', { name: '远程' });

test('the conversations a service may reach are chosen by workspace or one by one, then saved', async () => {
  render(<PluginSettings coreReady />);
  const shown = await remote();
  expect(within(shown).queryByText('旧项目')).toBeNull();
  const save = within(shown).getByRole('button', { name: '保存会话范围' }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  const live = within(shown).getByRole('list', { name: '当前项目 的会话' });
  expect(within(live).queryByText('收起的会话')).toBeNull();
  await userEvent.click(within(live).getByRole('checkbox', { name: '进行中' }));
  expect(save.disabled).toBe(false);
  expect(within(shown).getByRole('list', { name: '聊天 的会话' })).toBeTruthy();
  await userEvent.click(within(shown).getByRole('checkbox', { name: '聊天' }));
  expect(within(shown).queryByRole('list', { name: '聊天 的会话' })).toBeNull();
  await userEvent.click(save);
  expect(api.configurePlugin).toHaveBeenCalledWith({
    name: 'chat-bot',
    expected_revision: 7,
    scope: { workspaces: ['wsp_chat'], sessions: ['ses_live'] },
  });
  expect(
    await within(screen.getByRole('dialog', { name: 'chat-bot' })).findByText(
      '已保存 chat-bot 的会话范围',
    ),
  ).toBeTruthy();
});

test("a channel's allowlist adds and removes people; someone unlisted is added in one click", async () => {
  render(<PluginSettings coreReady />);
  const shown = await remote();
  const list = within(shown).getByRole('group', { name: 'QQ 机器人 的白名单' });
  expect(within(list).getByText('QQ 机器人 · QQ')).toBeTruthy();
  expect(within(list).getByText('friend')).toBeTruthy();
  expect(within(list).getByRole('button', { name: '移除 buddy' })).toBeTruthy();
  const add = within(list).getByRole('button', { name: '添加' }) as HTMLButtonElement;
  expect(add.disabled).toBe(true);
  const id = within(list).getByLabelText('用户编号');
  await userEvent.type(id, 'friend');
  expect(add.disabled).toBe(true);
  await userEvent.clear(id);
  await userEvent.type(id, ' 10086 ');
  await userEvent.type(within(list).getByLabelText('名字'), '小李');
  await userEvent.click(add);
  expect(api.configurePlugin).toHaveBeenLastCalledWith({
    name: 'chat-bot',
    expected_revision: 7,
    allowed: [
      { channel: 'qq-main', id: 'friend', name: '老王' },
      { channel: 'qq-main', id: 'buddy', name: '' },
      { channel: 'qq-main', id: '10086', name: '小李' },
    ],
  });
  expect(
    await within(screen.getByRole('dialog', { name: 'chat-bot' })).findByText(
      '已更新 chat-bot 的白名单',
    ),
  ).toBeTruthy();
  expect((id as HTMLInputElement).value).toBe('');

  await userEvent.click(within(list).getByRole('button', { name: '移除 老王' }));
  expect(api.configurePlugin).toHaveBeenLastCalledWith({
    name: 'chat-bot',
    expected_revision: 8,
    allowed: [{ channel: 'qq-main', id: 'buddy', name: '' }],
  });

  const unknown = within(await remote()).getByRole('group', { name: '不在名单里的用户' });
  expect(unknown.textContent).toContain('QQ 机器人 · 陌生人 · 群聊');
  await userEvent.click(within(unknown).getByRole('button', { name: '把 陌生人 加入白名单' }));
  expect(api.configurePlugin).toHaveBeenLastCalledWith({
    name: 'chat-bot',
    expected_revision: 8,
    allowed: [
      { channel: 'qq-main', id: 'friend', name: '老王' },
      { channel: 'qq-main', id: 'buddy', name: '' },
      { channel: 'qq-main', id: 'stranger', name: '陌生人' },
    ],
  });
});

test('remote approval is a switch of its own, off until the user turns it on', async () => {
  render(<PluginSettings coreReady />);
  const shown = await remote();
  const approval = within(shown).getByRole('switch', { name: 'chat-bot 的远程批准' });
  expect(approval.getAttribute('aria-checked')).toBe('false');
  await userEvent.click(approval);
  expect(api.configurePlugin).toHaveBeenCalledWith({
    name: 'chat-bot',
    expected_revision: 7,
    remote_approval: true,
  });
  expect(
    await within(screen.getByRole('dialog', { name: 'chat-bot' })).findByText(
      '已开启 chat-bot 的远程批准',
    ),
  ).toBeTruthy();
});

test('a plugin without a background service shows none of it', async () => {
  api.listPlugins.mockResolvedValue(registry([bot()]));
  render(<PluginSettings coreReady />);
  expect(within(await dialog()).queryByRole('region', { name: '远程' })).toBeNull();
});
