import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BrowserSettingsSnapshot } from '@aura/contracts/generated';

import { BrowserSettings } from './BrowserSettings';

const shown = (
  over: Partial<BrowserSettingsSnapshot['settings']> = {},
): BrowserSettingsSnapshot => ({
  revision: 3,
  issue: null,
  settings: {
    enabled: true,
    allowed_sites: ['docs.rs'],
    blocked_sites: [],
    debugging: false,
    ...over,
  },
});

let getSettings: ReturnType<typeof vi.fn>;
let updateSettings: ReturnType<typeof vi.fn>;
let command: ReturnType<typeof vi.fn>;
beforeEach(() => {
  getSettings = vi.fn(async () => ({ ok: true, value: shown() }));
  updateSettings = vi.fn(async (request: { settings: BrowserSettingsSnapshot['settings'] }) => ({
    ok: true,
    value: { ...shown(), revision: 4, settings: request.settings },
  }));
  command = vi.fn(async () => ({ ok: true }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: { browser: { getSettings, updateSettings, command, pages: vi.fn(), onState: vi.fn() } },
    },
  });
});
afterEach(() => cleanup());

describe('设置 → 网络与浏览器', () => {
  it('saves the switch and the site lists at the revision shown', async () => {
    const user = userEvent.setup();
    render(<BrowserSettings coreReady />);
    await screen.findByText('docs.rs');
    await user.click(screen.getByRole('switch', { name: '启用内置浏览器' }));
    expect(updateSettings).toHaveBeenLastCalledWith({
      expected_revision: 3,
      settings: { enabled: false, allowed_sites: ['docs.rs'], blocked_sites: [], debugging: false },
    });
    await user.type(screen.getByLabelText('添加到阻止的网站'), '*.tracker.example{Enter}');
    expect(updateSettings).toHaveBeenLastCalledWith({
      expected_revision: 4,
      settings: {
        enabled: false,
        allowed_sites: ['docs.rs'],
        blocked_sites: ['*.tracker.example'],
        debugging: false,
      },
    });
    await user.click(await screen.findByRole('button', { name: '移除 docs.rs' }));
    expect(updateSettings.mock.lastCall?.[0].settings.allowed_sites).toEqual([]);
  });

  it('names the list and the site Core refused, and keeps what was typed', async () => {
    const user = userEvent.setup();
    updateSettings.mockResolvedValueOnce({
      ok: false,
      error: {
        code: 'main_request_failed',
        message:
          'browser_settings.invalid: allowed_sites: “a b”：网站名无效：只能是域名、*.域名、IPv4 地址或 [IPv6] 地址',
      },
    });
    render(<BrowserSettings coreReady />);
    await screen.findByText('docs.rs');
    const input = screen.getByLabelText('添加到只读会话直接打开的网站') as HTMLInputElement;
    await user.type(input, 'a b{Enter}');
    expect((await screen.findByRole('alert')).textContent).toContain(
      '只读会话直接打开的网站：“a b”',
    );
    expect(input.value).toBe('a b');
  });

  it('turns debugging on only after the user reads the risk and agrees', async () => {
    const user = userEvent.setup();
    render(<BrowserSettings coreReady />);
    await screen.findByText('docs.rs');
    expect(screen.getByText(/运行它写的脚本/u)).toBeTruthy();
    await user.click(screen.getByRole('switch', { name: '允许模型调试网页' }));
    expect(updateSettings).not.toHaveBeenCalled();
    const dialog = await screen.findByRole('dialog');
    expect(dialog.textContent).toContain('登录状态');
    await user.click(screen.getByRole('button', { name: '允许' }));
    await waitFor(() => expect(updateSettings.mock.lastCall?.[0].settings.debugging).toBe(true));
  });

  it('clears the browsing data after asking', async () => {
    const user = userEvent.setup();
    render(<BrowserSettings coreReady />);
    await screen.findByText('docs.rs');
    await user.click(screen.getByRole('button', { name: '清除浏览数据' }));
    expect(command).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '清除' }));
    expect(command).toHaveBeenCalledWith({ kind: 'clear-data' });
  });

  it('shows the issue of settings Core cannot trust, with nothing switched on', async () => {
    getSettings.mockResolvedValueOnce({
      ok: true,
      value: {
        revision: null,
        issue: '浏览器设置不可用，内置浏览器已暂停。请重试、恢复上一版有效配置或重置此设置域。',
        settings: { enabled: false, allowed_sites: [], blocked_sites: [], debugging: false },
      },
    });
    render(<BrowserSettings coreReady />);
    expect((await screen.findByRole('alert')).textContent).toContain('内置浏览器已暂停');
    expect(
      screen.getByRole('switch', { name: '启用内置浏览器' }).getAttribute('aria-checked'),
    ).toBe('false');
  });
});
