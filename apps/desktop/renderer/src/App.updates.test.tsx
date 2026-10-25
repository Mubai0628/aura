import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test, vi } from 'vitest';
import type { AppUpdateCommand, AppUpdateState } from '../../shared/app-updates';
import { App } from './App';
import { installAppTestEnvironment } from './app-test-fixtures';

installAppTestEnvironment();

function updateBridge(change: Partial<AppUpdateState> = {}) {
  let state: AppUpdateState = {
    available: true,
    current: '0.1.0',
    previous: '0.0.9',
    waiting: null,
    phase: 'idle',
    latest: null,
    progress: null,
    error: null,
    checked_at: null,
    auto_check: true,
    notice: null,
    ...change,
  };
  const listeners = new Set<(next: AppUpdateState) => void>();
  const command = vi.fn(async (value: AppUpdateCommand) => {
    if (value.command === 'dismiss_notice') {
      state = { ...state, notice: null };
      listeners.forEach((listener) => listener(state));
    }
    return state;
  });
  const subscribe = vi.fn((listener: (next: AppUpdateState) => void) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  });
  Object.assign(window.aura.v1, { appUpdate: command, onAppUpdateState: subscribe });
  return {
    command,
    subscribe,
    push(change: Partial<AppUpdateState>) {
      state = { ...state, ...change };
      listeners.forEach((listener) => listener(state));
    },
  };
}

test('App receives update notices outside Settings and shares the live subscription with the page', async () => {
  const bridge = updateBridge();
  render(<App />);
  await waitFor(() => expect(bridge.command).toHaveBeenCalledWith({ command: 'state' }));
  act(() =>
    bridge.push({
      phase: 'available',
      latest: { version: '0.1.3', size: 1024, notes: '', published_at: null },
    }),
  );
  expect(await screen.findByText('发现新版本')).toBeTruthy();
  expect(screen.queryByRole('heading', { name: '关于与更新' })).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: '设置' }));
  await userEvent.click(screen.getByRole('button', { name: '关于与更新' }));
  expect(await screen.findByText('0.1.0')).toBeTruthy();
  expect(bridge.subscribe).toHaveBeenCalledTimes(1);
  expect(bridge.command.mock.calls.every(([value]) => value.command === 'state')).toBe(true);
});

test('App preserves a failed-update notice until explicitly acknowledged on the real Settings page', async () => {
  const bridge = updateBridge({
    notice: {
      version: '0.1.3',
      kept_version: '0.1.0',
      detail: '新版本未通过健康检查',
      going_back: false,
      data_restorable: true,
    },
  });
  render(<App />);
  const toast = await screen.findByText('更新失败，已自动退回 0.1.0');
  const article = toast.closest('article');
  expect(article).toBeTruthy();
  await userEvent.click(within(article!).getByRole('button', { name: '关闭消息' }));
  expect(bridge.command).not.toHaveBeenCalledWith({ command: 'dismiss_notice' });
  await userEvent.click(screen.getByRole('button', { name: '设置' }));
  await userEvent.click(screen.getByRole('button', { name: '关于与更新' }));
  expect(await screen.findByText('新版本未通过健康检查')).toBeTruthy();
  await userEvent.click(screen.getByRole('button', { name: '知道了' }));
  expect(bridge.command).toHaveBeenCalledWith({ command: 'dismiss_notice' });
  expect(screen.queryByText('新版本未通过健康检查')).toBeNull();
});
