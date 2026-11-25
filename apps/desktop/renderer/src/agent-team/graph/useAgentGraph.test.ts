import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { graphView } from '../graph-fixtures.test-support';
import { useAgentGraph } from './useAgentGraph';

const read = vi.fn(),
  mutate = vi.fn();
const good = (value: ReturnType<typeof graphView>) => ({ ok: true as const, value });
function delayed<T>() {
  let settle: (value: T) => void = () => {};
  const promise = new Promise<T>((resolve) => {
    settle = resolve;
  });
  return { promise, resolve: (value: T) => settle(value) };
}
beforeEach(() => {
  read.mockReset();
  mutate.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentGraph: read, mutateAgentGraph: mutate } },
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test('a late read from the previous conversation never becomes the current graph', async () => {
  const old = delayed<ReturnType<typeof good>>();
  read.mockImplementation(({ session_id }: { session_id: string }) =>
    session_id === 'ses_a'
      ? old.promise
      : Promise.resolve(good(graphView([['ses_b', '开发']], 'acg_b'))),
  );
  const shown = renderHook(({ session }) => useAgentGraph(session, true), {
    initialProps: { session: 'ses_a' },
  });
  shown.rerender({ session: 'ses_b' });
  await act(async () => {});
  expect(shown.result.current.view?.graph.sessions).toEqual(['ses_b']);
  await act(async () => old.resolve(good(graphView([['ses_a', '规划']]))));
  expect(shown.result.current.view?.graph.sessions).toEqual(['ses_b']);
});

test('an older poll cannot roll back a confirmed graph edit', async () => {
  vi.useFakeTimers();
  const old = delayed<ReturnType<typeof good>>();
  read
    .mockResolvedValueOnce(
      good(
        graphView([
          ['ses_a', '规划'],
          ['ses_b', '开发'],
        ]),
      ),
    )
    .mockReturnValueOnce(old.promise);
  mutate.mockResolvedValue(
    good(
      graphView(
        [
          ['ses_a', '规划'],
          ['ses_b', '开发'],
        ],
        'acg_one',
        8,
      ),
    ),
  );
  const shown = renderHook(() => useAgentGraph('ses_a', true));
  await act(async () => {});
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(read).toHaveBeenCalledTimes(2);
  await act(async () => {
    expect(
      await shown.result.current.change({
        action: 'connect',
        source_session_id: 'ses_a',
        target_session_id: 'ses_b',
      }),
    ).toBe(true);
  });
  await act(async () =>
    old.resolve(
      good(
        graphView([
          ['ses_a', '规划'],
          ['ses_b', '开发'],
        ]),
      ),
    ),
  );
  expect(shown.result.current.view?.graph.revision).toBe(8);
});

test('an unconfirmed mutation blocks another write and automatic polls until an explicit read', async () => {
  vi.useFakeTimers();
  read.mockResolvedValue(
    good(
      graphView([
        ['ses_a', '规划'],
        ['ses_b', '开发'],
      ]),
    ),
  );
  mutate.mockRejectedValue(new Error('transport response lost'));
  const shown = renderHook(() => useAgentGraph('ses_a', true));
  await act(async () => {});
  const action = {
    action: 'connect' as const,
    source_session_id: 'ses_a',
    target_session_id: 'ses_b',
  };
  await act(async () => {
    expect(await shown.result.current.change(action)).toBe(false);
  });
  expect(shown.result.current.uncertain).toBe(true);
  await act(async () => {
    expect(await shown.result.current.change(action)).toBe(false);
    await vi.advanceTimersByTimeAsync(9000);
  });
  expect(mutate).toHaveBeenCalledTimes(1);
  expect(read).toHaveBeenCalledTimes(1);
  await act(async () => shown.result.current.refresh());
  expect(read).toHaveBeenCalledTimes(2);
  expect(shown.result.current.uncertain).toBe(false);
  expect(mutate).toHaveBeenCalledTimes(1);
});

test('a late write result cannot enter a new conversation or grant it the previous graph revision', async () => {
  const pending = delayed<ReturnType<typeof good>>();
  read.mockImplementation(({ session_id }: { session_id: string }) =>
    Promise.resolve(
      good(graphView([[session_id, session_id === 'ses_a' ? '规划' : '开发']], session_id)),
    ),
  );
  mutate.mockReturnValue(pending.promise);
  const shown = renderHook(({ session }) => useAgentGraph(session, true), {
    initialProps: { session: 'ses_a' },
  });
  await act(async () => {});
  let written = Promise.resolve(false);
  act(() => {
    written = shown.result.current.change({ action: 'reset_layout' });
  });
  shown.rerender({ session: 'ses_b' });
  await act(async () => {});
  await act(async () => {
    pending.resolve(good(graphView([['ses_a', '规划']], 'acg_a', 8)));
    expect(await written).toBe(false);
  });
  expect(shown.result.current.view?.graph.sessions).toEqual(['ses_b']);
  expect(shown.result.current.busy).toBe(false);
});

test('explicit refresh keeps an uncertain write locked until the read actually returns', async () => {
  const pending = delayed<ReturnType<typeof good>>();
  read
    .mockResolvedValueOnce(good(graphView([['ses_a', '规划']])))
    .mockReturnValueOnce(pending.promise);
  mutate.mockRejectedValue(new Error('response lost'));
  const shown = renderHook(() => useAgentGraph('ses_a', true));
  await act(async () => {});
  await act(async () => {
    await shown.result.current.change({ action: 'reset_layout' });
  });
  await act(async () => shown.result.current.refresh());
  expect(shown.result.current.uncertain).toBe(true);
  await act(async () => {
    expect(await shown.result.current.change({ action: 'reset_layout' })).toBe(false);
  });
  expect(mutate).toHaveBeenCalledTimes(1);
  await act(async () => pending.resolve(good(graphView([['ses_a', '规划']], 'acg_one', 8))));
  expect(shown.result.current.uncertain).toBe(false);
  expect(shown.result.current.view?.graph.revision).toBe(8);
});

test('a failed read for the next session never retains the previous sessions graph', async () => {
  read
    .mockResolvedValueOnce(good(graphView([['ses_a', '规划']])))
    .mockRejectedValueOnce(new Error('not readable'));
  const shown = renderHook(({ session }) => useAgentGraph(session, true), {
    initialProps: { session: 'ses_a' },
  });
  await act(async () => {});
  shown.rerender({ session: 'ses_b' });
  await act(async () => {});
  expect(shown.result.current.view).toBeUndefined();
  expect(shown.result.current.uncertain).toBe(true);
  expect(shown.result.current.busy).toBe(false);
});

test('visibility recovery with a failed read clears busy without settling an old write as successful', async () => {
  const pending = delayed<ReturnType<typeof good>>();
  read
    .mockResolvedValueOnce(good(graphView([['ses_a', '规划']])))
    .mockRejectedValueOnce(new Error('read failed'));
  mutate.mockReturnValue(pending.promise);
  const shown = renderHook(({ visible }) => useAgentGraph('ses_a', visible), {
    initialProps: { visible: true },
  });
  await act(async () => {});
  let written = Promise.resolve(false);
  act(() => {
    written = shown.result.current.change({ action: 'reset_layout' });
  });
  shown.rerender({ visible: false });
  shown.rerender({ visible: true });
  await act(async () => {});
  expect(shown.result.current.busy).toBe(false);
  expect(shown.result.current.uncertain).toBe(true);
  await act(async () => {
    pending.resolve(good(graphView([['ses_a', '规划']], 'acg_one', 8)));
    expect(await written).toBe(false);
  });
  expect(shown.result.current.uncertain).toBe(true);
});
