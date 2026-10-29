import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useCallback } from 'react';
import { afterEach, expect, test, vi } from 'vitest';
import {
  UNAVAILABLE_UPDATE_STATE,
  type AppUpdateCommand,
  type AppUpdateState,
} from '../../../shared/app-updates';
import { NotificationViewport } from '../NotificationCenter';
import { useNotificationCenter, type NotificationInput } from '../notification-center-store';
import { UpdateNotifications } from './UpdateNotifications';
import { useAppUpdates } from './useAppUpdates';

const state = (change: Partial<AppUpdateState> = {}): AppUpdateState => ({
  ...UNAVAILABLE_UPDATE_STATE,
  available: true,
  current: '1.0.0',
  auto_check: true,
  ...change,
});
const release = { version: '2.0.0', published_at: null, size: 100, notes: '更新说明' };
const notice = {
  version: '2.0.0',
  kept_version: '1.0.0',
  detail: '签名校验失败。\n完整原因保留在这里。',
  going_back: false,
  data_restorable: false,
};
function bridge(initial = state()) {
  const listeners = new Set<(value: AppUpdateState) => void>();
  const appUpdate = vi.fn<(command: AppUpdateCommand) => Promise<AppUpdateState>>(
    async () => initial,
  );
  const onAppUpdateState = vi.fn((listener: (value: AppUpdateState) => void) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { appUpdate, onAppUpdateState } },
  });
  return {
    appUpdate,
    onAppUpdateState,
    push: (value: AppUpdateState) => act(() => listeners.forEach((listener) => listener(value))),
  };
}
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

test('announces an available release once across repeated progress pushes and callback changes, without acting on it', async () => {
  const api = bridge(),
    notify = vi.fn(),
    withdraw = vi.fn();
  const view = render(<UpdateNotifications onNotify={notify} onWithdraw={withdraw} />);
  await act(async () => {});
  const available = state({ phase: 'available', latest: release });
  api.push(available);
  expect(notify).toHaveBeenCalledTimes(1);
  expect(notify.mock.calls[0]![0]).toMatchObject({
    tone: 'info',
    title: expect.stringContaining('新版本'),
    message: expect.stringContaining('2.0.0'),
  });
  expect(notify.mock.calls[0]![0].message).toContain('设置 → 关于与更新');
  api.push({ ...available });
  api.push({ ...available, checked_at: '2026-10-25T02:00:00Z' });
  const next = vi.fn();
  view.rerender(<UpdateNotifications onNotify={next} onWithdraw={withdraw} />);
  expect(notify).toHaveBeenCalledTimes(1);
  expect(next).not.toHaveBeenCalled();
  expect(api.appUpdate.mock.calls.map(([command]) => command.command)).toEqual(['state']);
});

test('a release downloaded and prepared in the background is announced once, as ready to restart into', async () => {
  const api = bridge(),
    notify = vi.fn(),
    withdraw = vi.fn();
  render(<UpdateNotifications onNotify={notify} onWithdraw={withdraw} />);
  await act(async () => {});
  const ready = state({ phase: 'ready', waiting: '2.0.0' });
  api.push(ready);
  api.push({ ...ready });
  expect(notify).toHaveBeenCalledTimes(1);
  expect(notify.mock.calls[0]![0]).toMatchObject({
    tone: 'info',
    title: '新版本已准备好',
    message: expect.stringContaining('2.0.0 已下载'),
  });
  expect(notify.mock.calls[0]![0].message).toContain('设置 → 关于与更新');
});

function Harness({
  notify,
  withdraw,
}: {
  notify: (input: NotificationInput) => void;
  withdraw: (prefix: string) => void;
}) {
  const {
      publish: publishNotice,
      withdraw: withdrawNotices,
      items,
      dismiss,
    } = useNotificationCenter(),
    updates = useAppUpdates();
  const publish = useCallback(
    (input: NotificationInput) => {
      notify(input);
      publishNotice(input);
    },
    [notify, publishNotice],
  );
  const remove = useCallback(
    (prefix: string) => {
      withdraw(prefix);
      withdrawNotices(prefix);
    },
    [withdraw, withdrawNotices],
  );
  return (
    <>
      <UpdateNotifications onNotify={publish} onWithdraw={remove} />
      <NotificationViewport items={items} onDismiss={dismiss} />
      <button type="button" onClick={() => void updates.send({ command: 'dismiss_notice' })}>
        已知晓
      </button>
    </>
  );
}

