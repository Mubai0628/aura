import { afterEach, describe, expect, it, vi } from 'vitest';
import { CoreRpcError } from './core-transport.js';
import { idFromBytes } from './ids.js';
import { fixture, binding } from './core-transport.fixture.js';

vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe('Core transport terminals', () => {
  it('retires the channel when a binary frame write may have failed partway', async () => {
    const f = await fixture();
    try {
      f.fail(2);
      await expect(
        f.transport.sendBinaryStream(
          idFromBytes('str', new Uint8Array(16)),
          Buffer.from('synthetic binary payload'),
        ),
      ).rejects.toThrow('synthetic write failure after a possible partial write');
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.pipe.close).toHaveBeenCalledOnce();
      await expect(f.transport.request('agent.status', {}, 1, 10)).rejects.toThrow();
    } finally {
      f.transport.close();
    }
  });

  it('settles only the timed-out request and leaves the channel usable', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const slow = f.transport.request('agent.status', {}, 1, 10).catch((error: unknown) => error);
    const other = f.transport
      .request('workspace.create', {}, 1, 5_000)
      .catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(10);

    const failure = await slow;
    expect(failure).toBeInstanceOf(CoreRpcError);
    expect((failure as CoreRpcError).envelope).toMatchObject({
      code: 'ipc.request_timeout',
      // `cancelled`, not `failed`: the Core is healthy and only this caller stopped waiting. A
      // `failed` terminal would read as "Core is broken".
      terminal: 'cancelled',
      details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
    });

    // The other request is untouched: that is the whole point of request-local settlement.
    expect(f.onClosed).not.toHaveBeenCalled();
    expect(f.pipe.cancel).not.toHaveBeenCalled();
    expect(f.pipe.close).not.toHaveBeenCalled();

    // And the channel still carries new work.
    const later = f.transport.request('agent.status', {}, 1, 1_000);
    await vi.advanceTimersByTimeAsync(0);
    f.deliver({
      jsonrpc: '2.0',
      id: f.sent.at(-1)?.id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await expect(later).resolves.toEqual({ state: 'completed' });
    expect(f.onClosed).not.toHaveBeenCalled();

    // The slow request really is still live, not silently settled.
    f.deliver({
      jsonrpc: '2.0',
      id: f.sent.find((item) => item.method === 'workspace.create')?.id,
      result: { terminal: 'succeeded', value: { created: true } },
    });
    await expect(other).resolves.toEqual({ created: true });
    f.transport.close();
  });

  it('withdraws a timed-out request instead of dispatching it after the waiter was rejected', async () => {
    // The outbound barrier is held by an earlier write, so the second request settles while its own
    // write is still queued and provably unsent. Reporting `not_sent` is only honest if the frame is
    // then actually withdrawn - otherwise the bytes go out with no caller left to hear about it.
    const f = await fixture();
    f.holdWrites();
    // The holder occupies the barrier. Nothing ever answers it, so give it its own short deadline;
    // this test is about the second request's withdrawal, not about the holder's outcome.
    const holder = f.transport.request('workspace.create', {}, 1, 60).catch(() => undefined);
    await Promise.resolve();
    const before = f.sent.length;

    const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
    const failure = await gone;
    expect((failure as CoreRpcError).envelope.code).toBe('ipc.request_timeout');

    f.releaseWrites();
    await holder;
    await Promise.resolve();
    await Promise.resolve();
    expect(f.transport.requestStats().withdrawn).toBe(1);
    expect(f.sent.slice(before).map((item) => item.method)).not.toContain('agent.status');
    expect(f.onClosed).not.toHaveBeenCalled();
    f.transport.close();
  });

  it('accepts a legal late terminal without delivering it to the settled caller', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const settled = f.transport.request('agent.status', {}, 1, 10).catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(10);
    const failure = await settled;
    expect((failure as CoreRpcError).envelope.code).toBe('ipc.request_timeout');

    // The Core answers anyway. This is legal - the client gave up first - so it must NOT be a protocol
    // violation, and the late value must NOT be handed to a caller that already timed out.
    f.deliver({
      jsonrpc: '2.0',
      id: f.sent.find((item) => item.method === 'agent.status')?.id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await vi.advanceTimersByTimeAsync(50);
    expect(f.onClosed).not.toHaveBeenCalled();
    expect((failure as CoreRpcError).envelope.terminal).toBe('cancelled');
    expect(f.transport.requestStats().late).toBe(1);
    f.transport.close();
  });

  it('treats a repeated terminal for an already-answered request as a violation', async () => {
    const f = await fixture();
    const answered = f.transport.request('agent.status', {}, 1, 1_000);
    await Promise.resolve();
    const id = f.sent.at(-1)?.id;
    f.deliver({
      jsonrpc: '2.0',
      id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await expect(answered).resolves.toEqual({ state: 'completed' });

    // A second terminal for the same id is a defect, not a late answer: the client never gave up.
    f.deliver({
      jsonrpc: '2.0',
      id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await Promise.resolve();
    expect(f.onClosed).toHaveBeenCalledWith(
      expect.objectContaining({ code: 'ipc.protocol_violation', terminal: 'protocol_violation' }),
    );
  });

  it('still rejects every in-flight request of this channel on a real disconnect', async () => {
    const f = await fixture();
    const first = f.transport.request('agent.status', {}, 1, 10_000).catch((e: unknown) => e);
    const second = f.transport.request('workspace.create', {}, 1, 10_000).catch((e: unknown) => e);
    await Promise.resolve();
    f.transport.close();
    for (const error of [await first, await second]) {
      expect(error).toBeInstanceOf(CoreRpcError);
      expect((error as CoreRpcError).envelope).toMatchObject({
        code: 'ipc.transport_lost',
        terminal: 'transport_lost',
        details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
      });
      expect((error as CoreRpcError).envelope.details.command_invocation_id).toBeDefined();
    }
  });

  it('still rejects unknown responses on an active channel', async () => {
    const f = await fixture();
    f.deliver({ jsonrpc: '2.0', id: 'unknown', result: { terminal: 'succeeded', value: {} } });
    await Promise.resolve();
    expect(f.onClosed).toHaveBeenCalledWith(
      expect.objectContaining({
        code: 'ipc.protocol_violation',
        terminal: 'protocol_violation',
      }),
    );
  });

  it('still treats a wrong epoch or a wrong sequence as a violation', async () => {
    const f = await fixture();
    // Another connection's frame, or a replayed one: this channel must not act on it.
    f.deliverRaw(
      { jsonrpc: '2.0', id: 'x', result: { terminal: 'succeeded', value: {} } },
      { channelId: binding.channelId, connectionEpoch: 2n },
      0n,
    );
    await Promise.resolve();
    expect(f.onClosed).toHaveBeenCalledWith(
      expect.objectContaining({ code: 'ipc.protocol_violation' }),
    );
  });

  it('still treats a repeated sequence number as a violation', async () => {
    const f = await fixture();
    const answered = f.transport.request('agent.status', {}, 1, 1_000);
    await Promise.resolve();
    const id = f.sent.at(-1)?.id;
    f.deliver({
      jsonrpc: '2.0',
      id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await expect(answered).resolves.toEqual({ state: 'completed' });
    // Reuse the previous sequence number: duplicate/reversed/missing frames stay fatal.
    f.deliverRaw(
      { jsonrpc: '2.0', id: 'y', result: { terminal: 'succeeded', value: {} } },
      binding,
      0n,
    );
    await Promise.resolve();
    expect(f.onClosed).toHaveBeenCalledWith(
      expect.objectContaining({ code: 'ipc.protocol_violation' }),
    );
  });

  it('keeps delivering notifications and responses after one request times out', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const notifications: string[] = [];
    const timedOut = f.transport.request('agent.status', {}, 1, 5).catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(5);
    expect(((await timedOut) as CoreRpcError).envelope.code).toBe('ipc.request_timeout');

    // A notification is not a request: one caller giving up must not interrupt the subscription stream.
    f.deliver({ jsonrpc: '2.0', method: 'projection.changed', params: { revision: 2 } });
    await vi.advanceTimersByTimeAsync(0);
    expect(f.onNotification).toHaveBeenCalledWith('projection.changed', { revision: 2 });
    expect(f.onClosed).not.toHaveBeenCalled();
    f.transport.close();
    void notifications;
  });

  it('keeps a healthy channel open after a response arrives before its deadline', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const result = f.transport.request('agent.status', {}, 1, 10);
    await vi.advanceTimersByTimeAsync(0);
    f.deliver({
      jsonrpc: '2.0',
      id: f.sent.at(-1)?.id,
      result: { terminal: 'succeeded', value: { state: 'completed' } },
    });
    await expect(result).resolves.toEqual({ state: 'completed' });
    await vi.advanceTimersByTimeAsync(20);
    expect(f.onClosed).not.toHaveBeenCalled();
    f.transport.close();
  });

  it('consumes exactly one legal late response and rejects the next terminal as a duplicate', async () => {
    // Independent counterexample 1 (transport-probes.test.ts): submitted -> timeout ->
    // classifyLate -> classifyLate used to answer `legal-late` both times, so the second final
    // response for the same id neither closed the channel nor was rejected.
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const answer = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await answer) as CoreRpcError;
      expect(failure).toBeInstanceOf(CoreRpcError);
      const id = failure.envelope.correlation_id;
      // The id the caller gave up on is the id that really went out on the wire.
      expect(f.sent.at(-1)?.id).toBe(id);
      const response = { jsonrpc: '2.0', id, result: { terminal: 'succeeded', value: {} } };

      // The first final response after the client gave up is the ONE legal late terminal.
      f.deliver(response);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats().late).toBe(1);

      // A second final response for the same id is a duplicate terminal, not another late answer.
      f.deliver(response);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.onClosed).toHaveBeenCalledWith(
        expect.objectContaining({ code: 'ipc.protocol_violation', terminal: 'protocol_violation' }),
      );
      expect(f.transport.requestStats().duplicates).toBe(1);
      // The settled caller still holds its own timeout error, never the late value.
      expect(failure.envelope.code).toBe('ipc.request_timeout');
    } finally {
      f.transport.close();
    }
  });

  it('retires the channel when the writer cannot prove frame integrity', async () => {
    // Independent counterexample 2 (transport-probes.test.ts): a sink failure after a possible partial
    // write used to reject only this waiter while the writer had already closed its own queue.
    const f = await fixture();
    try {
      f.fail();
      const result = await f.transport
        .request('workspace.create', {}, 1, 500)
        .catch((error: unknown) => error);
      expect(result).toBeInstanceOf(CoreRpcError);
      expect((result as CoreRpcError).envelope.terminal).toBe('transport_lost');
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.pipe.close).toHaveBeenCalledOnce();
    } finally {
      f.transport.close();
    }
  });

  it('settles every in-flight request of this channel when the sink fails', async () => {
    const f = await fixture();
    try {
      f.holdWrites();
      const first = f.transport
        .request('agent.status', {}, 1, 10_000)
        .catch((error: unknown) => error);
      const second = f.transport
        .request('workspace.create', {}, 1, 10_000)
        .catch((error: unknown) => error);
      await Promise.resolve();
      f.fail();
      f.releaseWrites();

      // The channel owner retires the connection once, and that one retirement settles both waiters
      // with the queryable-outcome hint instead of a generic failure.
      for (const settled of [await first, await second]) {
        expect(settled).toBeInstanceOf(CoreRpcError);
        expect((settled as CoreRpcError).envelope).toMatchObject({
          code: 'ipc.transport_lost',
          details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
          terminal: 'transport_lost',
        });
      }
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.pipe.close).toHaveBeenCalledOnce();
    } finally {
      f.transport.close();
    }
  });

  it('handles a write failure that arrives after the waiter already timed out', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      f.holdWrites();
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      expect(await gone).toBeInstanceOf(CoreRpcError);
      // The waiter is already settled, so `#settle` returns undefined - that must not swallow the
      // connection failure.
      f.fail();
      f.releaseWrites();
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.onClosed).toHaveBeenCalledWith(
        expect.objectContaining({ terminal: 'transport_lost' }),
      );
      expect(f.pipe.close).toHaveBeenCalledOnce();
    } finally {
      f.transport.close();
    }
  });
});
