import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  ComputerControlSettings,
  ComputerControlSettingsSnapshot,
} from '@aura/contracts/generated';

import { ComputerSettings } from './ComputerSettings';

vi.mock('../agent-session/useDraftModelChoices', () => ({
  useDraftModelChoices: () => ({
    choices: [
      { id: 'mpr_01ARZ3NDEKTSV4RRFFQ69G5FA1', label: 'GPT', provider: 'OpenAI' },
      { id: 'mpr_01ARZ3NDEKTSV4RRFFQ69G5FA2', label: 'Flash', provider: 'DeepSeek' },
    ],
    loading: false,
  }),
}));

const NOTEPAD = { app_id: 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App', name: '记事本' };
const defaults: ComputerControlSettings = {
  enabled: false,
  always_allowed: [],
  added: [],
  paint_others: true,
  clipboard: false,
  system_keys: false,
  screenshot_long_edge: 1568,
  models: [],
};
let saved: ComputerControlSettingsSnapshot;
const settings = vi.fn(async () => ({ ok: true as const, value: saved }));
const updateSettings = vi.fn(
  async (request: { expected_revision: number; settings: ComputerControlSettings }) => {
    saved = { revision: request.expected_revision + 1, settings: request.settings };
    return { ok: true as const, value: saved };
  },
);
const sent = () => updateSettings.mock.calls.at(-1)?.[0];

/** The page once it shows what it read. */
async function shown() {
  render(<ComputerSettings coreReady />);
  await screen.findByRole('button', { name: '移除 记事本' });
}

beforeEach(() => {
  saved = { revision: 3, settings: { ...defaults, always_allowed: [NOTEPAD] } };
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { computer: { settings, updateSettings } } },
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('设置 → 电脑操控', () => {
  it('chooses where new turns operate programs at the revision it read', async () => {
    await shown();
    const mode = await screen.findByRole('combobox', { name: '操控方式' });
    expect(mode).toHaveProperty('value', 'foreground');
    expect(
      within(mode)
        .getAllByRole('option')
        .map((option) => option.textContent),
    ).toEqual(['前台', '小窗']);
    await userEvent.selectOptions(mode, '小窗');
    expect(sent()).toEqual({
      expected_revision: 3,
      settings: { ...saved.settings, mode: 'child_session' },
    });
    expect(screen.getByRole('combobox', { name: '操控方式' })).toHaveProperty(
      'value',
      'child_session',
    );
  });

  it('saves the stop shortcut with the current settings revision', async () => {
    await shown();
    const keys = await screen.findByRole('textbox', { name: '停止快捷键' });
    expect(keys).toHaveProperty('value', 'Ctrl+Alt+Q');
    await userEvent.clear(keys);
    await userEvent.type(keys, 'Ctrl+Alt+P');
    await userEvent.click(screen.getByRole('button', { name: '保存快捷键' }));
    expect(sent()).toEqual({
      expected_revision: 3,
      settings: { ...saved.settings, pip_stop_keys: 'Ctrl+Alt+P' },
    });
  });
  it('turns computer control on at the revision it read', async () => {
    await shown();
    const toggle = await screen.findByRole('switch', { name: '电脑操控' });
    await vi.waitFor(() => expect(toggle.hasAttribute('disabled')).toBe(false));
    await userEvent.click(toggle);
    expect(sent()).toEqual({
      expected_revision: 3,
      settings: { ...saved.settings, enabled: true },
    });
    expect(saved.settings.always_allowed).toEqual([NOTEPAD]);
  });

  it('keeps the shortcut draft and Core failure guidance when it was refused', async () => {
    updateSettings.mockResolvedValueOnce({
      ok: false,
      error: { code: 'computer_settings.invalid', message: '快捷键必须包含 Ctrl 或 Alt' },
    } as never);
    await shown();
    const keys = await screen.findByRole('textbox', { name: '停止快捷键' });
    await userEvent.clear(keys);
    await userEvent.type(keys, 'Q');
    await userEvent.click(screen.getByRole('button', { name: '保存快捷键' }));
    expect((await screen.findByRole('alert')).textContent).toContain('快捷键必须包含 Ctrl 或 Alt');
    expect(keys).toHaveProperty('value', 'Q');
    expect(saved.settings.pip_stop_keys).toBeUndefined();
  });

  it('takes a program off the always-allowed list and adds one by its path', async () => {
    await shown();
    await userEvent.click(await screen.findByRole('button', { name: '移除 记事本' }));
    expect(sent()?.settings.always_allowed).toEqual([]);
    expect(await screen.findByText('无')).toBeTruthy();
    await userEvent.type(screen.getByLabelText('程序路径'), '"C:\\Tools\\viewer.exe"');
    await userEvent.click(screen.getByRole('button', { name: '添加' }));
    expect(sent()?.settings.added).toEqual([{ path: 'C:\\Tools\\viewer.exe' }]);
    expect(await screen.findByText('viewer.exe')).toBeTruthy();
  });

  it('names the models that may use the tools, and none named is every model that takes images', async () => {
    await shown();
    const models = await screen.findByRole('region', { name: '可用的模型' });
    expect(within(models).getByRole('radio', { name: '能看图的模型' })).toHaveProperty(
      'checked',
      true,
    );
    await userEvent.click(within(models).getByRole('radio', { name: '指定的模型' }));
    expect(updateSettings).not.toHaveBeenCalled();
    await userEvent.click(within(models).getByRole('checkbox', { name: /Flash/u }));
    expect(sent()?.settings.models).toEqual(['mpr_01ARZ3NDEKTSV4RRFFQ69G5FA2']);
    await userEvent.click(within(models).getByRole('checkbox', { name: /Flash/u }));
    expect(sent()?.settings.models).toEqual([]);
    expect(within(models).getByRole('radio', { name: '能看图的模型' })).toHaveProperty(
      'checked',
      true,
    );
  });

  it('saves the screenshot size and the input switches', async () => {
    await shown();
    await userEvent.selectOptions(await screen.findByLabelText('截图长边'), '2560');
    expect(sent()?.settings.screenshot_long_edge).toBe(2560);
    await userEvent.click(screen.getByRole('switch', { name: '允许系统组合键' }));
    expect(sent()?.settings.system_keys).toBe(true);
    const input = screen.getByRole('region', { name: '输入' });
    expect(
      within(input)
        .getAllByRole('switch')
        .map((one) => one.getAttribute('aria-label')),
    ).toEqual(['允许使用剪贴板', '允许系统组合键']);
    expect(input.querySelector('p')).toBeNull();
  });

  it('reads the settings again when they changed elsewhere, and says so', async () => {
    updateSettings.mockResolvedValueOnce({
      ok: false,
      error: {
        code: 'computer_settings.stale',
        message: 'computer_settings.stale: 设置已在别处更改，请刷新后再改。',
      },
    } as never);
    await shown();
    const clipboard = await screen.findByRole('switch', { name: '允许使用剪贴板' });
    await vi.waitFor(() => expect(clipboard.hasAttribute('disabled')).toBe(false));
    await userEvent.click(clipboard);
    await vi.waitFor(() => expect(settings).toHaveBeenCalledTimes(2));
    expect((await screen.findByRole('alert')).textContent).toContain('设置已在别处更改');
  });
});
