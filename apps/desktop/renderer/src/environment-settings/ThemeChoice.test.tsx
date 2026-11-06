import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { PluginView, UiSettings } from '@aura/contracts/generated';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { EnvironmentSettings } from './EnvironmentSettings';
import { ThemeChoice } from './ThemeChoice';
import { resetUiSettingsForTests } from '../ui-settings-store';

// 设置 → 环境 → 外观: Aura's own theme or the theme of a ready plugin. A theme that cannot be
// used now stays shown as chosen, and the window shows Aura's own meanwhile.

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
  fingerprint: null,
  trusted_fingerprint: null,
  limits: { memory_mb: 2048, processes: 64 },
  counts: { skills: 0, commands: 0, servers: 0, hooks: 0 },
  problems: [],
  warnings: [],
  unsupported: [],
  pending_update: null,
  file_count: 2,
  bytes: 100,
  ...extra,
});

const listPlugins = vi.fn();
const update = vi.fn();

beforeEach(() => {
  resetUiSettingsForTests();
  listPlugins.mockReset().mockResolvedValue({
    ok: true,
    value: {
      revision: 3,
      plugins: [
        view('night', { theme: '夜间' }),
        view('draft', { theme: '草稿', state: 'not_trusted' }),
        view('plain'),
      ],
      library: 'C:\\Aura\\plugins\\packages',
      issue: null,
    },
  });
  update
    .mockReset()
    .mockImplementation(({ settings }: { expected_revision: number; settings: UiSettings }) =>
      Promise.resolve({ ok: true, value: { revision: 5, settings, issue: null } }),
    );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listPlugins,
        getUiSettings: vi.fn().mockResolvedValue({
          ok: true,
          value: {
            revision: 4,
            settings: { show_debug_information: false, default_terminal: 'power_shell' },
            issue: null,
          },
        }),
        updateUiSettings: update,
        git: { runtime: vi.fn(() => new Promise(() => undefined)), openInstallation: vi.fn() },
      },
    },
  });
});
afterEach(cleanup);

const radios = (group: HTMLElement) =>
  within(group)
    .getAllByRole('radio')
    .map((radio) => [radio.closest('label')?.textContent, (radio as HTMLInputElement).checked]);

test("the window's theme is Aura's own or a ready plugin's, saved with the other choices", async () => {
  const user = userEvent.setup();
  render(<EnvironmentSettings />);
  const group = await screen.findByRole('radiogroup', { name: '主题' });
  await within(group).findByRole('radio', { name: '夜间' });
  await vi.waitFor(() =>
    expect(within(group).getByRole('radio', { name: '夜间' })).toHaveProperty('disabled', false),
  );
  // A plugin not trusted yet, or without a theme, is not offered.
  expect(radios(group)).toEqual([
    ['Aura 默认', true],
    ['夜间', false],
  ]);
  await user.click(within(group).getByRole('radio', { name: '夜间' }));
  expect(update).toHaveBeenLastCalledWith({
    expected_revision: 4,
    settings: {
      show_debug_information: false,
      default_terminal: 'power_shell',
      plugin_theme: 'night',
    },
  });
  await vi.waitFor(() => expect(radios(group)[1]).toEqual(['夜间', true]));
  await user.click(within(group).getByRole('radio', { name: 'Aura 默认' }));
  expect(update).toHaveBeenLastCalledWith({
    expected_revision: 5,
    settings: {
      show_debug_information: false,
      default_terminal: 'power_shell',
      plugin_theme: null,
    },
  });
});

test('a chosen theme that cannot be used now stays shown as chosen', async () => {
  const onChoose = vi.fn();
  const { rerender } = render(<ThemeChoice chosen="gone" disabled={false} onChoose={onChoose} />);
  const group = screen.getByRole('radiogroup', { name: '主题' });
  // Before the plugins are read, it is shown by its name.
  expect(radios(group)).toEqual([
    ['Aura 默认', false],
    ['gone', true],
  ]);
  await within(group).findByRole('radio', { name: 'gone（现在不能用）' });
  expect(radios(group)).toEqual([
    ['Aura 默认', false],
    ['夜间', false],
    ['gone（现在不能用）', true],
  ]);
  rerender(<ThemeChoice chosen="night" disabled onChoose={onChoose} />);
  expect(radios(group)).toEqual([
    ['Aura 默认', false],
    ['夜间', true],
  ]);
  expect(within(group).getByRole('radio', { name: '夜间' })).toHaveProperty('disabled', true);
});
