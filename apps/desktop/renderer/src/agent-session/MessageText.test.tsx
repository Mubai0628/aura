import type { AgentMessageTextSource } from '@aura/contracts/generated';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { MessageText } from './MessageText';
import { clearMessageTextCache } from './message-text-cache';
import { messageTextQueue } from './message-text-queue';

/** Roughly `size` bytes of one repeated ASCII character, so offsets are exact. */
const body = (size: number, character = 'a'): string => character.repeat(size);

const source: AgentMessageTextSource = { event_id: 'evt_1', kind: 'user' };

interface Fetch {
  /** Which body asked, taken from the request itself. */
  event: string;
  offset: number;
}

/** A fake paging bridge; each page returns `page` bytes of `character`. `stop`
 *  makes it stop answering, so a test can inspect a strictly ordered queue. */
function bridge(
  total: number,
  options: { failFirst?: boolean; page?: number; stopAfter?: number } = {},
) {
  const pageSize = options.page ?? 1024;
  const fetches: Fetch[] = [];
  const state = { active: 0, peak: 0, stopped: false };
  let calls = 0;
  let failed = false;
  const readAgentMessageText = vi.fn(
    async (request: { offset: number; source: AgentMessageTextSource }) => {
      calls += 1;
      if (options.failFirst && !failed) {
        failed = true;
        return { error: { message: '读取失败' }, ok: false };
      }
      const event = request.source.kind === 'user' ? request.source.event_id : 'step';
      fetches.push({ event, offset: request.offset });
      state.active += 1;
      state.peak = Math.max(state.peak, state.active);
      const length = Math.min(pageSize, Math.max(0, total - request.offset));
      // A couple of microtasks, so several bodies really do overlap.
      await Promise.resolve();
      await Promise.resolve();
      state.active -= 1;
      if (options.stopAfter !== undefined && calls > options.stopAfter) {
        // Stop answering, but still settle: an unsettled page would keep its
        // concurrency slot and leak into the next test.
        state.stopped = true;
        return { error: { message: '读取失败' }, ok: false };
      }
      const text = body(length, String.fromCharCode(97 + (calls % 26)));
      return {
        ok: true,
        value: {
          next_offset: request.offset + length < total ? request.offset + length : null,
          // The page echoes the source it was asked for, as the real bridge does.
          source: request.source,
          text,
          total_bytes: total,
          // Sealed text never moves, so the page starts exactly where it was asked.
          body_id: null,
          from_offset: request.offset,
        },
      };
    },
  );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText } },
  });
  return { fetches, readAgentMessageText, state };
}

/** A controllable IntersectionObserver: nothing is visible until told. */
const observers: { element: Element; callback: (entries: unknown[]) => void }[] = [];

beforeEach(() => {
  clearMessageTextCache();
  observers.length = 0;
  // React logs the rejected page the loader handles; the test asserts on the UI.
  vi.spyOn(console, 'error').mockImplementation(() => undefined);
  class FakeObserver {
    constructor(private readonly callback: (entries: unknown[]) => void) {
      this.callback = callback;
    }
    private readonly element: Element = document.body;
    observe(element: Element): void {
      observers.push({ callback: this.callback, element });
    }
    disconnect(): void {
      /* nothing to release in the fake */
    }
  }
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: FakeObserver,
  });
});

afterEach(() => {
  cleanup();
  messageTextQueue.setConcurrency(2);
  for (const id of ['evt_1', 'evt_a', 'evt_b', 'evt_c']) {
    messageTextQueue.remove(`session:user:${id}`);
  }
  observers.length = 0;
  vi.restoreAllMocks();
});

