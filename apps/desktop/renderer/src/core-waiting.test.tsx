import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, expect, test, vi } from 'vitest';
import type { CoreHealthProjectionChange } from '../../shared/aura-api';
import { App } from './App';
import { installAppTestEnvironment, readyProjection } from './app-test-fixtures';
import { resetCoreHealthStoreForTests } from './core-health-store';
import { CoreWaitingNotice } from './core-waiting';
import { waitedText } from './core-waiting-time';

installAppTestEnvironment();
beforeEach(() => resetCoreHealthStoreForTests());

const waitingFor = (seconds: number, revision = '3'): CoreHealthProjectionChange => ({
  ...readyProjection,
  revision,
  payload: {
    ...readyProjection.payload,
    waiting: { since: new Date(Date.now() - seconds * 1_000).toISOString() },
  },
});

function healthPushes() {
  let push: ((change: CoreHealthProjectionChange) => void) | undefined;
  Object.assign(window.aura.v1, {
    onCoreHealth: vi.fn((listener: (change: CoreHealthProjectionChange) => void) => {
      push = listener;
      return () => undefined;
    }),
  });
  return (change: CoreHealthProjectionChange) => act(() => push?.(change));
}

test('the wait reads in the turn duration units', () => {
  expect(waitedText(0)).toBe('0 秒');
  expect(waitedText(59)).toBe('59 秒');
  expect(waitedText(65)).toBe('1 分 5 秒');
});

test('while Main waits for Core, the status at the foot counts the wait; then it is ready again', async () => {
  const push = healthPushes();
  render(<App />);
  expect(await screen.findByText('Aura Core 已就绪')).toBeTruthy();
  push(waitingFor(12));
  const footer = () => document.querySelector('.core-footer-status')?.textContent;
  await waitFor(() => expect(footer()).toBe('Aura Core 正在处理，已等待 12 秒'));
  push({ ...readyProjection, revision: '4' });
  expect(await screen.findByText('Aura Core 已就绪')).toBeTruthy();
});

test('above the composer, the wait counts on and 立即重启 asks Main to restart Core', async () => {
  const push = healthPushes();
  const restartCore = vi.fn(async () => ({ ok: true as const, value: { restarted: true } }));
  Object.assign(window.aura.v1, { restartCore });
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    render(<CoreWaitingNotice />);
    await act(async () => Promise.resolve());
    expect(screen.queryByRole('status')).toBeNull();
    push(waitingFor(10));
    expect(screen.getByRole('status').textContent).toContain('已等待 10 秒');
    await act(async () => {
      vi.advanceTimersByTime(2_000);
      await Promise.resolve();
    });
    expect(screen.getByRole('status').textContent).toContain('已等待 12 秒');
    fireEvent.click(screen.getByRole('button', { name: '立即重启' }));
    expect(restartCore).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: '立即重启' })).toHaveProperty('disabled', true);
  } finally {
    vi.useRealTimers();
  }
});

test('a restart Main refused says why and can be tried again', async () => {
  const push = healthPushes();
  Object.assign(window.aura.v1, {
    restartCore: vi.fn(async () => ({
      ok: false as const,
      error: {
        category: 'unavailable',
        code: 'core_not_ready',
        correlation_id: 'cor_00000000000000000000000000',
        details: {},
        message: 'Aura Core 尚未就绪，这次操作没有执行；请等它就绪后重试。',
        retryable: true,
        terminal: 'failed',
      },
    })),
  });
  render(<CoreWaitingNotice />);
  push(waitingFor(15));
  fireEvent.click(await screen.findByRole('button', { name: '立即重启' }));
  expect(await screen.findByRole('alert')).toBeTruthy();
  expect(screen.getByRole('button', { name: '立即重启' })).toHaveProperty('disabled', false);
});

test('no notice while Core is starting, even if Main was waiting', async () => {
  const push = healthPushes();
  render(<CoreWaitingNotice />);
  push({
    ...waitingFor(30),
    payload: { ...waitingFor(30).payload, state: 'restarting', status_code: 'core.restarting' },
  });
  await act(async () => Promise.resolve());
  expect(screen.queryByRole('status')).toBeNull();
});
