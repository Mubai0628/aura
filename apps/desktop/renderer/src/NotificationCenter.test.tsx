import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, test, vi } from 'vitest';

import { NotificationViewport } from './NotificationCenter';
import { useNotificationCenter, type NotificationTone } from './notification-center-store';

function NotificationHarness({ tone = 'danger' }: { tone?: NotificationTone }) {
  const { dismiss, items, publish, withdraw } = useNotificationCenter();

  return (
    <>
      <button
        onClick={() =>
          publish({
            dedupeKey: 'test-notice',
            message: '测试消息正文',
            title: '测试消息',
            tone,
          })
        }
        type="button"
      >
        发布消息
      </button>
      <button onClick={() => withdraw('test-')} type="button">
        状态恢复
      </button>
      <NotificationViewport items={items} onDismiss={dismiss} />
    </>
  );
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('NotificationCenter', () => {
  test('keeps a notice through 3999ms and auto-dismisses it at 4000ms', () => {
    vi.useFakeTimers();
    render(<NotificationHarness tone="success" />);

    fireEvent.click(screen.getByRole('button', { name: '发布消息' }));
    expect(screen.getByRole('status').textContent).toContain('测试消息正文');

    act(() => vi.advanceTimersByTime(3_999));
    expect(screen.getByRole('status')).toBeTruthy();

    act(() => vi.advanceTimersByTime(1));
    expect(screen.queryByRole('status')).toBeNull();
  });

  // A failure says what went wrong and what to do next, so it stays
  // until it is closed.
  test('keeps a failure until it is closed', () => {
    vi.useFakeTimers();
    render(<NotificationHarness />);

    fireEvent.click(screen.getByRole('button', { name: '发布消息' }));
    act(() => vi.advanceTimersByTime(60_000));
    expect(screen.getByRole('alert').textContent).toContain('测试消息正文');

    fireEvent.click(screen.getByRole('button', { name: '关闭消息' }));
    expect(screen.queryByRole('alert')).toBeNull();
  });

  test('deduplicates an active message and supports explicit close', () => {
    render(<NotificationHarness />);

    fireEvent.click(screen.getByRole('button', { name: '发布消息' }));
    fireEvent.click(screen.getByRole('button', { name: '发布消息' }));
    const [alert] = screen.getAllByRole('alert');
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(alert?.closest('.notification-viewport')?.parentElement).toBe(document.body);

    fireEvent.click(screen.getByRole('button', { name: '关闭消息' }));
    expect(screen.queryByRole('alert')).toBeNull();
  });

  // A failure that describes a state (model settings or permissions unavailable) is withdrawn
  // once the state is fine again; it would otherwise stay and contradict the screen.
  test('withdraws a state failure once the state recovers', () => {
    render(<NotificationHarness />);

    fireEvent.click(screen.getByRole('button', { name: '发布消息' }));
    expect(screen.getByRole('alert')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '状态恢复' }));
    expect(screen.queryByRole('alert')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: '发布消息' }));
    expect(screen.getByRole('alert')).toBeTruthy();
  });
});
