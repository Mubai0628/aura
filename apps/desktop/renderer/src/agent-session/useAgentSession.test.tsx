import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { prefetchAgentSession, useAgentSession } from './useAgentSession';

const read = vi.fn();
// Projection subsets exercise the consumer; Rust/bridge tests own DTO shape.
const page = (
  session: string,
  revision: string,
  ids: number[],
  runtime = 'runtime-a',
  before?: string,
) => ({
  ok: true,
  value: {
    session_id: session,
    revision,
    runtime_instance_id: runtime,
    active: null,
    inbox: { messages: [] },
    next_before_turn_task_id: null,
    turns: [
      {
        turn: { turn_task_id: 'turn' },
        user_messages: ids.map((n) => ({ event_id: `user-${n}`, text: `${n}` })),
        assistant_messages: [],
        next_messages: before
          ? { turn_task_id: 'turn', before_user_event_id: before, before_model_step_id: null }
          : null,
      },
    ],
  },
});
beforeEach(() => {
  vi.useFakeTimers();
  read.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentHistory: read } },
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test('polling preserves explicitly loaded older messages without duplicate or lost sources', async () => {
  read.mockResolvedValueOnce(page('history-pages', '1', [3, 4], 'runtime-a', 'user-3'));
  const { result } = renderHook(() => useAgentSession('history-pages'));
  await act(async () => undefined);
  read.mockResolvedValueOnce(page('history-pages', '1', [1, 2]));
  await act(() => result.current.loadTurnOlder('turn'));
  expect(result.current.projection?.turns[0]?.user_messages.map((v) => v.event_id)).toEqual([
    'user-1',
    'user-2',
    'user-3',
    'user-4',
  ]);
  read.mockResolvedValueOnce(page('history-pages', '2', [4, 5], 'runtime-a', 'user-4'));
  await act(() => vi.advanceTimersByTimeAsync(1000));
  expect(result.current.projection?.turns[0]?.user_messages.map((v) => v.event_id)).toEqual([
    'user-1',
    'user-2',
    'user-3',
    'user-4',
    'user-5',
  ]);
  expect(result.current.projection?.turns[0]?.next_messages).toBeNull();
});

test('a delayed response from the previous session cannot replace the selected session', async () => {
  let release: (value: unknown) => void = () => undefined;
  read.mockReturnValueOnce(
    new Promise((done) => {
      release = done;
    }),
  );
  const { result, rerender } = renderHook(({ session }) => useAgentSession(session), {
    initialProps: { session: 'late-a' },
  });
  read.mockResolvedValueOnce(page('late-b', '2', [2]));
  rerender({ session: 'late-b' });
  await act(async () => undefined);
  await act(async () => {
    release(page('late-a', '999', [1]));
  });
  expect(result.current.projection?.session_id).toBe('late-b');
  expect(result.current.projection?.turns[0]?.user_messages[0]?.event_id).toBe('user-2');
});

test('a fresh Core runtime replaces the old revision and partial history', async () => {
  read.mockResolvedValueOnce(page('runtime-change', '100', [1]));
  const { result } = renderHook(() => useAgentSession('runtime-change'));
  await act(async () => undefined);
  read.mockResolvedValueOnce(page('runtime-change', '2', [2], 'runtime-b'));
  await act(() => vi.advanceTimersByTimeAsync(1000));
  expect(result.current.projection?.runtime_instance_id).toBe('runtime-b');
  expect(result.current.projection?.turns[0]?.user_messages.map((v) => v.event_id)).toEqual([
    'user-2',
  ]);
});

/** The automatic pager decides whether to keep reading from the answer of one
 *  page, so the three outcomes have to be distinguishable. */
