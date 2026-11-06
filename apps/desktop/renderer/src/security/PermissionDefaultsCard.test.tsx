import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { PermissionDefaultsCard } from './PermissionDefaultsCard';

afterEach(cleanup);

const lost = {
  ok: false,
  error: {
    code: 'ipc.transport_lost',
    category: 'unavailable',
    message: 'Aura Core pipe I/O failed',
    retryable: true,
  },
};
const busy = {
  ok: false,
  error: {
    code: 'ipc.handler_overloaded',
    category: 'unavailable',
    message: 'Aura Core is already running the maximum number of concurrent read-only requests',
    retryable: true,
  },
};
const defaults = { ok: true, value: { revision: 4, mode: 'workspace_read_write' } };

function install(getPermissionDefaults: ReturnType<typeof vi.fn>) {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { getPermissionDefaults, setPermissionDefaults: vi.fn() } },
  });
}

test('a read the connection lost says why and what is unchanged, and 重试 reads again', async () => {
  const getPermissionDefaults = vi.fn().mockResolvedValueOnce(lost).mockResolvedValueOnce(defaults);
  install(getPermissionDefaults);
  render(<PermissionDefaultsCard coreReady onNotify={vi.fn()} />);
  const alert = await screen.findByRole('alert');
  expect(alert.textContent).toMatch(/连接中断/u);
  expect(alert.textContent).toMatch(/默认权限没有改变/u);
  await userEvent.click(screen.getByRole('button', { name: '重试' }));
  await screen.findByText('当前默认');
  expect(screen.queryByRole('alert')).toBeNull();
  expect(getPermissionDefaults).toHaveBeenCalledTimes(2);
});

test('the defaults are read again once Core is ready again after the connection came back', async () => {
  const getPermissionDefaults = vi.fn().mockResolvedValueOnce(lost).mockResolvedValueOnce(defaults);
  install(getPermissionDefaults);
  const view = render(<PermissionDefaultsCard coreReady onNotify={vi.fn()} />);
  await screen.findByRole('alert');
  view.rerender(<PermissionDefaultsCard coreReady={false} onNotify={vi.fn()} />);
  view.rerender(<PermissionDefaultsCard coreReady onNotify={vi.fn()} />);
  await screen.findByText('当前默认');
  expect(screen.queryByRole('alert')).toBeNull();
  expect(getPermissionDefaults).toHaveBeenCalledTimes(2);
});

test('nothing is read while Core is not ready', async () => {
  const getPermissionDefaults = vi.fn().mockResolvedValue(defaults);
  install(getPermissionDefaults);
  const view = render(<PermissionDefaultsCard coreReady={false} onNotify={vi.fn()} />);
  expect(getPermissionDefaults).not.toHaveBeenCalled();
  view.rerender(<PermissionDefaultsCard coreReady onNotify={vi.fn()} />);
  await screen.findByText('当前默认');
  expect(getPermissionDefaults).toHaveBeenCalledTimes(1);
});

test('a busy Core is named as busy, with the same retry', async () => {
  const getPermissionDefaults = vi.fn().mockResolvedValueOnce(busy).mockResolvedValueOnce(defaults);
  install(getPermissionDefaults);
  render(<PermissionDefaultsCard coreReady onNotify={vi.fn()} />);
  const alert = await screen.findByRole('alert');
  expect(alert.textContent).toMatch(/正忙/u);
  expect(alert.textContent).toMatch(/默认权限没有改变/u);
  await userEvent.click(screen.getByRole('button', { name: '重试' }));
  await screen.findByText('当前默认');
});
