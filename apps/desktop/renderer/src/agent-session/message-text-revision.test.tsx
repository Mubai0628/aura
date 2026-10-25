import { act, cleanup, render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { MessageText } from './MessageText';
import { clearMessageTextCache } from './message-text-cache';

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

const source = { kind: 'assistant' as const, model_step_id: 'sealed-cache-step' };
const prefix = 'stable paragraph '.repeat(128);
const body = prefix + 'old suffix '.repeat(500);
const props = {
  source,
  sessionId: 'cache-session',
  preview: body.slice(0, 2048),
  totalBytes: body.length,
  sealedRevision: 'revision-one',
  // A plain-text host, so the exact bytes on screen are compared (process notes are Markdown).
  role: 'brief' as const,
};
const page = (text: string, revision: string, offset: number) => ({
  ok: true,
  value: {
    source,
    text: text.slice(offset),
    sealed_revision: revision,
    body_id: null,
    from_offset: offset,
    total_bytes: text.length,
    next_offset: null,
  },
});
const bridge = (read: ReturnType<typeof vi.fn>) =>
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText: read } },
  });

test('a verified long sealed body survives remount immediately without rereading', async () => {
  const read = vi.fn(async (request: { offset: number }) =>
    page(body, props.sealedRevision, request.offset),
  );
  bridge(read);
  const first = render(<MessageText {...props} />);
  await waitFor(() => expect(first.container.textContent).toBe(body));
  first.unmount();
  read.mockClear();
  const again = render(<MessageText {...props} />);
  expect(again.container.textContent).toBe(body);
  expect(read).not.toHaveBeenCalled();
});

test('same preview and length with another sealed revision never reuse the old suffix', async () => {
  let text = body;
  let revision = props.sealedRevision;
  const read = vi.fn(async (request: { offset: number }) => page(text, revision, request.offset));
  bridge(read);
  const first = render(<MessageText {...props} />);
  await waitFor(() => expect(first.container.textContent).toBe(body));
  first.unmount();
  text = body.replaceAll('old suffix', 'new suffix');
  revision = 'revision-two';
  const again = render(<MessageText {...props} sealedRevision={revision} />);
  expect(again.container.textContent).not.toContain('old suffix');
  await waitFor(() => expect(again.container.textContent).toBe(text));
});

test('a mismatching page revision is rejected and does not enter the sealed cache', async () => {
  const read = vi.fn(async (request: { offset: number }) =>
    page(body, 'another-revision', request.offset),
  );
  bridge(read);
  const host = render(<MessageText {...props} />);
  await waitFor(() => expect(host.getByRole('alert')).toBeTruthy());
  expect(host.container.textContent).not.toContain('old suffix');
});

test('an old in-flight page cannot overwrite a newly selected sealed revision', async () => {
  let deliver: ((value: ReturnType<typeof page>) => void) | undefined;
  const nextBody = body.replaceAll('old suffix', 'new suffix');
  const read = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          deliver = resolve;
        }),
    )
    .mockImplementation(async (request: { offset: number }) =>
      page(nextBody, 'revision-two', request.offset),
    );
  bridge(read);
  const host = render(<MessageText {...props} />);
  await waitFor(() => expect(deliver).toBeDefined());
  host.rerender(<MessageText {...props} sealedRevision="revision-two" />);
  await waitFor(() => expect(host.container.textContent).toBe(nextBody));
  await act(async () => {
    deliver?.(page(body, props.sealedRevision, 2048));
  });
  expect(host.container.textContent).toBe(nextBody);
});
