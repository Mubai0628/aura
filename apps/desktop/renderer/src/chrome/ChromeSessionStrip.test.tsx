import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChromeSessionShares } from '@aura/contracts/generated';

import { ChromeSessionStrip, SHARES_POLL_MS } from './ChromeSessionStrip';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
let current: ChromeSessionShares;
const shares = vi.fn(async () => ({ ok: true as const, value: current }));
const stop = vi.fn(async () => ({ ok: true as const, value: {} }));
const finishAssist = vi.fn(async () => ({ ok: true as const, value: current }));

const tab = (over: Partial<ChromeSessionShares['tabs'][number]> = {}) => ({
  tab: 't1',
  title: 'Cart',
  url: 'https://shop.example/cart',
  state: 'active' as const,
  this_turn_only: false,
  assist_reason: null,
  ...over,
});

beforeEach(() => {
  current = { connected: true, tabs: [] };
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { chrome: { shares, stop, finishAssist } } },
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe('the conversation’s Chrome tabs above the message box', () => {
  it('say nothing while the conversation shares no tab', async () => {
    const { container } = render(<ChromeSessionStrip sessionId={S} />);
    await vi.waitFor(() => expect(shares).toHaveBeenCalledWith({ session_id: S }));
    expect(container.textContent).toBe('');
  });

  it('count the shared tabs, flag a site to confirm, and stop everything', async () => {
    current = {
      connected: true,
      tabs: [tab(), tab({ tab: 't2', state: 'awaiting_origin' })],
    };
    render(<ChromeSessionStrip sessionId={S} />);
    expect(await screen.findByText('Chrome · 2 个标签页')).toBeTruthy();
    expect(screen.getByText('等待确认网站')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '停止' }));
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it('show what the model asked the user to do in a tab, and pass on 完成', async () => {
    current = {
      connected: true,
      tabs: [tab({ state: 'with_user', assist_reason: '请在页面上登录' })],
    };
    render(<ChromeSessionStrip sessionId={S} />);
    expect(await screen.findByText('请在页面上登录')).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Chrome 等待你的操作' })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '完成' }));
    expect(finishAssist).toHaveBeenCalledWith({ session_id: S });
  });

  it('say why Stop did not take effect, and keep it for a retry', async () => {
    current = { connected: true, tabs: [tab()] };
    stop.mockResolvedValueOnce({
      ok: false as const,
      error: { message: 'chrome.not_connected' },
    } as never);
    render(<ChromeSessionStrip sessionId={S} />);
    await userEvent.click(await screen.findByRole('button', { name: '停止' }));
    expect((await screen.findByRole('alert')).textContent).toBe('Chrome 没有停止，请重试。');
    expect((screen.getByRole('button', { name: '停止' }) as HTMLButtonElement).disabled).toBe(
      false,
    );
    stop.mockResolvedValueOnce({ ok: true as const, value: {} });
    current = { connected: true, tabs: [] };
    await userEvent.click(screen.getByRole('button', { name: '停止' }));
    await vi.waitFor(() => expect(screen.queryByRole('status')).toBeNull());
  });

  it('show Stop and 完成 as pending until they settle, and say when 完成 failed', async () => {
    current = {
      connected: true,
      tabs: [tab({ state: 'with_user', assist_reason: '请在页面上登录' })],
    };
    let settle: () => void = () => undefined;
    finishAssist.mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          settle = () => reject(new Error('ipc closed'));
        }) as never,
    );
    render(<ChromeSessionStrip sessionId={S} />);
    await userEvent.click(await screen.findByRole('button', { name: '完成' }));
    const waiting = await screen.findByRole('button', { name: '正在交还…' });
    expect((waiting as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: '停止' }) as HTMLButtonElement).disabled).toBe(true);
    settle();
    expect((await screen.findByRole('alert')).textContent).toBe('没有交还给 Aura，请重试。');
    expect(screen.getByRole('button', { name: '完成' })).toBeTruthy();
  });

  it('drop a periodic read that answers after Stop has taken effect', async () => {
    vi.useFakeTimers();
    let late: (value: unknown) => void = () => undefined;
    current = { connected: true, tabs: [tab()] };
    shares
      .mockResolvedValueOnce({ ok: true as const, value: current })
      .mockImplementationOnce(() => new Promise((resolve) => (late = resolve)) as never);
    render(<ChromeSessionStrip sessionId={S} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByText('Chrome · 1 个标签页')).toBeTruthy();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SHARES_POLL_MS);
    });
    expect(shares).toHaveBeenCalledTimes(2);
    current = { connected: true, tabs: [] };
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '停止' }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(stop).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(/个标签页/u)).toBeNull();
    await act(async () => {
      late({ ok: true, value: { connected: true, tabs: [tab()] } });
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.queryByText(/个标签页/u)).toBeNull();
  });

  it('say when Chrome is no longer connected', async () => {
    current = { connected: false, tabs: [tab()] };
    render(<ChromeSessionStrip sessionId={S} />);
    expect(await screen.findByText('已断开')).toBeTruthy();
  });
});
