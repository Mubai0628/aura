// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { fixture } from './core-supervisor-tests/fixture.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

// Core answers every subscription with the route's current state. A route released while its
// subscription is still on the way (a page that loads again at startup) has to be unsubscribed in
// Core too, so the next subscription is sent and answered, not taken as already held.

const selector = {
  projection_kind: 'session_model_selector',
  projection_scope: { type: 'session', session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV' },
} as const;

test('a model route released before Core answered its subscription is unsubscribed, and subscribed again', async () => {
  const f = await fixture();
  const answer = Promise.withResolvers<unknown>();
  const sent: string[] = [];
  const usual = f.transport.request.getMockImplementation()!;
  f.transport.request.mockImplementation(async (method, params, ...rest) => {
    const kind = (params as { projection_kind?: string } | undefined)?.projection_kind;
    if (kind === 'session_model_selector') {
      sent.push(method);
      if (method === 'projection.unsubscribe') return { unsubscribed: true };
      if (method === 'projection.subscribe' && sent.length === 1) return answer.promise;
      return { subscribed: true };
    }
    return usual(method, params, ...rest);
  });
  const first = f.supervisor.subscribeModelProjection(selector);
  await f.supervisor.unsubscribeModelProjection(selector);
  answer.resolve({ subscribed: true });
  await first;
  await f.supervisor.subscribeModelProjection(selector);
  expect(sent).toEqual(['projection.subscribe', 'projection.unsubscribe', 'projection.subscribe']);
});

test('a model route asked for again while held is asked of Core again, and released once', async () => {
  const f = await fixture();
  const sent: string[] = [];
  const usual = f.transport.request.getMockImplementation()!;
  f.transport.request.mockImplementation(async (method, params, ...rest) => {
    if (
      (params as { projection_kind?: string } | undefined)?.projection_kind ===
      'session_model_selector'
    ) {
      sent.push(method);
      return method === 'projection.subscribe' ? { subscribed: true } : { unsubscribed: true };
    }
    return usual(method, params, ...rest);
  });
  await f.supervisor.subscribeModelProjection(selector);
  await f.supervisor.subscribeModelProjection(selector);
  await f.supervisor.unsubscribeModelProjection(selector);
  await f.supervisor.unsubscribeModelProjection(selector);
  expect(sent).toEqual(['projection.subscribe', 'projection.subscribe', 'projection.unsubscribe']);
});
