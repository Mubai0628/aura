import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { HostHarness } from './host-harness';
import { resetTerminalStoreForTests } from './store';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';
import { resetUiSettingsForTests } from '../ui-settings-store';

vi.mock('./xterm-runtime', () => ({
  terminalEngine: () => ({ terminal: { focus: vi.fn(), options: {} } }),
  mountTerminal: () => () => {},
  disposeTerminalEngine: vi.fn(),
}));
const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const opening = '00000000-0000-4000-8000-000000000001';
const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
function fixture() {
  let finish!: (value: unknown) => void;
  const response = new Promise((resolve) => {
    finish = resolve;
  });
  const listeners = new Set<(value: unknown) => void>();
  const api = {
    open: vi.fn(() => response),
    beginOpen: vi.fn(() => response),
    cancelOpen: vi.fn(async () => ({ ok: true, value: 'accepted' })),
    onOpening: vi.fn((listener: (value: unknown) => void) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    }),
    list: vi.fn(async () => ({ ok: true, value: { terminals: [], next_cursor: null } })),
    onState: vi.fn(),
    onOutput: vi.fn(),
    close: vi.fn(),
  };
  vi.spyOn(crypto, 'randomUUID').mockReturnValue(opening);
  vi.stubGlobal('aura', {
    v1: {
      terminal: api,
      onCoreHealth: vi.fn(),
      getUiSettings: vi.fn(async () => ({
        ok: true,
        value: {
          revision: 1,
          settings: { show_debug_information: false, default_terminal: 'power_shell' },
          issue: null,
        },
      })),
    },
  });
  return {
    api,
    finish,
    progress: (phase: string, other: Partial<{ opening_id: string; session_id: string }> = {}) => {
      for (const listener of listeners)
        listener({
          contract_version: 1,
          opening_id: opening,
          runtime_instance_id: runtime,
          session_id: session,
          phase,
          ...other,
        });
    },
  };
}
beforeEach(() => {
  resetToolTabsForTests();
  resetTerminalStoreForTests();
  resetUiSettingsForTests();
});
// A terminal tab opens the default terminal (设置 → 环境) by itself; each case starts where
// clicking "新建终端" used to.
async function start(f: ReturnType<typeof fixture>) {
  toolTabs.open(session, 'terminal');
  const view = render(<HostHarness session={session} />);
  if (vi.isFakeTimers())
    for (let i = 0; i < 3; i += 1)
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
  await vi.waitFor(() => expect(f.api.beginOpen).toHaveBeenCalledTimes(1));
  return view;
}
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test('waiting for the real PowerShell verification is visible within 200 ms and cancellable', async () => {
  vi.useFakeTimers();
  const f = fixture();
  await start(f);
  act(() => f.progress('waiting_runtime'));
  await act(() => vi.advanceTimersByTimeAsync(200));
  expect(screen.getByText('正在校验 PowerShell 运行环境…')).not.toBeNull();
  act(() => f.progress('launching', { session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW' }));
  expect(screen.getByText('正在校验 PowerShell 运行环境…')).not.toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '取消打开' }));
  await act(async () => {
    await Promise.resolve();
  });
  expect(f.api.cancelOpen).toHaveBeenCalledWith({ contract_version: 1, opening_id: opening });
  expect(screen.getByText('正在取消打开…')).not.toBeNull();
  expect(screen.queryByText('已取消打开。')).toBeNull();
  await act(async () =>
    f.finish({
      ok: false,
      error: { code: 'terminal.open_cancelled', message: '已取消打开。', terminal: 'cancelled' },
    }),
  );
  expect(screen.getByText('已取消打开。')).not.toBeNull();
  expect(f.api.beginOpen).toHaveBeenCalledTimes(1);
  expect(f.api.open).not.toHaveBeenCalled();
});