test('a body longer than one page fills in completely without any click', async () => {
  const total = 4096;
  const { fetches, readAgentMessageText } = bridge(total);
  render(
    <MessageText
      sessionId="session"
      source={source}
      preview={body(1024, 'a')}
      totalBytes={total}
      role="user"
    />,
  );
  await waitFor(() =>
    expect(document.querySelector('.markdown-body')?.textContent?.length).toBe(total),
  );
  // Three pages after the preview, and no manual entry point anywhere.
  expect(fetches.map((fetch) => fetch.offset)).toEqual([1024, 2048, 3072]);
  expect(readAgentMessageText).toHaveBeenCalledTimes(3);
  expect(screen.queryByRole('button', { name: '继续读取正文' })).toBeNull();
  expect(screen.queryByRole('button')).toBeNull();
});

/** A bridge that answers one short page and then reports the source exhausted,
 *  which is what a body that shrank — or a projection that announced more than it
 *  can deliver — looks like from the renderer's side. */
function shortBridge(announced: number, delivered: number) {
  const readAgentMessageText = vi.fn(
    async (request: { offset: number; source: AgentMessageTextSource }) => ({
      ok: true,
      value: {
        next_offset: null,
        source: request.source,
        text: delivered > request.offset ? body(delivered - request.offset) : '',
        total_bytes: announced,
        body_id: null,
        from_offset: request.offset,
      },
    }),
  );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText } },
  });
  return { readAgentMessageText };
}

test('a short body with an exhausted source is surfaced, never padded to complete', async () => {
  const total = 20;
  const { readAgentMessageText } = shortBridge(total, 3);
  render(
    <MessageText
      sessionId="session"
      source={source}
      preview={body(3, 'a')}
      totalBytes={total}
      role="user"
    />,
  );
  // The three bytes that really exist are shown, and the mismatch is reported
  // instead of being turned into a completed twenty-byte body.
  const alert = await screen.findByRole('alert');
  expect(alert.textContent).toContain('正文少于声明的长度');
  expect(document.querySelector('.markdown-body')?.textContent ?? '').not.toBe(body(total));
  expect(readAgentMessageText).toHaveBeenCalled();
  // No fabricated padding anywhere in the rendered text.
  const rendered = document.querySelector('.markdown-body')?.textContent ?? '';
  expect(rendered.length).toBeLessThan(total);
});

test('a replacement preview of the same length updates the displayed body', async () => {
  const total = 4096;
  bridge(total);
  const { rerender } = render(
    <MessageText
      sessionId="session"
      source={source}
      preview={body(8, 'a')}
      totalBytes={total}
      role="user"
    />,
  );
  // A new partial body can arrive with exactly the same announced size; the
  // displayed text must follow the projection rather than keep the old bytes.
  rerender(
    <MessageText
      sessionId="session"
      source={source}
      preview={body(8, 'b')}
      totalBytes={total}
      role="user"
    />,
  );
  await waitFor(() =>
    expect(document.querySelector('.markdown-body')?.textContent?.startsWith('bbbb')).toBe(true),
  );
});

test('a complete preview fetches nothing at all', async () => {
  const { readAgentMessageText } = bridge(8);
  render(
    <MessageText
      sessionId="session"
      source={source}
      preview={body(8)}
      totalBytes={8}
      role="user"
    />,
  );
  await act(async () => undefined);
  expect(readAgentMessageText).not.toHaveBeenCalled();
  expect(document.querySelector('.markdown-body')?.textContent).toBe(body(8));
});

/** A fake bridge for a step that is still running: its pages come from one named
 *  body, report the size the live body currently has, and start exactly where
 *  they were asked to. */
function liveBridge(
  total: number,
  bodyId: string,
  options: { page?: number; character?: string } = {},
) {
  const pageSize = options.page ?? 1024;
  const character = options.character ?? 'z';
  const fetches: number[] = [];
  const readAgentMessageText = vi.fn(
    async (request: { offset: number; source: AgentMessageTextSource }) => {
      fetches.push(request.offset);
      const length = Math.min(pageSize, Math.max(0, total - request.offset));
      return {
        ok: true,
        value: {
          body_id: bodyId,
          from_offset: request.offset,
          next_offset: request.offset + length < total ? request.offset + length : null,
          source: request.source,
          text: body(length, character),
          total_bytes: total,
        },
      };
    },
  );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText } },
  });
  return { fetches, readAgentMessageText };
}

