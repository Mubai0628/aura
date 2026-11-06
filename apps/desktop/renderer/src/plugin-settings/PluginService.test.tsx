import { act, cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { PluginServiceView, PluginView } from '@aura/contracts/generated';
import type { PluginUiChange } from '../../../shared/plugin-ui-api';
import { PluginSettings } from './PluginSettings';

const bot = (extra: Partial<PluginView> = {}): PluginView => ({
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
  counts: { skills: 0, commands: 0, servers: 0, hooks: 0, services: 1, channels: 2 },
  problems: [],
  warnings: [],
  unsupported: [],
  pending_update: null,
  file_count: 3,
  bytes: 2048,
  service: { on: false, status: { state: 'off' } },
  options: [
    {
      option: { key: 'app_id', title: 'AppID', kind: 'text', required: true, description: null },
      text: null,
      set: false,
    },
    {
      option: {
        key: 'app_secret',
        title: 'AppSecret',
        kind: 'secret',
        required: true,
        description: null,
      },
      text: null,
      set: false,
    },
  ],
  ...extra,
});
const registry = (plugins: PluginView[], revision = 7) => ({
  ok: true,
  value: { revision, plugins, library: 'C:\\Aura\\plugins\\packages', issue: null },
});

let listeners: ((change: PluginUiChange) => void)[] = [];
const api = {
  listPlugins: vi.fn(),
  configurePlugin: vi.fn(),
  setPluginSecret: vi.fn(),
  listMcpServers: vi.fn(),
  listHooks: vi.fn(),
  pluginUi: {
    statuses: vi.fn(async () => []),
    onChange: vi.fn((listener: (change: PluginUiChange) => void) => {
      listeners.push(listener);
      return () => {
        listeners = listeners.filter((known) => known !== listener);
      };
    }),
  },
};
beforeEach(() => {
  listeners = [];
  for (const mock of [api.listPlugins, api.configurePlugin, api.setPluginSecret]) mock.mockReset();
  api.listMcpServers.mockResolvedValue({ ok: true, value: { revision: 1, servers: [] } });
  api.listHooks.mockResolvedValue({ ok: true, value: { revision: 1, entries: [] } });
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

/** The plugin's dialog, opened with its row's 配置. */
const card = async () => {
  const heading = await screen.findByText('chat-bot', { selector: 'strong' });
  const row = heading.closest('li') as HTMLElement;
  await userEvent.click(within(row).getByRole('button', { name: '配置 chat-bot' }));
  return screen.getByRole('dialog', { name: 'chat-bot' });
};

test("the service's switch is saved on the plugin page and its state is shown", async () => {
  api.listPlugins.mockResolvedValue(registry([bot()]));
  const waiting = bot({
    service: { on: true, status: { state: 'waiting', reason: '缺少设置：AppID、AppSecret' } },
  });
  api.configurePlugin.mockResolvedValue(registry([waiting], 8));
  render(<PluginSettings coreReady />);
  const row = await card();
  expect(within(row).getByText(/渠道 2/u)).toBeTruthy();
  const service = within(row).getByRole('region', { name: '后台服务' });
  expect(within(service).getByText('未开启')).toBeTruthy();
  await userEvent.click(within(service).getByRole('switch', { name: 'chat-bot 的后台服务' }));
  expect(api.configurePlugin).toHaveBeenCalledWith({
    name: 'chat-bot',
    expected_revision: 7,
    service: true,
  });
  expect(await within(service).findByText('缺少设置：AppID、AppSecret')).toBeTruthy();
});

test('each state of the service reads as what it is', async () => {
  const states: [PluginServiceView, string, string?][] = [
    [{ on: true, status: { state: 'starting' } }, '启动中'],
    [{ on: true, status: { state: 'running', since: '2026-11-01T08:00:00Z' } }, '运行中'],
    [
      { on: true, status: { state: 'restarting', seconds: 4, reason: '服务结束了（退出码 1）' } },
      '4 秒后重启',
      '服务结束了（退出码 1）',
    ],
    [{ on: true, status: { state: 'stopped', reason: '协议版本不对' } }, '已停止', '协议版本不对'],
  ];
  for (const [service, label, detail] of states) {
    api.listPlugins.mockResolvedValue(registry([bot({ service })]));
    render(<PluginSettings coreReady />);
    const shown = within(await card()).getByRole('region', { name: '后台服务' });
    expect(within(shown).getByText(label)).toBeTruthy();
    if (detail) expect(within(shown).getByText(detail)).toBeTruthy();
    cleanup();
  }
});

test('a text setting is saved with the plugin; a secret one only on its own channel', async () => {
  api.listPlugins.mockResolvedValue(registry([bot()]));
  api.configurePlugin.mockResolvedValue(registry([bot()], 8));
  api.setPluginSecret.mockResolvedValue({ ok: true, value: { revision: 9 } });
  render(<PluginSettings coreReady />);
  const settings = within(await card()).getByRole('region', { name: '设置' });
  await userEvent.type(within(settings).getByLabelText(/AppID/u), ' 1024 ');
  await userEvent.click(within(settings).getByRole('button', { name: '保存设置' }));
  expect(api.configurePlugin).toHaveBeenCalledWith({
    name: 'chat-bot',
    expected_revision: 7,
    text: { app_id: '1024' },
  });
  const secret = within(settings).getByLabelText('AppSecret（必填）') as HTMLInputElement;
  await userEvent.type(secret, 'sk-live-123');
  await userEvent.click(within(settings).getByRole('button', { name: '保存 AppSecret' }));
  const [request, bytes] = api.setPluginSecret.mock.calls[0] as [unknown, Uint8Array];
  expect(request).toEqual({ name: 'chat-bot', key: 'app_secret' });
  expect(new TextDecoder().decode(bytes)).toBe('sk-live-123');
  expect(secret.value).toBe('');
  expect(api.listPlugins).toHaveBeenCalledTimes(2);
});

test('a secret setting that has a value can be cleared', async () => {
  const options = (bot().options ?? []).map((value) =>
    value.option.kind === 'secret' ? { ...value, set: true } : value,
  );
  api.listPlugins.mockResolvedValue(registry([bot({ options })]));
  api.configurePlugin.mockResolvedValue(registry([bot()], 8));
  render(<PluginSettings coreReady />);
  const settings = within(await card()).getByRole('region', { name: '设置' });
  await userEvent.click(within(settings).getByRole('button', { name: '清除 AppSecret' }));
  expect(api.configurePlugin).toHaveBeenCalledWith({
    name: 'chat-bot',
    expected_revision: 7,
    forget_secrets: ['app_secret'],
  });
});

test('the plugins are read again when a background service changes state', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    api.listPlugins.mockResolvedValue(registry([bot()]));
    render(<PluginSettings coreReady />);
    await card();
    expect(api.listPlugins).toHaveBeenCalledTimes(1);
    act(() => {
      for (const listener of listeners) listener({ kind: 'service', plugin: 'chat-bot' });
      for (const listener of listeners) listener({ kind: 'service', plugin: 'chat-bot' });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(600);
    });
    expect(api.listPlugins).toHaveBeenCalledTimes(2);
  } finally {
    vi.useRealTimers();
  }
});
