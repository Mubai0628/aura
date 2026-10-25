import { Buffer } from 'node:buffer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { idFromBytes } from './ids.js';

import { MAX_BINARY_CHUNK_BYTES } from './protocol.js';
import {
  isUnconfirmedCommandOutcome,
  OUTCOME_RECOVERY_NOT_SENT,
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
} from '../shared/ipc-request-failure.js';
import {
  FRAME_TYPE_CONTROL,
  fixture,
  flush,
  sha256,
  received,
} from './outbound-send-tests/support.js';

vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe('R2 deadlines: one complete-frame write, counted per frame', () => {
  it('does not retire a stream that takes longer than one frame budget while every frame finishes', async () => {
    vi.useFakeTimers();
    // The PRODUCTION budget, not a shortened one: 2 x 70 s of real work is 140 s in total, which is
    // longer than 120 s - and it must still not be read as "one write never progressed".
    const f = await fixture();
    const streamId = idFromBytes('str', new Uint8Array(16));
    const input = Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 2, 0x5a);
    try {
      f.sink.holdWriteAt(2);
      f.sink.holdWriteAt(3);
      const bulk = f.transport.sendBinaryStream(streamId, input).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.sink.starts).toEqual([1, 2]);

      // Frame one takes 70 s: under its own 120 s.
      await vi.advanceTimersByTimeAsync(70_000);
      expect(f.onClosed).not.toHaveBeenCalled();
      f.sink.openWriteAt(2);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.sink.starts).toEqual([1, 2, 3]);

      // Frame two takes another 70 s. The task is now 140 s old, and it is still exactly one write
      // behind: the deadline belongs to the frame, not to the task.
      await vi.advanceTimersByTimeAsync(70_000);
      expect(f.onClosed).not.toHaveBeenCalled();
      f.sink.openWriteAt(3);
      await vi.advanceTimersByTimeAsync(0);

      await expect(bulk).resolves.toBeUndefined();
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats().outbound.physicallyStuck).toBe(0);
      expect(sha256(received(f.sink, streamId).bytes)).toBe(sha256(input));
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('retires exactly once at the declared deadline and never lets a late write revive the owner', async () => {
    const budget = 1_000;
    vi.useFakeTimers();
    const f = await fixture({ physicalSendBudgetMilliseconds: budget });
    try {
      f.sink.holdWriteAt(2);
      const stuck = f.transport.request('workspace.create', {}, 1, 30_000).catch(() => undefined);
      await vi.advanceTimersByTimeAsync(0);
      // A P0 cancellation arrives while the frame is HALF WRITTEN. It may not cross that frame, and
      // it may not be written at all once the deadline retires the channel.
      const cancel = f.transport
        .cancel(idFromBytes('cor', new Uint8Array(16)))
        .catch(() => undefined);
      await flush();
      expect(f.sink.starts).toEqual([1, 2]);
      expect(f.sink.sent.some((message) => message.method === 'aura.cancel')).toBe(false);

      await vi.advanceTimersByTimeAsync(budget);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.transport.requestStats().outbound.physicallyStuck).toBe(1);
      expect(f.transport.requestStats().outboundFrames.writesFailed).toBe(0);

      // The native side now completes the write it was holding. The frame may be complete on the wire,
      // but the channel is over: nothing is adopted, nothing is written behind it, and the retirement
      // is not repeated.
      f.sink.openHeld();
      await vi.advanceTimersByTimeAsync(0);
      await flush();
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.sink.sent.some((message) => message.method === 'aura.cancel')).toBe(false);
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outbound.pendingBytes).toBe(0);
      await cancel;
      await stuck;
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('settles a short injection honestly: not_sent for the frame that never went out, Unknown for the one that did', async () => {
    vi.useFakeTimers();
    const f = await fixture({ physicalSendBudgetMilliseconds: 1_000 });
    try {
      // The first business frame is handed to the sink and never settles; the second cannot be
      // dispatched at all. Both callers give up first, and the two outcomes must NOT read alike.
      f.sink.holdWriteAt(2);
      const dispatched = f.transport
        .request('workspace.create', {}, 1, 5)
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      const queued = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);

      const first = (await dispatched) as {
        envelope: { code: string; details: Record<string, unknown>; terminal: string };
      };
      const second = (await queued) as {
        envelope: { code: string; details: Record<string, unknown>; terminal: string };
      };
      expect(first.envelope.details).toMatchObject({
        outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
      });
      expect(second.envelope.details).toMatchObject({
        outcome_recovery: OUTCOME_RECOVERY_NOT_SENT,
      });
      // The two answers differ in the way the product reads them: one is a possibly-executed command,
      // the other is definite.
      expect(isUnconfirmedCommandOutcome(first.envelope)).toBe(true);
      expect(isUnconfirmedCommandOutcome(second.envelope)).toBe(false);
      expect(f.sink.sent.some((message) => message.method === 'agent.status')).toBe(false);
      // Neither caller's own budget retired the channel: the send owner's frame budget does that.
      expect(f.onClosed).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(1_000);
      expect(f.onClosed).toHaveBeenCalledOnce();
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('counts the atomic binding from its command frame and does not restart it for frame two', async () => {
    const groupBudget = 5_000;
    vi.useFakeTimers();
    const failing = await fixture({
      physicalSendBudgetMilliseconds: 60_000,
      secretInputBindingBudgetMilliseconds: groupBudget,
    });
    const healthy = await fixture();
    const secret = Buffer.from('synthetic-binding-value');
    try {
      failing.sink.holdWriteAt(2);
      failing.sink.holdWriteAt(3);
      const pending = failing.transport
        .requestWithSecretInput(
          'models.endpoint.configure_credential',
          {},
          'models_endpoint_configure_credential',
          secret,
          30_000,
        )
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      // The command frame is still inside the sink: the binding has begun, nothing is written yet.
      expect(failing.sink.kinds.slice(1)).toEqual([]);

      await vi.advanceTimersByTimeAsync(4_000);
      expect(failing.onClosed).not.toHaveBeenCalled();
      // Frame one completes 4 s in. Frame two starts - and 4 s is ALREADY spent of the binding's 5 s.
      failing.sink.openWriteAt(2);
      await vi.advanceTimersByTimeAsync(0);
      expect(failing.sink.kinds.slice(1)).toEqual([FRAME_TYPE_CONTROL]);

      // Frame two hangs. A deadline that restarted with it would fire at 9 s; the binding's own
      // deadline, counted from the command frame, fires now.
      await vi.advanceTimersByTimeAsync(1_001);
      expect(failing.onClosed).toHaveBeenCalledOnce();
      expect(failing.transport.requestStats().outbound.physicallyStuck).toBe(1);
      expect(await pending).toBeInstanceOf(Error);

      // Only THIS connection was retired: the other one is untouched and still carries work.
      expect(healthy.onClosed).not.toHaveBeenCalled();
      const answer = healthy.transport.request('agent.status', {}, 1, 1_000);
      await flush();
      healthy.sink.deliver({
        jsonrpc: '2.0',
        id: healthy.sink.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await expect(answer).resolves.toEqual({ state: 'completed' });
      expect(secret.every((byte) => byte === 0)).toBe(true);
    } finally {
      failing.sink.openHeld();
      healthy.sink.openHeld();
      failing.transport.close();
      healthy.transport.close();
      secret.fill(0);
      await vi.advanceTimersByTimeAsync(0);
    }
  });
});
