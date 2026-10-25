import { cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { MessageText } from './MessageText';
import { TurnProcess } from './TurnProcess';
import {
  clearMessageTextCache,
  pageCacheKey,
  readPageCache,
  writePageCache,
} from './message-text-cache';

beforeEach(() => {
  clearMessageTextCache();
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

test('sealing same preview must replace a changed suffix with authoritative text', async () => {
  const source = { kind: 'assistant' as const, model_step_id: 'step-seal' };
  let sealed = false;
  const read = vi.fn(async (request: { offset: number }) => ({
    ok: true,
    value: {
      source,
      body_id: sealed ? null : 'live-body',
      from_offset: request.offset,
      total_bytes: 9,
      next_offset: null,
      text: (sealed ? 'HEAD-NEW!' : 'HEAD-OLD!').slice(request.offset),
    },
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText: read } },
  });
  const props = {
    source,
    sessionId: 'review',
    preview: 'HEAD-',
    totalBytes: 9,
    role: 'process' as const,
  };
  const host = render(<MessageText {...props} partial bodyId="live-body" />);
  await waitFor(() => expect(host.container.querySelector('p')?.textContent).toBe('HEAD-OLD!'));
  sealed = true;
  host.rerender(<MessageText {...props} partial={false} bodyId={null} />);
  await waitFor(() => expect(host.container.querySelector('p')?.textContent).toBe('HEAD-NEW!'));
});

test('a remount must not prefer cached text over a changed authoritative preview', () => {
  const source = { kind: 'user' as const, event_id: 'event-cache' };
  const key = pageCacheKey('review:user:event-cache', null, 9);
  writePageCache(key, 'OLD-BODY!');
  const read = vi.fn(async () => {
    throw new Error('full preview requires no paging');
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText: read } },
  });
  const host = render(
    <MessageText
      source={source}
      sessionId="review"
      preview="NEW-BODY!"
      totalBytes={9}
      role="process"
    />,
  );
  expect(host.container.textContent).toBe('NEW-BODY!');
});

test('an over-budget body is not retained by the global cache', () => {
  writePageCache('oversized', 'x'.repeat(5 * 1024 * 1024));
  expect(readPageCache('oversized')).toBeUndefined();
});

test('the cache evicts the oldest page once the total UTF-8 budget is full', () => {
  const page = 'x'.repeat(1024 * 1024);
  writePageCache('session-a', page);
  writePageCache('session-b', page);
  writePageCache('session-c', page);
  writePageCache('session-d', page);
  writePageCache('session-e', page);
  expect(readPageCache('session-a')).toBeUndefined();
  expect(readPageCache('session-e')).toBe(page);
  expect(readPageCache('session-b')).toBe(page);
});

test('an interrupted turn still auto-loads every persisted byte of its unsealed process body', async () => {
  const source = { kind: 'assistant' as const, model_step_id: 'interrupted-step' };
  const full = 'p'.repeat(4096);
  const read = vi.fn(async (request: { offset: number; source: typeof source }) => ({
    ok: true,
    value: {
      source: request.source,
      body_id: 'stopped-attempt',
      from_offset: request.offset,
      total_bytes: full.length,
      next_offset: null,
      text: full.slice(request.offset),
    },
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText: read } },
  });
  const item = {
    turn: {
      session_id: 'interrupted-session',
      turn_task_id: 'interrupted-turn',
      root_user_event_id: 'root',
      state: { state: 'recovery_required' },
      created_at: '2026-09-26T09:00:00Z',
      updated_at: '2026-09-26T09:01:00Z',
    },
    user_messages: [],
    assistant_messages: [
      {
        model_step_id: source.model_step_id,
        partial: true,
        stream_body_id: 'stopped-attempt',
        text: full.slice(0, 2048),
        text_total_bytes: full.length,
        reasoning: full.slice(0, 2048),
        reasoning_total_bytes: full.length,
        at: '2026-09-26T09:00:00Z',
        tool_call_ids: [],
      },
    ],
  };
  const host = render(
    <TurnProcess item={item as never} calls={new Map()} awaitingContext={false} />,
  );
  await waitFor(() => expect(read).toHaveBeenCalled());
  await waitFor(() =>
    expect(host.container.querySelector('.agent-interim-reply')?.textContent?.length).toBe(4096),
  );
  await waitFor(() =>
    expect(host.container.querySelector('.agent-reasoning')?.textContent?.length).toBe(4096),
  );
  expect(host.container.textContent).not.toContain('正文仍在生成');
});
