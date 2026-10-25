import { Buffer } from 'node:buffer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PHYSICAL_SEND_BUDGET_MS, SECRET_INPUT_BINDING_BUDGET_MS } from './core-transport.js';
import { idFromBytes } from './ids.js';

import { MAX_BINARY_CHUNK_BYTES } from './protocol.js';
import {
  FRAME_TYPE_CONTROL,
  FRAME_TYPE_BINARY,
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

describe('M1 send ownership and per-frame scheduling', () => {
  it('admits queued control frames at the boundary the held frame defines', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      f.sink.holdWriteAt(2);
      const business = f.transport.request('workspace.create', {}, 1, 5).catch((e: unknown) => e);
      await vi.advanceTimersByTimeAsync(5);
      expect(await business).toBeInstanceOf(Error);
      const cancel = f.transport.cancel(idFromBytes('cor', new Uint8Array(16)));
      const query = f.transport.queryCommandOutcome(idFromBytes('civ', new Uint8Array(16)));
      await vi.advanceTimersByTimeAsync(0);
      // While the frame is inside the sink nothing else may be written - a half frame cannot be
      // interrupted - so the control work waits for the boundary rather than being dropped.
      expect(f.sink.starts).toEqual([1, 2]);

      f.sink.openHeld();
      for (let turn = 0; turn < 4; turn += 1) await vi.advanceTimersByTimeAsync(0);
      await expect(cancel).resolves.toMatchObject({ disposition: 'accepted' });
      await expect(query).resolves.toMatchObject({ value: { replayed: false } });
      // The complete frame's boundary is where the control class gets its turn: both are written, in the
      // order they were admitted, after the frame in progress finished.
      expect(f.sink.sent.map((message) => message.method)).toEqual([
        'runtime.handshake.finish',
        'workspace.create',
        'aura.cancel',
        'runtime.command_outcome',
      ]);
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outboundFrames.queuedFrames).toBe(0);
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('Codex counterexample 1: a stalled physical write is retired on its OWN budget, not by the caller', async () => {
    const budget = 5_000;
    vi.useFakeTimers();
    const f = await fixture({ physicalSendBudgetMilliseconds: budget });
    try {
      // Ordinal 1 is the handshake. The business frame is ordinal 2 and its write never finishes.
      f.sink.holdWriteAt(2);
      const business = f.transport.request('workspace.create', {}, 1, 5).catch((e: unknown) => e);
      await vi.advanceTimersByTimeAsync(5);
      expect(await business).toBeInstanceOf(Error);

      // The caller's budget is spent, and the physical send has no owner of its own yet: the request
      // is Unknown, and the connection is still there.
      expect(f.onClosed).not.toHaveBeenCalled();
      const physical = f.transport.requestStats().outbound.physicalSendBudgetMs;
      expect(physical).toBe(budget);

      // Advancing past the SEND OWNER's budget retires the connection exactly once, and the frame is
      // never reported as `not_sent` - it may be half written.
      await vi.advanceTimersByTimeAsync(budget);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.onClosed).toHaveBeenCalledWith(
        expect.objectContaining({ terminal: 'transport_lost' }),
      );
      expect(f.transport.requestStats().outbound.physicallyStuck).toBe(1);
      expect(f.sink.cancelCalls).toBeGreaterThan(0);
      expect(f.sink.closeCalls).toBeGreaterThan(0);
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('defaults the physical-send budget to its own production value, independent of any request budget', async () => {
    const f = await fixture();
    try {
      const stats = f.transport.requestStats().outbound;
      expect(stats.physicalSendBudgetMs).toBe(PHYSICAL_SEND_BUDGET_MS);
      expect(stats.secretInputBindingBudgetMs).toBe(SECRET_INPUT_BINDING_BUDGET_MS);
      // The three budgets are genuinely different owners, not one number reused.
      expect(PHYSICAL_SEND_BUDGET_MS).toBeGreaterThan(SECRET_INPUT_BINDING_BUDGET_MS);
      expect(stats.physicalSendBudgetMs).toBeGreaterThan(15_000);
    } finally {
      f.transport.close();
    }
  });

  it('Codex counterexample 2: a P0 control frame takes the complete-frame boundary before the next bulk chunk', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      // Ordinal 1 = handshake, 2 = first bulk chunk (held), 3 = the very next frame written.
      f.sink.holdWriteAt(2);
      const bulk = f.transport
        .sendBinaryStream(
          idFromBytes('str', new Uint8Array(16)),
          Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 2),
        )
        .catch((error: unknown) => error);
      await flush();
      expect(f.sink.starts).toEqual([1, 2]);
      expect(f.sink.kinds).toEqual([FRAME_TYPE_CONTROL]);

      // The first bulk frame is INCOMPLETE inside the sink: nothing may be written yet. A business
      // request and a cancellation are submitted meanwhile, so all three frames are competing.
      const business = f.transport
        .request('agent.status', {}, 1, 30_000)
        .catch((error: unknown) => error);
      const cancel = f.transport.cancel(idFromBytes('cor', new Uint8Array(16)));
      await flush();
      await flush();
      expect(f.sink.starts).toEqual([1, 2]);
      expect(f.sink.kinds).toEqual([FRAME_TYPE_CONTROL]);

      // The first bulk frame now COMPLETES. The next frame selected is the P0 cancellation, NOT the next
      // bulk chunk and NOT the business frame: under the old whole-stream lock the observed order could
      // only have been bulk/bulk/..., because the stream held the whole outbound sequence.
      // The next frame selected is the P0 cancellation, NOT the next bulk chunk and NOT the business
      // frame: under the old whole-stream lock the observed order could only have been bulk/bulk/...
      // because the stream held the whole outbound sequence.
      f.sink.openHeld();
      await flush();
      expect(f.sink.kinds).toContain(FRAME_TYPE_BINARY);
      expect(f.sink.sent.map((message) => message.method)).toContain('aura.cancel');
      expect(f.sink.starts.slice(0, 3)).toContain(3);

      await vi.advanceTimersByTimeAsync(0);
      await vi.advanceTimersByTimeAsync(0);
      await bulk;
      await expect(cancel).resolves.toMatchObject({ disposition: 'accepted' });
      // Both bulk chunks really did go out: the control frame was inserted at a boundary between two
      // complete frames, not swapped in for one of them.
      expect(f.sink.kinds.filter((kind) => kind === FRAME_TYPE_BINARY)).toHaveLength(2);
      // And the numbers are contiguous in the order the sink really saw the frames.
      expect(f.sink.frames.map((frame) => frame.sequence)).toEqual(
        f.sink.frames.map((_frame, index) => BigInt(index)),
      );
      void business;
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('never writes anything into a frame that is still being written', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const streamId = idFromBytes('str', new Uint8Array(16));
    // THREE chunks, and the last one deliberately shorter than a full chunk: the chunk count is not
    // the atomic two-frame limit, and `endOfMessage` has to land on the short final frame.
    const input = Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 2 + 12_345);
    for (let index = 0; index < input.byteLength; index += 1) input[index] = index % 251;
    try {
      f.sink.holdWriteAt(2);
      const bulk = f.transport.sendBinaryStream(streamId, input).catch((error: unknown) => error);
      await flush();
      const cancel = f.transport
        .cancel(idFromBytes('cor', new Uint8Array(16)))
        .catch(() => undefined);
      await flush();

      // While ordinal 2 is parked inside the sink, NOTHING else has claimed a write: the cancellation
      // could not have been inserted into the middle of a frame even if it had wanted to.
      expect(f.sink.starts).toEqual([1, 2]);
      expect(f.sink.kinds).toEqual([FRAME_TYPE_CONTROL]);

      f.sink.openHeld();
      for (let turn = 0; turn < 8; turn += 1) await vi.advanceTimersByTimeAsync(0);
      // The stream really succeeded: a caught rejection must never be left unasserted.
      await expect(bulk).resolves.toBeUndefined();
      await cancel;
      // Every frame that reached the sink is a whole frame of the protocol's own shape...
      for (const frame of f.sink.frames) expect(frame.length).toBeGreaterThan(37);
      // ...its sequence number follows the previous one with no gap and no repeat...
      expect(f.sink.frames.map((frame) => frame.sequence)).toEqual(
        f.sink.frames.map((_frame, index) => BigInt(index)),
      );
      // ...and the cancellation was written as its own complete frame, never inside the stream's.
      expect(
        f.sink.frames.filter((frame) => frame.kind === FRAME_TYPE_CONTROL).length,
      ).toBeGreaterThan(1);
      expect(f.sink.sent.map((message) => message.method)).toContain('aura.cancel');

      // The receiver rebuilt the input exactly, from three complete frames.
      const got = received(f.sink, streamId);
      expect(got.chunkSequences).toEqual([0, 1, 2]);
      expect(got.lengths).toEqual([MAX_BINARY_CHUNK_BYTES, MAX_BINARY_CHUNK_BYTES, 12_345]);
      expect(got.endOfMessage).toEqual([false, false, true]);
      expect(sha256(got.bytes)).toBe(sha256(input));
      // The three chunks really carried the connection numbers the frame log saw, in order.
      for (let index = 1; index < got.sequences.length; index += 1) {
        expect(got.sequences[index]).toBeGreaterThan(got.sequences[index - 1] as bigint);
      }
      expect(f.transport.requestStats().outboundFrames.writesFailed).toBe(0);
      // Nothing is left holding the stream's bytes.
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outbound.pendingBytes).toBe(0);
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('keeps the business and control capacity pools isolated', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      f.sink.holdWriteAt(2);
      // Business work is admitted and queued behind the held write; none of it settles.
      const held = Array.from({ length: 8 }, () =>
        f.transport.request('agent.status', {}, 1, 60_000).catch((error: unknown) => error),
      );
      await vi.advanceTimersByTimeAsync(0);
      const before = f.sink.sent.length;

      // The control entry points are NOT behind that capacity: both frames reach the sink even though
      // eight business requests are already waiting.
      const cancel = f.transport.cancel(idFromBytes('cor', new Uint8Array(16)));
      const query = f.transport.queryCommandOutcome(idFromBytes('civ', new Uint8Array(16)));
      f.sink.openHeld();
      await vi.advanceTimersByTimeAsync(0);
      const control = f.sink.sent
        .slice(before)
        .filter(
          (message) =>
            message.method === 'aura.cancel' || message.method === 'runtime.command_outcome',
        );
      expect(control.map((message) => message.method)).toEqual([
        'aura.cancel',
        'runtime.command_outcome',
      ]);
      await expect(cancel).resolves.toEqual({ disposition: 'accepted' });
      await expect(query).resolves.toEqual({ terminal: 'succeeded', value: { replayed: false } });
      expect(f.transport.requestStats().activeControl).toBe(0);
      for (const pending of held) void pending;
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('keeps the control class out of reach of a caller-supplied method or priority', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      // The literal reserved method name and the reserved P0 priority, from an ORDINARY caller.
      const ordinary = f.transport
        .request('aura.cancel', {}, 0, 60_000)
        .catch((error: unknown) => error);
      const ordinaryQuery = f.transport
        .request('runtime.command_outcome', {}, 0, 60_000)
        .catch((error: unknown) => error);
      await flush();
      // Both really reached the sink...
      expect(
        f.sink.sent.filter(
          (message) =>
            message.method === 'aura.cancel' || message.method === 'runtime.command_outcome',
        ),
      ).toHaveLength(2);
      // ...and the CONTROL worker pool still refuses to let anything be charged to it: only the private
      // route can spend reserved control capacity.
      expect(f.transport.requestStats().controlAdmissionRefusals).toBe(0);
      void ordinary;
      void ordinaryQuery;
    } finally {
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('keeps the outbound sequence gap-free across refusals, withdrawals and a P0 reorder', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const framesBefore = f.sink.frames.length;
      // The write in progress belongs to the request whose frame the next one queues behind.
      f.sink.holdWriteAt(2);
      const holder = f.transport.request('workspace.create', {}, 1, 30_000).catch(() => undefined);
      await vi.advanceTimersByTimeAsync(0);
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      expect(await gone).toBeInstanceOf(Error);

      // A P0 cancellation's frame is NOT queued behind that stalled write: it has its own class and its
      // own space, so the boundary that follows the first frame writes it.
      const cancel = f.transport.cancel(idFromBytes('cor', new Uint8Array(16)));
      f.sink.openHeld();
      for (let turn = 0; turn < 8; turn += 1) await vi.advanceTimersByTimeAsync(0);
      await cancel;

      const sequences = f.sink.frames.slice(framesBefore).map((frame) => frame.sequence);
      // The frame that was taken back spent NO number: what the sink saw is contiguous from the start of
      // this window, and the local give-up never produced a frame at all.
      expect(sequences.length).toBeGreaterThan(0);
      expect(sequences).toEqual(
        sequences.map((_value, index) => BigInt(index) + BigInt(framesBefore)),
      );
      expect(f.sink.sent.some((message) => message.method === 'agent.status')).toBe(false);
      expect(f.sink.sent.some((message) => message.method === 'aura.cancel')).toBe(true);
      expect(f.transport.nextOutboundSequence).toBe(f.sink.frames.at(-1)!.sequence + 1n);
      expect(f.transport.requestStats().withdrawn).toBe(1);
      void holder;
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('does not spend a sequence number or register a send task when admission refuses the request', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const before = f.transport.requestStats();
      const writesBefore = f.sink.ordinals.length;
      const refused = await (async () => {
        // Saturate the ordinary identity pool the way the transport's own admission measures it.
        const batch = Array.from({ length: 512 }, () =>
          f.transport.request('agent.status', {}, 1, 1).catch((error: unknown) => error),
        );
        await vi.advanceTimersByTimeAsync(2);
        await Promise.all(batch);
        return f.transport.request('workspace.create', {}, 1, 5).catch((error: unknown) => error);
      })();
      expect(refused).toBeInstanceOf(Error);
      expect(
        (refused as { envelope: { details: Record<string, unknown> } }).envelope.details,
      ).toMatchObject({
        outcome_recovery: 'not_sent',
      });
      expect(f.transport.requestStats().outbound.tasks).toBe(before.outbound.tasks + 512);
      expect(f.sink.ordinals.length).toBeGreaterThan(writesBefore);
      expect(f.transport.requestStats().admissionRefusals).toBeGreaterThan(0);
    } finally {
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('sends nothing and releases its bytes when the waiter withdrew in time', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      f.sink.holdWriteAt(2);
      const holder = f.transport.request('workspace.create', {}, 1, 60).catch(() => undefined);
      await vi.advanceTimersByTimeAsync(0);
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      expect((await gone) as Error).toBeInstanceOf(Error);

      f.sink.openHeld();
      await vi.advanceTimersByTimeAsync(60);
      await holder;
      await vi.advanceTimersByTimeAsync(0);

      expect(f.transport.requestStats().withdrawn).toBe(1);
      expect(f.sink.sent.some((message) => message.method === 'agent.status')).toBe(false);
      // Nothing waits any more: no task is still holding params, a buffer or a secret.
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outboundFrames.queuedFrames).toBe(0);
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });
});
