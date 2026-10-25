import { afterEach, describe, expect, it, vi } from 'vitest';
import { CoreRpcError } from './core-transport.js';
import { fixture, binding } from './core-transport.fixture.js';

vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

// The acceptance layer's only observation of the send stage. These cases are about the SEAM being
// narrow and inert: exactly two emission points, on the real path only, carrying identity only, and
// unable to fail a request.
describe('Core transport lifecycle seam', () => {
  it('reports both stages at the hand-over point, before any byte reached the writer', async () => {
    const records: { method?: string; stage?: string }[] = [];
    const f = await fixture({ onLifecycle: (record) => records.push(record as never) });
    for (const message of ['runtime.handshake.finish']) {
      expect(
        records.filter((record) => record.method === message).map((record) => record.stage),
      ).toEqual(['pending-write', 'submitted']);
    }
    // Hold the writer: nothing is written, yet both stages are already reported. `submitted` marks the
    // hand-over to the writer, which is exactly the boundary that decides `not_sent` vs "query".
    f.holdWrites();
    const pending = f.transport.request('agent.status', {}, 1, 5_000);
    await Promise.resolve();
    await Promise.resolve();
    expect(
      records.filter((record) => record.method === 'agent.status').map((record) => record.stage),
    ).toEqual(['pending-write', 'submitted']);
    expect(f.sent.some((message) => message.method === 'agent.status')).toBe(false);
    f.releaseWrites();
    await Promise.resolve();
    await Promise.resolve();
    f.deliver({
      jsonrpc: '2.0',
      id: f.sent.at(-1)?.id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await expect(pending).resolves.toEqual({ state: 'completed' });
    expect(f.transport.lifecycleFacts()).toMatchObject({ failures: 0, seen: 4 });
    f.transport.close();
  });

  it('never emits on the withdrawn path, so an unsent request has no submitted record', async () => {
    const records: { method?: string; stage?: string }[] = [];
    const f = await fixture({ onLifecycle: (record) => records.push(record as never) });
    f.holdWrites();
    const holder = f.transport.request('workspace.create', {}, 1, 60).catch(() => undefined);
    await Promise.resolve();
    const gone = f.transport.request('agent.status', {}, 1, 5).catch(() => undefined);
    await gone;
    f.releaseWrites();
    await holder;
    await Promise.resolve();
    await Promise.resolve();
    expect(f.transport.requestStats().withdrawn).toBe(1);
    expect(
      records.filter((record) => record.method === 'agent.status').map((record) => record.stage),
    ).toEqual(['pending-write']);
    f.transport.close();
  });

  it('carries identity only, and a throwing observer is counted instead of failing the request', async () => {
    const seen: string[] = [];
    const f = await fixture({
      onLifecycle: (record) => {
        seen.push(JSON.stringify(record));
        throw new Error('synthetic observer failure');
      },
    });
    const request = f.transport.request(
      'agent.status',
      { nonce: 'NONCE-MUST-NOT-APPEAR', secret: 'PARAMS-MUST-NOT-APPEAR' },
      1,
      1_000,
    );
    await Promise.resolve();
    await Promise.resolve();
    expect(seen.length).toBeGreaterThan(0);
    for (const record of seen) {
      expect(record).not.toContain('PARAMS-MUST-NOT-APPEAR');
      expect(record).not.toContain('NONCE-MUST-NOT-APPEAR');
    }
    f.deliver({
      jsonrpc: '2.0',
      id: f.sent.at(-1)?.id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await expect(request).resolves.toEqual({ state: 'completed' });
    expect(f.transport.lifecycleFacts()).toMatchObject({
      failures: 4,
      last_failure: 'synthetic observer failure',
      observer_attached: true,
    });
    expect(f.transport.requestStats().lifecycleFailures).toBe(4);
    f.transport.close();
  });

  it('reports the channel it really authenticated and can be detached again', async () => {
    const records: unknown[] = [];
    const f = await fixture({ onLifecycle: (record) => records.push(record) });
    expect(f.transport.channelFacts()).toEqual({
      authenticated: true,
      channel_id: binding.channelId,
      closed: false,
      connection_epoch: 1,
      observer_attached: true,
    });
    f.transport.attachLifecycleObserver(undefined);
    expect(f.transport.channelFacts().observer_attached).toBe(false);
    const after = records.length;
    const request = f.transport.request('agent.status', {}, 1, 1_000);
    await Promise.resolve();
    expect(records.length).toBe(after);
    f.deliver({
      jsonrpc: '2.0',
      id: f.sent.at(-1)?.id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await expect(request).resolves.toEqual({ state: 'completed' });
    f.transport.close();
    expect(f.transport.channelFacts().closed).toBe(true);
  });

  it('reports the consumed legal late terminal as a record about THAT request', async () => {
    // The per-request fact E1/E2 require: this record names the request whose first-late terminal was
    // consumed, so it can be attributed. `late` alone counts the channel and cannot say whose it was.
    vi.useFakeTimers();
    const records: { method?: string; request_id?: string; stage?: string }[] = [];
    const f = await fixture({ onLifecycle: (record) => records.push(record as never) });
    try {
      const answer = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await answer) as CoreRpcError;
      const id = failure.envelope.correlation_id;
      expect(
        records.filter((record) => record.request_id === id).map((record) => record.stage),
      ).toEqual(['pending-write', 'submitted']);

      f.deliver({ jsonrpc: '2.0', id, result: { terminal: 'succeeded', value: {} } });
      await vi.advanceTimersByTimeAsync(0);
      expect(
        records.filter((record) => record.request_id === id).map((record) => record.stage),
      ).toEqual(['pending-write', 'submitted', 'late-terminal-consumed']);
      const consumed = records.find((record) => record.stage === 'late-terminal-consumed');
      expect(consumed).toMatchObject({
        method: 'agent.status',
        request_id: id,
      });
      // Reporting the consumption changed nothing about the request: the waiter still holds its own
      // timeout and the channel stayed open.
      expect(failure.envelope.code).toBe('ipc.request_timeout');
      expect(f.transport.requestStats().late).toBe(1);
      expect(f.onClosed).not.toHaveBeenCalled();
    } finally {
      f.transport.close();
    }
  });

  it('counts a throwing observer on the late-consumption report without failing the channel', async () => {
    vi.useFakeTimers();
    const f = await fixture({
      onLifecycle: () => {
        throw new Error('synthetic observer failure');
      },
    });
    try {
      const answer = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await answer) as CoreRpcError;
      expect(failure.envelope.code).toBe('ipc.request_timeout');
      const before = f.transport.lifecycleFacts();
      f.deliver({
        jsonrpc: '2.0',
        id: failure.envelope.correlation_id,
        result: { terminal: 'succeeded', value: {} },
      });
      await vi.advanceTimersByTimeAsync(0);
      const after = f.transport.lifecycleFacts();
      // The late-consumption report really was attempted (and its own throw was counted)...
      expect(after.seen).toBeGreaterThan(before.seen);
      expect(after.failures).toBeGreaterThan(before.failures);
      // ...and it did not become a protocol failure of the request or of the channel.
      expect(f.transport.requestStats().late).toBe(1);
      expect(f.onClosed).not.toHaveBeenCalled();
    } finally {
      f.transport.close();
    }
  });
});