test('a page of earlier turns reports whether it really moved the history', async () => {
  const withCursor = (before: string | null, ids: number[]) => ({
    ...page('older-turns', '5', ids),
    value: { ...page('older-turns', '5', ids).value, next_before_turn_task_id: before },
  });
  read.mockResolvedValueOnce(withCursor('turn-1', [1]));
  const { result } = renderHook(() => useAgentSession('older-turns'));
  await act(async () => undefined);

  // A page that arrives and moves the cursor forward: keep reading.
  read.mockResolvedValueOnce({
    ok: true,
    value: {
      session_id: 'older-turns',
      revision: '6',
      runtime_instance_id: 'runtime-a',
      active: null,
      inbox: { messages: [] },
      next_before_turn_task_id: 'turn-0',
      turns: [
        { turn: { turn_task_id: 'turn-0' }, user_messages: [], assistant_messages: [] },
        ...withCursor(null, [1]).value.turns,
      ],
    },
  });
  await expect(result.current.loadOlder()).resolves.toBe(true);

  // The same cursor answered again with what is already on screen: stop.
  read.mockResolvedValueOnce({
    ok: true,
    value: {
      session_id: 'older-turns',
      revision: '6',
      runtime_instance_id: 'runtime-a',
      active: null,
      inbox: { messages: [] },
      next_before_turn_task_id: 'turn-0',
      turns: withCursor(null, [1]).value.turns,
    },
  });
  await expect(result.current.loadOlder()).resolves.toBe(false);

  // A failed read is reported as "nothing arrived" rather than as success.
  read.mockResolvedValueOnce({ ok: false, error: { message: '读取失败' } });
  await expect(result.current.loadOlder()).resolves.toBe(false);

  // A turn with no older cursor left is not a request at all.
  await expect(result.current.loadTurnOlder('missing-turn')).resolves.toBeUndefined();
});

// What a running step writes into a tool call is not stored, so it does not
// move the revision; a poll that brings new figures for it still shows them.
test('a running step’s tool input is taken from a poll of the same revision', async () => {
  const running = (bytes: number) => {
    const value = page('live-input', '7', [1]);
    value.value.active = { agent_run_id: 'run' } as never;
    value.value.turns[0]!.assistant_messages = [
      {
        model_step_id: 'step',
        partial: true,
        text: '',
        tool_input: [{ title: '写入文件', received_bytes: bytes, complete: false }],
      },
    ] as never;
    return value;
  };
  read.mockResolvedValueOnce(running(1024));
  const { result } = renderHook(() => useAgentSession('live-input'));
  await act(async () => undefined);
  const shown = () =>
    (result.current.projection?.turns[0]?.assistant_messages[0] as { tool_input?: unknown[] })
      ?.tool_input;
  expect(shown()).toEqual([{ title: '写入文件', received_bytes: 1024, complete: false }]);
  read.mockResolvedValueOnce(running(18_637));
  await act(() => vi.advanceTimersByTimeAsync(250));
  expect(shown()).toEqual([{ title: '写入文件', received_bytes: 18_637, complete: false }]);
});

// Coming back to a
// conversation the window has already read shows it at once; the read refreshes it quietly.
test('coming back to a conversation already read shows it at once, not "loading"', async () => {
  read.mockResolvedValue(page('come-back', '1', [1]));
  const first = renderHook(() => useAgentSession('come-back'));
  await act(async () => undefined);
  expect(first.result.current.loading).toBe(false);
  first.unmount();
  read.mockReturnValue(new Promise(() => undefined));
  const again = renderHook(() => useAgentSession('come-back'));
  expect(again.result.current.loading).toBe(false);
  expect(again.result.current.projection?.turns[0]?.user_messages[0]?.event_id).toBe('user-1');
});

// A
// conversation opens with its newest turns; earlier ones come in small pages as the top nears, and
// the polls that follow the running turn read only the newest.
test('a conversation opens with its newest turns; earlier ones and the polls come in small reads', async () => {
  read.mockResolvedValue({
    ...page('fast-open', '1', [9]),
    value: { ...page('fast-open', '1', [9]).value, next_before_turn_task_id: 'turn-earlier' },
  });
  const { result } = renderHook(() => useAgentSession('fast-open'));
  await act(async () => undefined);
  expect(read.mock.calls[0]?.[0]).toMatchObject({ session_id: 'fast-open', limit: 2 });
  await act(() => vi.advanceTimersByTimeAsync(1000));
  expect(read.mock.calls.at(-1)?.[0]).toMatchObject({ before_turn_task_id: null, limit: 2 });
  await act(() => result.current.loadOlder());
  expect(read.mock.calls.at(-1)?.[0]).toMatchObject({
    before_turn_task_id: 'turn-earlier',
    limit: 5,
  });
});

// Read ahead from the navigation, a conversation opens drawn.
test('a conversation read ahead opens at once, without "loading"', async () => {
  read.mockResolvedValue(page('read-ahead', '1', [1]));
  prefetchAgentSession('read-ahead');
  prefetchAgentSession('read-ahead');
  await act(async () => undefined);
  expect(read).toHaveBeenCalledTimes(1);
  expect(read.mock.calls[0]?.[0]).toMatchObject({ session_id: 'read-ahead', limit: 2 });
  const { result } = renderHook(() => useAgentSession('read-ahead'));
  expect(result.current.loading).toBe(false);
  expect(result.current.projection?.session_id).toBe('read-ahead');
});
