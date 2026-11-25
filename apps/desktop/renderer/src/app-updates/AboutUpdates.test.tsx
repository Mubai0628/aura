import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { UNAVAILABLE_UPDATE_STATE } from '../../../shared/app-updates';
import { AboutUpdates } from './AboutUpdates';
import { useAppUpdates } from './useAppUpdates';

vi.mock('./useAppUpdates', () => ({ useAppUpdates: vi.fn() }));
afterEach(cleanup);

test('installed update with cache left still offers restart and retries cleanup only', () => {
  const send = vi.fn().mockResolvedValue(true);
  vi.mocked(useAppUpdates).mockReturnValue({
    state: {
      ...UNAVAILABLE_UPDATE_STATE,
      available: true,
      current: '0.1.0',
      waiting: '0.1.1',
      phase: 'ready',
      cleanup_pending: true,
    },
    send,
    error: undefined,
    pending: undefined,
  });
  render(<AboutUpdates />);
  expect(screen.getByRole('status').textContent).toContain('可以照常重启');
  expect(screen.getByText('等待重启')).toBeTruthy();
  expect(screen.queryByText('更新失败')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '重试清理更新缓存' }));
  expect(send).toHaveBeenCalledExactlyOnceWith({ command: 'retry_cleanup' });
});