test('launching disables cancellation and foreign opening progress cannot change this attempt', async () => {
  vi.useFakeTimers();
  const f = fixture();
  await start(f);
  act(() => f.progress('waiting_runtime'));
  await act(() => vi.advanceTimersByTimeAsync(200));
  act(() => f.progress('launching', { opening_id: '00000000-0000-4000-8000-000000000002' }));
  expect(screen.getByText('正在校验 PowerShell 运行环境…')).not.toBeNull();
  act(() => f.progress('launching'));
  expect(screen.queryByRole('button', { name: '取消打开' })).toBeNull();
  expect(screen.getByText('正在打开终端…')).not.toBeNull();
  expect(f.api.cancelOpen).not.toHaveBeenCalled();
  await act(async () =>
    f.finish({ ok: false, error: { code: 'fixture.failure', message: '打开失败。' } }),
  );
  expect(screen.getByRole('alert').textContent).toContain('打开失败。');
});

test('cancellation loses to the confirmed launch boundary without reporting a false cancelled state', async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.api.cancelOpen.mockResolvedValueOnce({ ok: true, value: 'too_late_to_cancel' });
  await start(f);
  act(() => f.progress('waiting_runtime'));
  await act(() => vi.advanceTimersByTimeAsync(200));
  fireEvent.click(screen.getByRole('button', { name: '取消打开' }));
  await act(async () => {
    await Promise.resolve();
  });
  expect(screen.queryByRole('button', { name: '取消打开' })).toBeNull();
  expect(screen.getByText('正在打开终端…')).not.toBeNull();
  expect(screen.queryByText('已取消打开。')).toBeNull();
  await act(async () =>
    f.finish({ ok: false, error: { code: 'fixture.failure', message: '打开失败。' } }),
  );
});

test('unmount requests cancellation of this pending attempt and drops its progress listener', async () => {
  const f = fixture();
  const mounted = await start(f);
  act(() => f.progress('waiting_runtime'));
  mounted.unmount();
  expect(f.api.cancelOpen).toHaveBeenCalledExactlyOnceWith({
    contract_version: 1,
    opening_id: opening,
  });
  await act(async () =>
    f.finish({
      ok: false,
      error: { code: 'terminal.open_cancelled', message: '已取消打开。', terminal: 'cancelled' },
    }),
  );
});

test('queued Core cancellation uses the bound original cancelled terminal without requiring a terminal-specific code', async () => {
  const f = fixture();
  await start(f);
  await act(async () =>
    f.finish({
      ok: false,
      error: { code: 'ipc.request_cancelled', message: 'Request cancelled', terminal: 'cancelled' },
    }),
  );
  expect(screen.getByText('已取消打开。')).not.toBeNull();
  expect(screen.queryByRole('alert')).toBeNull();
});

test('cancelled caller timeout with an unknown outcome never becomes confirmed opening cancellation', async () => {
  const f = fixture();
  await start(f);
  await act(async () =>
    f.finish({
      ok: false,
      error: {
        code: 'ipc.request_timeout',
        message: '结果未确认',
        terminal: 'cancelled',
        details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
      },
    }),
  );
  expect(screen.queryByText('已取消打开。')).toBeNull();
  expect(screen.getByRole('alert').textContent).toContain('结果未确认');
});

test('waiting for a launch slot is cancellable and never reports verification still running', async () => {
  vi.useFakeTimers();
  const f = fixture();
  await start(f);
  act(() => f.progress('waiting_launch'));
  await act(() => vi.advanceTimersByTimeAsync(200));
  expect(screen.getByText('正在等待启动终端…')).not.toBeNull();
  expect(screen.queryByText('正在校验 PowerShell 运行环境…')).toBeNull();
  expect(screen.getByRole('button', { name: '取消打开' })).not.toBeNull();
  await act(async () =>
    f.finish({
      ok: false,
      error: { code: 'ipc.request_cancelled', message: '已取消打开。', terminal: 'cancelled' },
    }),
  );
});
