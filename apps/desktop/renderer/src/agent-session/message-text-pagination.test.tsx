import type { AgentMessageTextRequest, AgentMessageTextSource } from '@aura/contracts/generated';
import { act, cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { MessageText, type MessageTextRole } from './MessageText';
import { messageTextQueue } from './message-text-queue';

const preview = '鸟'.repeat(682);
const firstPage = '甲'.repeat(10_922);
const lastPage = '末页';
const text = preview + firstPage + lastPage;
const size = (value: string) => new TextEncoder().encode(value).length;
const userSource: AgentMessageTextSource = { kind: 'user', event_id: 'evt_paged' };

function bridge(totalText: string, bodyId: string | null, hold = Promise.resolve()) {
  const encoded = new TextEncoder().encode(totalText);
  const read = vi.fn(async (request: AgentMessageTextRequest) => {
    await hold;
    const end = Math.min(request.offset + size(firstPage), encoded.length);
    return {
      ok: true,
      value: {
        source: request.source,
        body_id: bodyId,
        from_offset: request.offset,
        text: new TextDecoder('utf-8', { fatal: true }).decode(encoded.slice(request.offset, end)),
        total_bytes: encoded.length,
        next_offset: end < encoded.length ? end : null,
        sealed_revision: bodyId === null ? 'revision-1' : null,
      },
    };
  });
  vi.stubGlobal('aura', { v1: { readAgentMessageText: read } });
  return read;
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const sources: [MessageTextRole, AgentMessageTextSource, string | null][] = [
  ['user', userSource, null],
  ['final', { kind: 'assistant', model_step_id: 'step_final' }, null],
  ['final', { kind: 'completion', turn_task_id: 'turn_final' }, null],
  ['process', { kind: 'assistant', model_step_id: 'step_live' }, 'attempt-1'],
  ['reasoning', { kind: 'reasoning', model_step_id: 'step_reasoning' }, 'attempt-1'],
];

test.each(sources)(
  '%s text loads each page once when its source rerenders (%j)',
  async (role, source, bodyId) => {
    const pending = Promise.withResolvers<void>();
    const read = bridge(text, bodyId, pending.promise);
    const message = () => (
      <MessageText
        bodyId={bodyId}
        partial={bodyId !== null}
        preview={preview}
        role={role}
        sealedRevision={bodyId === null ? 'revision-1' : null}
        sessionId="session-paged"
        source={{ ...source }}
        totalBytes={size(text)}
      />
    );
    const view = render(message());
    await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
    view.rerender(message());
    await act(async () => pending.resolve());

    expect(read.mock.calls.map(([request]) => request.offset)).toEqual([
      size(preview),
      size(preview + firstPage),
    ]);
    expect(view.container.textContent).toBe(text);
  },
);

test('development effect replay sends only one request for the first page', async () => {
  const read = bridge(preview + lastPage, null);
  const view = render(
    <MessageText
      preview={preview}
      role="user"
      sessionId="session-strict"
      source={userSource}
      totalBytes={size(preview + lastPage)}
    />,
    { reactStrictMode: true },
  );
  await waitFor(() => expect(view.container.textContent).toBe(preview + lastPage));
  expect(read.mock.calls.map(([request]) => request.offset)).toEqual([size(preview)]);
});

test.each([null, 'attempt-1'])(
  'an already applied response is ignored for body %s',
  async (bodyId) => {
    const fullText = preview + lastPage;
    bridge(fullText, bodyId);
    let responses: Promise<void[]> | undefined;
    vi.spyOn(messageTextQueue, 'read').mockImplementation((_id, offset, run) => {
      if (responses) return;
      const signal = new AbortController().signal;
      responses = Promise.all([run(signal, offset), run(signal, offset)]);
    });
    const view = render(
      <MessageText
        bodyId={bodyId}
        partial={bodyId !== null}
        preview={preview}
        role="brief"
        sessionId="session-duplicate-response"
        source={userSource}
        totalBytes={size(fullText)}
      />,
    );
    await act(async () => {
      await responses;
    });
    expect(view.container.textContent).toBe(fullText);
  },
);

test('a replacement live body can be read at byte zero even when the old body was empty', async () => {
  const read = bridge(lastPage, 'attempt-2');
  const view = render(
    <MessageText
      bodyId="attempt-1"
      partial
      preview=""
      role="reasoning"
      sessionId="session-restarted"
      source={{ kind: 'reasoning', model_step_id: 'step_restarted' }}
      totalBytes={size(lastPage)}
    />,
  );
  await waitFor(() => expect(view.container.textContent).toBe(lastPage));
  expect(read.mock.calls.map(([request]) => request.offset)).toEqual([0, 0]);
});