test('an automatic fallback notice is a persistent danger with its full detail and never auto-acknowledges', async () => {
  vi.useFakeTimers();
  const api = bridge(state({ notice })),
    notify = vi.fn(),
    withdraw = vi.fn();
  render(<Harness notify={notify} withdraw={withdraw} />);
  await act(async () => {});
  const alert = screen.getByRole('alert');
  expect(alert.textContent).toContain('1.0.0');
  expect(alert.textContent).toContain('2.0.0');
  expect(alert.textContent).toContain(notice.detail);
  expect(notify.mock.calls[0]![0].tone).toBe('danger');
  expect(api.onAppUpdateState).toHaveBeenCalledTimes(1);
  expect(api.appUpdate).toHaveBeenCalledTimes(1);
  act(() => vi.advanceTimersByTime(24 * 60 * 60 * 1000));
  expect(screen.getByRole('alert')).toBeTruthy();
  expect(api.appUpdate.mock.calls.map(([command]) => command.command)).toEqual(['state']);
});

test('repeated fallback pushes do not re-notify and explicit dismiss withdraws the notification', async () => {
  const api = bridge(state({ notice })),
    notify = vi.fn(),
    withdraw = vi.fn();
  render(<Harness notify={notify} withdraw={withdraw} />);
  await act(async () => {});
  api.push(state({ notice: { ...notice }, phase: 'checking' }));
  api.push(state({ notice: { ...notice }, phase: 'error', error: '检查失败' }));
  expect(notify).toHaveBeenCalledTimes(1);
  const key = notify.mock.calls[0]![0].dedupeKey as string;
  api.appUpdate.mockResolvedValueOnce(state({ notice: null }));
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: '已知晓' }));
  });
  expect(screen.queryByRole('alert')).toBeNull();
  expect(withdraw).toHaveBeenCalledWith(expect.stringMatching(/^app-update:notice:/u));
  const prefix = withdraw.mock.calls.at(-1)![0] as string;
  expect(key.startsWith(prefix)).toBe(true);
  expect(api.appUpdate.mock.calls.map(([command]) => command.command)).toEqual([
    'state',
    'dismiss_notice',
  ]);
});

test('changed fallback detail replaces the old notice while a cleared pushed notice withdraws it', async () => {
  const api = bridge(),
    notify = vi.fn(),
    withdraw = vi.fn();
  render(<UpdateNotifications onNotify={notify} onWithdraw={withdraw} />);
  await act(async () => {});
  api.push(state({ notice }));
  api.push(state({ notice: { ...notice, detail: '另一个完整失败原因' } }));
  expect(notify).toHaveBeenCalledTimes(2);
  expect(notify.mock.calls[1]![0].message).toContain('另一个完整失败原因');
  expect(withdraw).toHaveBeenCalledWith(expect.stringMatching(/^app-update:notice:/u));
  const count = withdraw.mock.calls.length;
  api.push(state());
  expect(withdraw.mock.calls.length).toBeGreaterThan(count);
});

test('a failed dismiss acknowledgement does not withdraw a still-active fallback notice', async () => {
  const api = bridge(state({ notice })),
    notify = vi.fn(),
    withdraw = vi.fn();
  render(<Harness notify={notify} withdraw={withdraw} />);
  await act(async () => {});
  api.appUpdate.mockResolvedValueOnce(UNAVAILABLE_UPDATE_STATE);
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: '已知晓' }));
  });
  expect(screen.getByRole('alert').textContent).toContain(notice.detail);
  expect(withdraw).not.toHaveBeenCalled();
});

test('a failed older-version rollback says it returned and maps internal launcher detail to a Chinese instruction', async () => {
  const api = bridge(
      state({
        notice: {
          ...notice,
          version: '0.9.0',
          going_back: true,
          data_restorable: true,
          detail: 'launcher.restore_failed: internal backend failure',
        },
      }),
    ),
    notify = vi.fn(),
    withdraw = vi.fn();
  render(<UpdateNotifications onNotify={notify} onWithdraw={withdraw} />);
  await act(async () => {});
  const input = notify.mock.calls[0]![0] as NotificationInput;
  expect(input.title).toBe('回退失败，已返回 1.0.0');
  expect(input.message).not.toMatch(/launcher|internal|backend|更新失败/u);
  expect(input.message).toContain('设置 → 关于与更新');
  expect(input.tone).toBe('danger');
  expect(api.appUpdate).toHaveBeenCalledTimes(1);
});

test('a pending version is announced as ready to restart into, never as newly available, and an unavailable state says nothing', async () => {
  const api = bridge(state({ phase: 'ready', waiting: '2.0.0', latest: release })),
    notify = vi.fn(),
    withdraw = vi.fn();
  render(<UpdateNotifications onNotify={notify} onWithdraw={withdraw} />);
  await act(async () => {});
  expect(notify).toHaveBeenCalledTimes(1);
  expect(notify.mock.calls[0]![0].title).toBe('新版本已准备好');
  api.push(state({ available: false, phase: 'available', latest: release }));
  expect(notify).toHaveBeenCalledTimes(1);
  expect(api.appUpdate).toHaveBeenCalledTimes(1);
});
