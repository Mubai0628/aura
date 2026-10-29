import type { AgentTurnView } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { endedAgo, endedAtText } from './ended-ago';
import { TurnSummary } from './TurnSummary';

afterEach(cleanup);

const MINUTE = 60_000;
const at = new Date(2026, 9, 28, 21, 35, 12).getTime();

test('how long ago keeps the two largest units and no seconds', () => {
  expect(endedAgo(at, at + 59_000)).toBe('刚刚');
  expect(endedAgo(at, at + 53 * MINUTE + 40_000)).toBe('53 分钟之前');
  expect(endedAgo(at, at + 65 * MINUTE)).toBe('1 小时 5 分钟之前');
  expect(endedAgo(at, at + 180 * MINUTE)).toBe('3 小时之前');
  expect(endedAgo(at, at + ((2 * 24 + 13) * 60 + 53) * MINUTE)).toBe('2 天 13 小时之前');
  expect(endedAgo(at, at + 3 * 24 * 60 * MINUTE + 7 * MINUTE)).toBe('3 天之前');
  // A clock that went back is not a turn from the future.
  expect(endedAgo(at, at - 5 * MINUTE)).toBe('刚刚');
});

test('the bubble names the day and the minute, 24-hour', () => {
  expect(endedAtText(at)).toBe('2026年10月28日 21:35');
  expect(endedAtText(new Date(2026, 0, 3, 7, 4).getTime())).toBe('2026年1月3日 07:04');
});

const finished = (endedAt: number) =>
  ({
    turn: {
      state: { state: 'completed', final_report: '好了' },
      created_at: new Date(endedAt - (15 * 60 + 41) * 1_000).toISOString(),
      updated_at: new Date(endedAt).toISOString(),
    },
    goal: null,
  }) as unknown as AgentTurnView;

test('a finished turn says how long ago it ended before its time, and when on rest', () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    vi.setSystemTime(at + 53 * MINUTE);
    render(<TurnSummary item={finished(at)} />);
    const ago = screen.getByText('53 分钟之前');
    expect(ago.nextElementSibling?.textContent).toBe('15 分 41 秒');
    expect(screen.queryByRole('tooltip')).toBeNull();
    fireEvent.mouseEnter(ago);
    expect(screen.getByRole('tooltip').textContent).toBe('2026年10月28日 21:35');
    fireEvent.mouseLeave(ago);
    expect(screen.queryByRole('tooltip')).toBeNull();
    // It is counted again every minute.
    act(() => {
      vi.advanceTimersByTime(8 * MINUTE);
    });
    expect(screen.getByText('1 小时 1 分钟之前')).toBeTruthy();
  } finally {
    vi.useRealTimers();
  }
});
