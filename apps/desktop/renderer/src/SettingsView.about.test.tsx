import { act, cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import type { ComponentProps } from 'react';
import type { AppUpdateCommand, AppUpdateState } from '../../shared/app-updates';
import { SettingsView } from './SettingsView';

vi.mock('./ModelSettings', () => ({ ModelSettings: () => null }));
vi.mock('./ToolSettings', () => ({ ToolSettings: () => null }));
vi.mock('./SecuritySettings', () => ({ SecuritySettings: () => null }));
vi.mock('./settings-recovery/ConfigRecoveryPanel', () => ({ ConfigRecoveryPanel: () => null }));
vi.mock('./ProjectionCompatibilityNotice', () => ({ ProjectionCompatibilityNotice: () => null }));

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function fixture(change: Partial<AppUpdateState> = {}) {
  return {
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
    rollback_needs_data: false,
    ...change,
  } satisfies AppUpdateState;
}
const latest = {
  version: '0.1.3',
  size: 1024,
  published_at: null,
  notes: '<script>untrusted</script>',
};

function setup(initial = fixture(), section = 'about') {
  let state = initial;
  const listeners = new Set<(next: AppUpdateState) => void>();
  const push = (next: AppUpdateState) => {
    state = next;
    listeners.forEach((listener) => listener(next));
  };
  const command = vi.fn(async (value: AppUpdateCommand) => {
    if (value.command === 'set_auto_check') state = { ...state, auto_check: value.enabled };
    return state;
  });
  vi.stubGlobal('aura', {
    v1: {
      appUpdate: command,
      onAppUpdateState: (listener: (next: AppUpdateState) => void) => {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
    },
  });
  const change = vi.fn();
  render(
    <SettingsView
      coreReady={false}
      coreStatus={{ label: 'Core 未连接', tone: 'muted' }}
      onChangeSection={change}
      onClose={vi.fn()}
      onNotify={vi.fn()}
      onWithdrawNotice={vi.fn()}
      section={section as ComponentProps<typeof SettingsView>['section']}
      security={{} as ComponentProps<typeof SettingsView>['security']}
    />,
  );
  return { command, push, change };
}

test('system settings expose About and Updates without removing Commands, including search', async () => {
  const { change } = setup(fixture(), 'tools');
  await userEvent.click(screen.getByRole('button', { name: '关于与更新' }));
  expect(change).toHaveBeenCalledWith('about');
  expect(screen.getByRole('button', { name: '命令' })).toBeTruthy();
  await userEvent.type(screen.getByRole('searchbox'), '版本');
  expect(screen.getByRole('button', { name: '关于与更新' })).toBeTruthy();
});

test('reads the actual Main version and settings even when Core is disconnected', async () => {
  const { command } = setup();
  expect(await screen.findByText('0.1.0')).toBeTruthy();
  expect(command).toHaveBeenCalledWith({ command: 'state' });
  await userEvent.click(screen.getByRole('button', { name: '检查更新' }));
  expect(command).toHaveBeenCalledWith({ command: 'check' });
  await userEvent.click(screen.getByRole('switch', { name: '自动检查更新' }));
  expect(command).toHaveBeenCalledWith({ command: 'set_auto_check', enabled: false });
  expect(screen.getByRole('switch').getAttribute('aria-checked')).toBe('false');
  expect(screen.queryByText(/每天|GitHub|代理|不发送你的/u)).toBeNull();
});

test.each([
  ['idle', '尚未检查'],
  ['checking', '正在检查更新'],
  ['up_to_date', '已是最新版本'],
  ['available', '发现新版本'],
  ['downloading', '正在下载'],
  ['paused', '下载已暂停'],
  ['applying', '正在校验与安装'],
  ['ready', '等待重启'],
  ['error', '更新失败'],
] as const)('projects Main phase %s as a real status', async (phase, label) => {
  setup(fixture({ phase, latest, waiting: phase === 'ready' ? '0.1.3' : null }));
  expect(await screen.findByText(label)).toBeTruthy();
  if (['checking', 'downloading', 'applying'].includes(phase))
    expect((screen.getByRole('button', { name: '检查更新' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
});

test('unavailable installs retain the version but cannot check, download, restart or roll back', async () => {
  setup(fixture({ available: false, phase: 'available', latest, waiting: '0.1.2' }));
  expect(await screen.findByText('此版本不支持应用内更新')).toBeTruthy();
  for (const name of ['检查更新', '下载更新', '重启并更新', '回到上一版本'])
    expect((screen.getByRole('button', { name }) as HTMLButtonElement).disabled).toBe(true);
});

test('download can be paused while its real RPC is still pending and resumed from Main state', async () => {
  const { command, push } = setup(fixture({ phase: 'available', latest }));
  await screen.findByText('发现新版本');
  let finish: ((next: AppUpdateState) => void) | undefined;
  command.mockImplementation(async (value) => {
    if (value.command === 'download') {
      push(fixture({ phase: 'downloading', latest, progress: { done: 256, total: 1024 } }));
      return new Promise<AppUpdateState>((resolve) => {
        finish = resolve;
      });
    }
    if (value.command === 'pause') {
      const paused = fixture({ phase: 'paused', latest, progress: { done: 256, total: 1024 } });
      push(paused);
      finish?.(paused);
      return paused;
    }
    return fixture();
  });
  await userEvent.click(screen.getByRole('button', { name: '下载更新' }));
  expect(
    (screen.getByRole('progressbar', { name: '更新下载进度' }) as HTMLProgressElement).value,
  ).toBe(256);
  await userEvent.click(screen.getByRole('button', { name: '暂停下载' }));
  expect(command).toHaveBeenCalledWith({ command: 'pause' });
  expect(await screen.findByRole('button', { name: '继续下载' })).toBeTruthy();
});

test('a download the network broke says so, keeps its progress and continues from there', async () => {
  const reason =
    '下载中断，可能是网络或代理暂时不通。已下载的部分会保留，网络恢复后点“继续下载”从中断处接着下载。';
  const { command } = setup(
    fixture({ phase: 'paused', latest, progress: { done: 256, total: 1024 }, error: reason }),
  );
  expect(await screen.findByText('下载中断')).toBeTruthy();
  expect(screen.getByRole('alert').textContent).toBe(reason);
  expect(screen.queryByText(/重新运行安装程序/u)).toBeNull();
  expect(
    (screen.getByRole('progressbar', { name: '更新下载进度' }) as HTMLProgressElement).value,
  ).toBe(256);
  await userEvent.click(screen.getByRole('button', { name: '继续下载' }));
  expect(command).toHaveBeenCalledWith({ command: 'download' });
});

test('a waiting version remains restartable after a later check fails, only after confirmation', async () => {
  const { command } = setup(
    fixture({ phase: 'error', waiting: '0.1.3', error: '检查更新失败（HTTP 503）' }),
  );
  await screen.findByText('0.1.0');
  expect(command).not.toHaveBeenCalledWith({ command: 'restart' });
  await userEvent.click(screen.getByRole('button', { name: '重启并更新' }));
  const dialog = screen.getByRole('dialog', { name: '重启并更新' });
  expect(within(dialog).getByText(/运行中的任务/u)).toBeTruthy();
  await userEvent.click(within(dialog).getByRole('button', { name: '重启并更新' }));
  expect(command).toHaveBeenCalledWith({ command: 'restart' });
  expect(screen.queryByRole('dialog', { name: '重启并更新' })).toBeNull();
});

test('rollback defaults to program only and data restoration is explicitly confirmed', async () => {
  const { command } = setup();
  await screen.findByText('0.1.0');
  await userEvent.click(screen.getByRole('button', { name: '回到上一版本' }));
  let dialog = screen.getByRole('dialog', { name: '回到上一版本' });
  expect((within(dialog).getByRole('checkbox') as HTMLInputElement).checked).toBe(false);
  await userEvent.click(within(dialog).getByRole('button', { name: '确认回退' }));
  expect(command).toHaveBeenCalledWith({ command: 'roll_back', restore_data: false });
  await userEvent.click(screen.getByRole('button', { name: '回到上一版本' }));
  dialog = screen.getByRole('dialog', { name: '回到上一版本' });
  await userEvent.click(within(dialog).getByRole('checkbox', { name: '同时恢复更新前的数据' }));
  expect(within(dialog).getByText(/更新之后的新会话和改动/u)).toBeTruthy();
  await userEvent.click(within(dialog).getByRole('button', { name: '恢复数据并回退' }));
  expect(command).toHaveBeenCalledWith({ command: 'roll_back', restore_data: true });
});

test('after an update that changed the data, going back always takes the data from before', async () => {
  const { command } = setup(fixture({ rollback_needs_data: true }));
  await screen.findByText('0.1.0');
  await userEvent.click(screen.getByRole('button', { name: '回到上一版本' }));
  const dialog = screen.getByRole('dialog', { name: '回到上一版本' });
  const box = within(dialog).getByRole('checkbox', {
    name: '同时恢复更新前的数据',
  }) as HTMLInputElement;
  expect(box.checked).toBe(true);
  expect(box.disabled).toBe(true);
  expect(within(dialog).getByText(/升级了数据/u)).toBeTruthy();
  await userEvent.click(within(dialog).getByRole('button', { name: '恢复数据并回退' }));
  expect(command).toHaveBeenCalledWith({ command: 'roll_back', restore_data: true });
});

test('automatic fallback notice keeps the real reason and is acknowledged explicitly', async () => {
  const { command, push } = setup(
    fixture({
      notice: {
        version: '0.1.3',
        kept_version: '0.1.0',
        detail: '新版本未能通过健康检查',
        going_back: false,
        data_restorable: false,
      },
    }),
  );
  expect(await screen.findByText('新版本未能通过健康检查')).toBeTruthy();
  expect(command).not.toHaveBeenCalledWith({ command: 'dismiss_notice' });
  await userEvent.click(screen.getByRole('button', { name: '知道了' }));
  expect(command).toHaveBeenCalledWith({ command: 'dismiss_notice' });
  act(() => push(fixture()));
  expect(screen.queryByText('新版本未能通过健康检查')).toBeNull();
});

// Main takes the data request back when the rollback fails, and says so when it cannot.
test('failed data rollback keeps the selected option and shows why, without restarting', async () => {
  const { command, push } = setup();
  await screen.findByText('0.1.0');
  command.mockImplementation(async () => {
    const failed = fixture({ phase: 'error', error: '安装程序未能完成回退' });
    push(failed);
    return failed;
  });
  await userEvent.click(screen.getByRole('button', { name: '回到上一版本' }));
  const dialog = screen.getByRole('dialog', { name: '回到上一版本' });
  await userEvent.click(within(dialog).getByRole('checkbox'));
  await userEvent.click(within(dialog).getByRole('button', { name: '恢复数据并回退' }));
  expect((within(dialog).getByRole('checkbox') as HTMLInputElement).checked).toBe(true);
  expect(await within(dialog).findByText('安装程序未能完成回退')).toBeTruthy();
  expect(within(dialog).queryByText(/重新启动可能会恢复更新前的数据/u)).toBeNull();
  expect((within(dialog).getByRole('checkbox') as HTMLInputElement).disabled).toBe(false);
  expect(command).not.toHaveBeenCalledWith({ command: 'restart' });
});

test('cancelled confirmations do not restart or roll back and release notes remain plain text', async () => {
  const { command } = setup(fixture({ phase: 'ready', latest, waiting: '0.1.3' }));
  await screen.findByText('0.1.0');
  await userEvent.click(screen.getByRole('button', { name: '重启并更新' }));
  await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: '取消' }));
  await userEvent.click(screen.getByRole('button', { name: '回到上一版本' }));
  await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: '取消' }));
  expect(command.mock.calls.every(([value]) => value.command === 'state')).toBe(true);
  await userEvent.click(screen.getByText('更新内容'));
  expect(screen.getByText(latest.notes)).toBeTruthy();
  expect(document.querySelector('script')).toBeNull();
});

test('a changed previous version invalidates the open rollback confirmation', async () => {
  const { command, push } = setup();
  await screen.findByText('0.1.0');
  await userEvent.click(screen.getByRole('button', { name: '回到上一版本' }));
  act(() => push(fixture({ previous: '0.0.8' })));
  const confirm = within(screen.getByRole('dialog')).getByRole('button', { name: '确认回退' });
  expect((confirm as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText('版本状态已变化，请关闭后重新确认。')).toBeTruthy();
  expect(command).not.toHaveBeenCalledWith({ command: 'roll_back', restore_data: false });
});

test('a changed waiting version invalidates the open restart confirmation', async () => {
  const { command, push } = setup(fixture({ phase: 'ready', waiting: '0.1.3' }));
  await screen.findByText('0.1.0');
  await userEvent.click(screen.getByRole('button', { name: '重启并更新' }));
  act(() => push(fixture({ phase: 'ready', waiting: '0.1.4' })));
  const confirm = within(screen.getByRole('dialog')).getByRole('button', { name: '重启并更新' });
  expect((confirm as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText('版本状态已变化，请关闭后重新确认。')).toBeTruthy();
  expect(command).not.toHaveBeenCalledWith({ command: 'restart' });
});
