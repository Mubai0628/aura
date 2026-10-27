import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChromeControlStatus } from '@aura/contracts/generated';

import { ChromeSettings } from './ChromeSettings';

const status = (over: Partial<ChromeControlStatus> = {}): ChromeControlStatus => ({
  enabled: true,
  revision: 3,
  issue: null,
  registered: true,
  paired: null,
  connection: 'waiting',
  pairing_expires_at: null,
  ...over,
});

let current: ChromeControlStatus;
const api = {
  status: vi.fn(async () => ({ ok: true as const, value: current })),
  setEnabled: vi.fn(async (request: { enabled: boolean }) => ({
    ok: true as const,
    value: { ...current, enabled: request.enabled, revision: 4 },
  })),
  beginPairing: vi.fn(async () => ({
    ok: true as const,
    value: { code: '12345678', expires_at: new Date(Date.now() + 300_000).toISOString() },
  })),
  revoke: vi.fn(async () => ({ ok: true as const, value: { ...current, paired: null } })),
  openExtensionFolder: vi.fn(async () => ({
    ok: true as const,
    value: { path: 'C:/Aura/resources/chrome-extension', opened: false },
  })),
};

beforeEach(() => {
  current = status();
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: { chrome: api } } });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('设置 → 网络与浏览器 → Chrome', () => {
  it('turns Chrome control on at the revision shown, and off only after asking', async () => {
    current = status({ enabled: false, connection: 'off', registered: false });
    const user = userEvent.setup();
    render(<ChromeSettings coreReady />);
    expect(await screen.findByText('已关闭')).toBeTruthy();
    await user.click(screen.getByRole('switch', { name: 'Chrome 控制' }));
    expect(api.setEnabled).toHaveBeenCalledWith({ expected_revision: 3, enabled: true });
    current = status({ revision: 4 });
    await screen.findByText('等待扩展连接');
    await user.click(screen.getByRole('switch', { name: 'Chrome 控制' }));
    expect(api.setEnabled).toHaveBeenCalledTimes(1);
    expect((await screen.findByRole('dialog')).textContent).toContain('已配对的扩展会被忘记');
    await user.click(screen.getByRole('button', { name: '关闭' }));
    await waitFor(() =>
      expect(api.setEnabled).toHaveBeenLastCalledWith({ expected_revision: 4, enabled: false }),
    );
  });

  it('shows a pairing code with the time it has left', async () => {
    const user = userEvent.setup();
    render(<ChromeSettings coreReady />);
    await user.click(await screen.findByRole('button', { name: '配对' }));
    expect(await screen.findByText('1234 5678')).toBeTruthy();
    expect(screen.getByRole('timer').textContent).toMatch(/剩余 0[45]:\d\d/u);
  });

  it('forgets the paired extension only after asking', async () => {
    current = status({
      connection: 'connected',
      paired: { extension_id: 'jhkgloifhjcbdiefjedcjnpjnlicijlg', paired_at: '2026-10-26' },
    });
    const user = userEvent.setup();
    render(<ChromeSettings coreReady />);
    expect(await screen.findByText('已连接')).toBeTruthy();
    expect(screen.getByText('jhkgloif…')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: '断开并忘记' }));
    expect(api.revoke).not.toHaveBeenCalled();
    await user.click(await screen.findByRole('button', { name: '断开' }));
    expect(api.revoke).toHaveBeenCalledTimes(1);
  });

  it('opens the extension folder and names it, also when it is missing', async () => {
    const user = userEvent.setup();
    render(<ChromeSettings coreReady />);
    await user.click(await screen.findByRole('button', { name: '打开扩展文件夹' }));
    expect(await screen.findByText('C:/Aura/resources/chrome-extension')).toBeTruthy();
    expect(screen.getByText(/没有找到扩展文件夹/u)).toBeTruthy();
  });

  it('says when the state cannot be read, and reads it again on 重试', async () => {
    api.status.mockResolvedValue({ ok: false, error: { message: 'request_timeout' } } as never);
    const user = userEvent.setup();
    render(<ChromeSettings coreReady />);
    expect((await screen.findByRole('alert')).textContent).toContain(
      '读不到 Chrome 控制的状态，开关暂不可用。',
    );
    expect(
      (screen.getByRole('switch', { name: 'Chrome 控制' }) as HTMLButtonElement).disabled,
    ).toBe(true);
    api.status.mockResolvedValue({ ok: true as const, value: current });
    await user.click(screen.getByRole('button', { name: '重试' }));
    expect(await screen.findByText('等待扩展连接')).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('shows a pairing request while it runs', async () => {
    let answer: (value: unknown) => void = () => undefined;
    api.beginPairing.mockImplementationOnce(
      () => new Promise((resolve) => (answer = resolve)) as never,
    );
    const user = userEvent.setup();
    render(<ChromeSettings coreReady />);
    await user.click(await screen.findByRole('button', { name: '配对' }));
    const waiting = await screen.findByRole('button', { name: '正在获取配对码…' });
    expect((waiting as HTMLButtonElement).disabled).toBe(true);
    answer({
      ok: true,
      value: { code: '12345678', expires_at: new Date(Date.now() + 300_000).toISOString() },
    });
    expect(await screen.findByText('1234 5678')).toBeTruthy();
  });

  it('keeps the newest state when an older read answers last', async () => {
    api.status.mockResolvedValueOnce({
      ok: false,
      error: { message: 'request_timeout' },
    } as never);
    const user = userEvent.setup();
    render(<ChromeSettings coreReady />);
    await screen.findByRole('alert');
    const answers: ((value: unknown) => void)[] = [];
    api.status.mockImplementation(() => new Promise((resolve) => answers.push(resolve)) as never);
    await user.click(screen.getByRole('button', { name: '重试' }));
    await user.click(screen.getByRole('button', { name: '重试' }));
    expect(answers).toHaveLength(2);
    answers[1]!({ ok: true, value: status({ revision: 3, connection: 'connected' }) });
    expect(await screen.findByText('已连接')).toBeTruthy();
    answers[0]!({ ok: true, value: status({ revision: 2, enabled: false, connection: 'off' }) });
    await waitFor(() => expect(screen.queryByText('已关闭')).toBeNull());
    expect(screen.getByText('已连接')).toBeTruthy();
    expect(
      (screen.getByRole('switch', { name: 'Chrome 控制' }) as HTMLButtonElement).getAttribute(
        'aria-checked',
      ),
    ).toBe('true');
    api.status.mockImplementation(async () => ({ ok: true as const, value: current }));
  });
});
