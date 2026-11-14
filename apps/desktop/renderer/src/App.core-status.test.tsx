import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, readyProjection } from './app-test-fixtures';
import { resetCoreHealthStoreForTests } from './core-health-store';

installAppTestEnvironment();
beforeEach(() => resetCoreHealthStoreForTests());
afterEach(() => {
  cleanup();
  vi.mocked(window.aura.v1.getCoreHealth).mockResolvedValue({ ok: true, value: readyProjection });
});

const health = (state: string, status_code: string, status_message: string) => ({
  ok: true as const,
  value: {
    ...readyProjection,
    payload: { ...readyProjection.payload, state, status_code, status_message },
  } as typeof readyProjection,
});

// The settings page's status names the state in Chinese and says why when hovered.
test('a Core that cannot start says why on the settings page', async () => {
  const why =
    '找不到 aura-core.exe（可能被安全软件隔离或安装不完整），Aura 现在不能使用；请到安全软件的隔离区恢复它，或重新运行安装程序修复，然后重启 Aura。';
  vi.mocked(window.aura.v1.getCoreHealth).mockResolvedValue(
    health('unavailable', 'core.executable_missing', why),
  );
  const { container } = render(<App />);
  fireEvent.click(await screen.findByRole('button', { name: '设置' }));
  await waitFor(() =>
    expect(container.querySelector('.settings-core-status')?.getAttribute('title')).toBe(why),
  );
  expect(container.querySelector('.settings-core-status')?.textContent).toBe('无法启动 Aura Core');
});

test('a crash handed to the launcher reads in Chinese', async () => {
  vi.mocked(window.aura.v1.getCoreHealth).mockResolvedValue(
    health('app_crash_escalated', 'core.app_crash_escalated', 'Main escalated'),
  );
  const { container } = render(<App />);
  fireEvent.click(await screen.findByRole('button', { name: '设置' }));
  await waitFor(() =>
    expect(container.querySelector('.settings-core-status')?.textContent).toBe(
      '启动器正在接管恢复',
    ),
  );
  expect(container.querySelector('.settings-core-status')?.getAttribute('title')).not.toContain(
    'Main',
  );
});

// A state Core did not give in time is not a Core that has not started.
test('a Core state that could not be read says so, not that Core has not started', async () => {
  vi.mocked(window.aura.v1.getCoreHealth).mockResolvedValue({
    ok: false,
    error: {
      category: 'cancelled',
      code: 'ipc.request_timeout',
      correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      details: {},
      message: 'Core request timed out: projection.snapshot',
      retryable: false,
      terminal: 'cancelled',
    },
  });
  const { container } = render(<App />);
  await waitFor(() =>
    expect(container.textContent).toContain(
      '这次没能及时从 Aura Core 读到状态（Core 可能正忙），暂时不能确认 Core 是否就绪；Aura 会自动再读，读到后这条提示自动消失。',
    ),
  );
  expect(container.textContent).not.toContain('Core 尚未启动');
  fireEvent.click(screen.getByRole('button', { name: '设置' }));
  expect(container.querySelector('.settings-core-status')?.textContent).toBe(
    'Aura Core 状态暂时读不到',
  );
});
