import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { useToolCallDirectory } from './useToolCallDirectory';

// The calls the window has read are kept by the window.

const read = vi.fn();
const call = (id: string, name: string) => ({
  summary: { tool_call_id: id, outcome: { kind: 'succeeded' }, display_name: name },
});
beforeEach(() => {
  read.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentToolActivity: read } },
  });
});
afterEach(cleanup);

test('coming back, every finished call already read is named at once', async () => {
  read.mockResolvedValue({
    ok: true,
    value: { session_id: 'ses', calls: [call('a', '列出目录'), call('b', '读取文件')] },
  });
  const first = renderHook(() => useToolCallDirectory('ses', 'run', ['a', 'b'], 'token'));
  await act(async () => undefined);
  expect(first.result.current.get('a')?.summary.display_name).toBe('列出目录');
  first.unmount();

  read.mockReturnValue(new Promise(() => undefined));
  const again = renderHook(() => useToolCallDirectory('ses', 'run', ['a', 'b'], 'token'));
  expect(again.result.current.get('b')?.summary.display_name).toBe('读取文件');
  // Nothing finished is asked for again.
  expect(read).toHaveBeenCalledTimes(1);
});

test('a Core that started again is asked afresh', async () => {
  read.mockResolvedValue({
    ok: true,
    value: { session_id: 'ses2', calls: [call('a', '读取文件')] },
  });
  const first = renderHook(() => useToolCallDirectory('ses2', 'run-1', ['a'], 'token'));
  await act(async () => undefined);
  first.unmount();
  read.mockReturnValue(new Promise(() => undefined));
  const restarted = renderHook(() => useToolCallDirectory('ses2', 'run-2', ['a'], 'token'));
  expect(restarted.result.current.size).toBe(0);
});

// A call Core has no
// record of is asked for again while the turn runs, and no more once it has ended.
test('a call that never ran stops being asked for once its turn has ended', async () => {
  vi.useFakeTimers();
  try {
    read.mockResolvedValue({
      ok: true,
      value: { session_id: 'ses3', calls: [call('a', '读取文件')] },
    });
    const running = renderHook(
      ({ settled }) => useToolCallDirectory('ses3', 'run', ['a', 'never'], 'token', settled),
      { initialProps: { settled: false } },
    );
    await act(async () => undefined);
    expect(running.result.current.get('a')?.summary.display_name).toBe('读取文件');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1600);
    });
    expect(read).toHaveBeenCalledTimes(2);
    running.rerender({ settled: true });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    // Asked once more on the change, then never again.
    expect(read.mock.calls.length).toBeLessThanOrEqual(3);
    const asked = read.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(read).toHaveBeenCalledTimes(asked);
  } finally {
    vi.useRealTimers();
  }
});