test('a running step is read page by page from its live body', async () => {
  const { fetches, readAgentMessageText } = liveBridge(4096, 'attempt-1');
  const view = render(
    <MessageText
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step_1' }}
      preview={body(16, 'z')}
      totalBytes={4096}
      partial
      bodyId="attempt-1"
      role="reasoning"
    />,
  );
  // The body is readable while the step runs: every page continues the one
  // before it until the size the projection reports is on screen.
  await waitFor(() =>
    expect(view.container.querySelector('.agent-message-text')?.textContent?.length).toBe(4096),
  );
  expect(fetches).toEqual([16, 1040, 2064, 3088]);
  expect(readAgentMessageText).toHaveBeenCalledTimes(4);
  // No fixed “still generating” line; the turn says what is happening.
  expect(screen.queryByText('正文仍在生成，会持续补齐。')).toBeNull();
});

test('a page from a restarted body is discarded and the body is read again', async () => {
  // The first page belongs to the body on screen; the second arrives after a
  // retry began, so its bytes are a different body's and must not be appended.
  const pages = [
    { body_id: 'attempt-1', from_offset: 8, text: body(8, 'b'), total_bytes: 1024 },
    { body_id: 'attempt-2', from_offset: 16, text: body(8, 'c'), total_bytes: 64 },
    { body_id: 'attempt-2', from_offset: 0, text: body(32, 'd'), total_bytes: 64 },
    { body_id: 'attempt-2', from_offset: 32, text: body(32, 'd'), total_bytes: 64 },
    { body_id: 'attempt-2', from_offset: 64, text: '', total_bytes: 64 },
  ];
  const asked: number[] = [];
  const readAgentMessageText = vi.fn(async (request: { offset: number }) => {
    asked.push(request.offset);
    const page = pages.shift();
    return {
      ok: true,
      value: {
        next_offset: null,
        source: { kind: 'assistant', model_step_id: 'step_1' },
        ...(page ?? { body_id: 'attempt-2', from_offset: 64, text: '', total_bytes: 64 }),
      },
    };
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText } },
  });
  const view = render(
    <MessageText
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step_1' }}
      preview={body(8, 'a')}
      totalBytes={1024}
      partial
      bodyId="attempt-1"
      // A plain-text host, so the exact bytes on screen are compared (process notes are Markdown).
      role="brief"
    />,
  );
  // The restarted body is read from its first byte again: the answer is the new
  // body only, never the old bytes plus the new ones.
  await waitFor(() =>
    expect(view.container.querySelector('.agent-message-text')?.textContent).toBe(body(64, 'd')),
  );
  expect(asked.slice(0, 3)).toEqual([8, 16, 0]);
});

test('a page that does not continue the body on screen is not spliced in', async () => {
  const asked: number[] = [];
  const readAgentMessageText = vi.fn(async (request: { offset: number }) => {
    asked.push(request.offset);
    // The first answer starts behind the byte the reader holds, so it cannot be
    // the continuation it was asked for; the second is the body's first page.
    const gap = asked.length === 1;
    return {
      ok: true,
      value: {
        body_id: 'attempt-1',
        from_offset: gap ? 900 : 0,
        next_offset: gap ? null : null,
        source: { kind: 'assistant', model_step_id: 'step_1' },
        text: gap ? body(100, 'b') : body(2048, 'c'),
        total_bytes: 2048,
      },
    };
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText } },
  });
  const view = render(
    <MessageText
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step_1' }}
      preview={body(1000, 'a')}
      totalBytes={2048}
      partial
      bodyId="attempt-1"
      // A plain-text host, so the exact bytes on screen are compared (process notes are Markdown).
      role="brief"
    />,
  );
  await waitFor(() =>
    expect(view.container.querySelector('.agent-message-text')?.textContent).toBe(body(2048, 'c')),
  );
  // The reader asked from its own byte, was answered from a different one, and
  // started again from the first byte instead of showing unrelated text.
  expect(asked).toEqual([1000, 0]);
});

