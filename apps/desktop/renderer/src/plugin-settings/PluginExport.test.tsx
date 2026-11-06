import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { PluginPreview, PluginView } from '@aura/contracts/generated';
import { PluginSettings } from './PluginSettings';

// A plugin is exported as a ZIP file: the page names the plugin, Main asks where to save it and
// Core writes the files it read there. The page says what happened. Its theme is reviewed with
// what else it brings.

const view = (name: string, extra: Partial<PluginView> = {}): PluginView => ({
  name,
  version: '1.0.0',
  description: null,
  author: null,
  homepage: null,
  layout: 'aura',
  state: 'ready',
  enabled: true,
  path: `C:\\Aura\\plugins\\packages\\${name}`,
  fingerprint: `${name}-fingerprint`,
  trusted_fingerprint: `${name}-fingerprint`,
  limits: { memory_mb: 2048, processes: 64 },
  counts: { skills: 1, commands: 0, servers: 0, hooks: 0, themes: 1 },
  problems: [],
  warnings: [],
  unsupported: [],
  pending_update: null,
  file_count: 3,
  bytes: 512,
  ...extra,
});
const registry = (plugins: PluginView[]) => ({
  ok: true,
  value: { revision: 7, plugins, library: 'C:\\Aura\\plugins\\packages', issue: null },
});

const api = {
  listPlugins: vi.fn(),
  previewPlugin: vi.fn(),
  exportPlugin: vi.fn(),
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

test('a plugin is exported where Main asks, and the page says what happened', async () => {
  const user = userEvent.setup();
  api.listPlugins.mockResolvedValue(
    registry([
      view('night'),
      view('broken', {
        state: 'invalid',
        counts: { skills: 0, commands: 0, servers: 0, hooks: 0 },
      }),
    ]),
  );
  render(<PluginSettings coreReady />);
  const dialog = await details('night');
  expect(within(dialog).getByText(/Skill 1 · 主题 1/u)).toBeTruthy();

  api.exportPlugin.mockResolvedValueOnce({
    ok: true,
    value: { completed: true, message: '已导出 3 个文件' },
  });
  await user.click(within(dialog).getByRole('button', { name: '导出…' }));
  expect(api.exportPlugin).toHaveBeenCalledWith({ name: 'night' });
  expect((await within(dialog).findByRole('status')).textContent).toBe('已导出 3 个文件');

  api.exportPlugin.mockResolvedValueOnce({
    ok: true,
    value: { completed: false, message: '已取消导出' },
  });
  await user.click(within(dialog).getByRole('button', { name: '导出…' }));
  expect((await within(dialog).findByRole('status')).textContent).toBe('已取消导出');

  api.exportPlugin.mockResolvedValueOnce({
    ok: false,
    error: { message: 'plugins.export_failed: 插件的文件 theme.toml 在读取后变了，请刷新后再导出' },
  });
  await user.click(within(dialog).getByRole('button', { name: '导出…' }));
  const refused = await within(dialog).findByText(
    '插件的文件 theme.toml 在读取后变了，请刷新后再导出',
  );
  expect(refused.getAttribute('role')).toBe('alert');

  // A plugin Aura cannot read has nothing to export.
  const broken = await details('broken');
  expect(within(broken).queryByRole('button', { name: '导出…' })).toBeNull();
});

test('the review shows the theme a plugin brings, or why it cannot be used', async () => {
  const night = view('night', { state: 'not_trusted', trusted_fingerprint: null });
  api.listPlugins.mockResolvedValue(registry([night]));
  const shownPreview = (theme: NonNullable<PluginPreview['theme']>): PluginPreview => ({
    plugin: night,
    pending: false,
    fingerprint: 'night-fingerprint',
    skills: [],
    commands: [],
    servers: [],
    hooks: [],
    files: [{ path: 'theme.toml', bytes: 120 }],
    theme,
  });
  api.previewPlugin.mockResolvedValueOnce({
    ok: true,
    value: shownPreview({ title: '夜间', problems: [] }),
  });
  render(<PluginSettings coreReady />);
  await userEvent.click(within(await card('night')).getByRole('button', { name: '查看并信任' }));
  const dialog = screen.getByRole('dialog', { name: 'night' });
  const theme = await within(dialog).findByRole('region', { name: '主题' });
  expect(within(theme).getByText('夜间')).toBeTruthy();
  expect(within(theme).queryByRole('alert')).toBeNull();

  await userEvent.click(within(dialog).getByRole('button', { name: '收起' }));
  api.previewPlugin.mockResolvedValueOnce({
    ok: true,
    value: shownPreview({ problems: ['[colors] ink 要是 #rgb、#rrggbb 或 #rrggbbaa'] }),
  });
  await userEvent.click(within(dialog).getByRole('button', { name: '查看并信任' }));
  const broken = await within(dialog).findByRole('region', { name: '主题' });
  expect(within(broken).getByRole('alert').textContent).toContain('[colors] ink');
});