test('a sealed body is never spliced onto live bytes', async () => {
  // The projection still calls the step running, but the Core has already sealed
  // it: the sealed text may differ, so the live bytes are not extended with it.
  const readAgentMessageText = vi.fn(async () => ({
    ok: true,
    value: {
      body_id: null,
      from_offset: 8,
      next_offset: 64,
      source: { kind: 'assistant', model_step_id: 'step_1' },
      text: body(56, 'x'),
      total_bytes: 64,
    },
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText } },
  });
  const view = render(
    <MessageText
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step_1' }}
      preview={body(8, 'z')}
      totalBytes={64}
      partial
      bodyId="attempt-1"
      // A plain-text host, so the exact bytes on screen are compared (process notes are Markdown).
      role="brief"
    />,
  );
  await act(async () => undefined);
  expect(readAgentMessageText).toHaveBeenCalledTimes(1);
  expect(view.container.querySelector('.agent-message-text')?.textContent).toBe(body(8, 'z'));
});

test('two long bodies on screen both fill in without a click', async () => {
  // One slot at a time, so the arrival order of the two bodies is observable.
  messageTextQueue.setConcurrency(1);
  const { fetches } = bridge(2048, { page: 512 });
  const first = render(
    <MessageText
      sessionId="session"
      source={{ event_id: 'evt_a', kind: 'user' }}
      preview={body(512)}
      totalBytes={2048}
      role="user"
    />,
  );
  const second = render(
    <MessageText
      sessionId="session"
      source={{ event_id: 'evt_b', kind: 'user' }}
      preview={body(512)}
      totalBytes={2048}
      role="user"
    />,
  );
  await waitFor(() => expect(fetches.length).toBeGreaterThanOrEqual(2));
  // Both are off screen, so both fill in through the background queue in
  // arrival order; neither is starved.
  expect(fetches.slice(0, 2).map((fetch) => `${fetch.event}@${fetch.offset}`)).toEqual([
    'evt_a@512',
    'evt_b@512',
  ]);
  // The strict promotion order is covered by the queue tests; here the point is
  // that both bodies reach their full text with no click and no manual step.
  await waitFor(() => {
    expect(first.container.querySelector('.markdown-body')?.textContent?.length).toBe(2048);
    expect(second.container.querySelector('.markdown-body')?.textContent?.length).toBe(2048);
  });
  expect(screen.queryByRole('button')).toBeNull();
});

test('concurrency stays bounded while every body wants a page', async () => {
  // The bridge answers exactly two pages and then fails them, so only the first
  // two bodies can finish while the third still has to take its turn.
  const { fetches, state } = bridge(2048, { page: 512, stopAfter: 2 });
  for (const id of ['evt_a', 'evt_b', 'evt_c']) {
    render(
      <MessageText
        sessionId="session"
        source={{ event_id: id, kind: 'user' }}
        preview={body(512)}
        totalBytes={2048}
        role="user"
      />,
    );
  }
  await waitFor(() => expect(fetches.map((fetch) => fetch.event)).toContain('evt_c'));
  // Two requests at a time, in arrival order, and no body starved.
  expect(state.peak).toBeLessThanOrEqual(2);
  expect(messageTextQueue.activeCount).toBeLessThanOrEqual(2);
  expect(fetches.slice(0, 2).map((fetch) => fetch.event)).toEqual(['evt_a', 'evt_b']);
  // Exactly one page per body was in flight at any moment.
  const events = fetches.map((fetch) => fetch.event);
  expect(new Set(events).size).toBe(3);
  expect(events.filter((event) => event === 'evt_c')).toHaveLength(1);
});

test('a failing fetch exposes a retry path', async () => {
  const total = 2048;
  const { readAgentMessageText } = bridge(total, { failFirst: true, page: 1024 });
  render(
    <MessageText
      sessionId="session"
      source={source}
      preview={body(1024, 'a')}
      totalBytes={total}
      role="user"
    />,
  );
  const retry = await screen.findByRole('button', { name: '重新读取正文' });
  expect(screen.getByRole('alert')).toBeDefined();
  expect(readAgentMessageText).toHaveBeenCalledTimes(1);

  retry.click();

  await waitFor(() =>
    expect(document.querySelector('.markdown-body')?.textContent?.length).toBe(total),
  );
  // The retry resumed at the byte the preview stopped at, and succeeded once.
  expect(readAgentMessageText).toHaveBeenCalledTimes(2);
  expect(readAgentMessageText.mock.calls[1]?.[0]).toMatchObject({ offset: 1024 });
  await waitFor(() => expect(screen.queryByRole('button', { name: '重新读取正文' })).toBeNull());
});

test('both directions of the viewport are reported to the shared queue', async () => {
  bridge(4096);
  const setVisible = vi.spyOn(messageTextQueue, 'setVisible');
  render(
    <MessageText
      sessionId="session"
      source={source}
      preview={body(1024, 'a')}
      totalBytes={4096}
      role="user"
    />,
  );
  await waitFor(() => expect(observers.length).toBe(1));
  const observer = observers[0]!;
  const queueIdentity = setVisible.mock.calls.at(-1)?.[0];
  expect(queueIdentity).toBeDefined();
  // Leaving the viewport must be reported too: an off-screen body cannot keep
  // outranking the body the user is looking at.
  act(() => observer.callback([{ isIntersecting: false, target: observer.element }]));
  expect(setVisible).toHaveBeenLastCalledWith(queueIdentity, false);
  act(() => observer.callback([{ isIntersecting: true, target: observer.element }]));
  expect(setVisible).toHaveBeenLastCalledWith(queueIdentity, true);
});

test('a page from a changed source is rejected with the Chinese explanation', async () => {
  const readAgentMessageText = vi.fn().mockResolvedValue({
    ok: true,
    value: {
      next_offset: null,
      source: { event_id: 'evt_other', kind: 'user' },
      text: '别人的正文',
      total_bytes: 2048,
    },
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText } },
  });
  render(
    <MessageText
      sessionId="session"
      source={source}
      preview={body(8, 'a')}
      totalBytes={2048}
      role="user"
    />,
  );
  await waitFor(() => expect(screen.getByRole('alert')).toBeDefined());
  expect(screen.getByRole('alert').textContent).toBe(
    '正文来源已变化，请重新加载会话。重新读取正文',
  );
  expect(document.querySelector('.markdown-body')?.textContent).toBe(body(8, 'a'));
  expect(readAgentMessageText).toHaveBeenCalledTimes(1);
});

test('the model’s process notes are Markdown; its reasoning stays plain text', async () => {
  const notes = '## 计划\n\n先读 **入口** 文件。';
  const bytes = new TextEncoder().encode(notes).length;
  const step: AgentMessageTextSource = { kind: 'assistant', model_step_id: 'step_1' };
  bridge(bytes);
  const process = render(
    <MessageText
      sessionId="session"
      source={step}
      preview={notes}
      totalBytes={bytes}
      role="process"
    />,
  );
  await waitFor(() => expect(process.container.querySelector('h2')?.textContent).toBe('计划'));
  expect(process.container.textContent).toContain('先读 入口 文件。');
  expect(process.container.textContent).not.toContain('**');
  process.unmount();
  const reasoning = render(
    <MessageText
      sessionId="session"
      source={step}
      preview={notes}
      totalBytes={bytes}
      role="reasoning"
    />,
  );
  await act(async () => undefined);
  expect(reasoning.container.querySelector('h2')).toBeNull();
  expect(reasoning.container.textContent).toContain('## 计划');
});
