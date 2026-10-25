import { Buffer } from 'node:buffer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FrameScheduler, FrameWriteAbortedError } from './frame-scheduler.js';
import { idFromBytes } from './ids.js';

import {
  OutboundSendOwner,
  createTaskKey,
  SecretInputOwnership,
  SendBudgetExceededError,
  STREAM_SOURCE_BUDGET_BYTES,
} from './outbound-send-owner.js';
import { MAX_BINARY_CHUNK_BYTES } from './protocol.js';
import { OUTCOME_RECOVERY_NOT_SENT } from '../shared/ipc-request-failure.js';
import { binding, fixture, flush } from './outbound-send-tests/support.js';

vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe('R3 live withdrawal and full accounting', () => {
  it('holds a task back when its own live predicate says no, spending no number and releasing everything', async () => {
    vi.useFakeTimers();
    const frames: number[] = [];
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const scheduler = new FrameScheduler({
      write: async (bytes) => {
        frames.push(bytes.byteLength);
        if (frames.length === 1) await gate;
      },
    });
    const sender = new OutboundSendOwner(binding, scheduler, {
      onSettled: () => undefined,
      onUnrecoverable: () => undefined,
      onWriteStarted: () => undefined,
    });
    const spec = {
      commandInvocationId: undefined,
      correlationId: idFromBytes('cor', new Uint8Array(16)),
      framePayload: Buffer.from('{}', 'utf8'),
      priority: 1 as const,
    };
    const first = sender.register(spec, createTaskKey('first'));
    const second = sender.register(spec, createTaskKey('second'));
    // The predicate is installed AFTER registration, and it is what the scheduler must read at the
    // moment it would really build the frame.
    second.setWanted(() => false);
    const sequenceBefore = scheduler.nextSequence;
    release();
    await Promise.all([first.done, second.done]);

    expect(frames).toHaveLength(1);
    // The first frame was already committed when the sequence was read, and the second spent NOTHING:
    // a frame refused at the boundary is never numbered.
    expect(scheduler.nextSequence).toBe(sequenceBefore);
    expect(second.outcome()).toMatchObject({ wroteAnything: false, withdrewBeforeWrite: true });
    expect(sender.watching).toBe(0);
    expect(sender.facts().pendingBytes).toBe(0);
    expect(scheduler.facts().withdrawnGroups).toBe(1);
  });

  it('settles a task whose enqueue threw synchronously instead of leaking its reservation', async () => {
    const exploding = new (class extends FrameScheduler {
      override enqueue(): never {
        throw new Error('synthetic synchronous enqueue refusal');
      }
    })({ write: async () => undefined });
    const settled: string[] = [];
    const sender = new OutboundSendOwner(binding, exploding, {
      onSettled: (task) => settled.push(task.label),
      onUnrecoverable: () => undefined,
      onWriteStarted: () => undefined,
    });
    const task = sender.register(
      {
        commandInvocationId: undefined,
        correlationId: idFromBytes('cor', new Uint8Array(16)),
        framePayload: Buffer.from('{}', 'utf8'),
        priority: 1,
      },
      createTaskKey('sync-throw'),
    );
    await task.done;
    expect(settled).toEqual(['sync-throw']);
    expect(task.outcome()).toMatchObject({ refusedBeforeWrite: true, wroteAnything: false });
    expect(task.failure()).toBeInstanceOf(Error);
    // Nothing is left behind: no record, no reservation, no timer.
    expect(sender.watching).toBe(0);
    expect(sender.facts().pendingBytes).toBe(0);
    expect(exploding.facts().writesStarted).toBe(0);
  });

  it('really refuses a task that does not fit its class bound, and keeps the reserved class working', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const admittedId = idFromBytes('str', new Uint8Array(16));
    const refusedId = idFromBytes(
      'str',
      Uint8Array.from({ length: 16 }, () => 0x5a),
    );
    // The bulk class holds ONE maximum Tool Draft file, so two 40 MiB streams cannot both be admitted -
    // and the second is refused as a CAPACITY fact, not as a verdict about the file.
    const forty = 40 * 1024 * 1024;
    try {
      // The normal path runs FIRST: one stream is admitted and really dispatched.
      f.sink.holdWriteAt(2);
      const admitted = Buffer.alloc(forty);
      const first = f.transport
        .sendBinaryStream(admittedId, admitted)
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.sink.starts).toEqual([1, 2]);
      const held = f.transport.requestStats().outbound;
      expect(held.pendingBytes).toBeGreaterThan(forty);
      expect(held.streamSourceBytes).toBe(forty);
      expect(held.pendingBytes).toBeLessThanOrEqual(held.maxPendingBytes);

      // A second stream of the same size cannot fit the class any more, so it is REFUSED while nothing
      // of it exists: no frame, no number, and the refusal is recorded as a transient capacity limit.
      const sequenceBefore = f.transport.nextOutboundSequence;
      const refusal = await f.transport.sendBinaryStream(refusedId, Buffer.alloc(forty)).then(
        () => undefined,
        (error: unknown) =>
          error as Error & { limitBytes?: number; sourceClass?: string; transient?: boolean },
      );
      expect(refusal).toBeInstanceOf(SendBudgetExceededError);
      expect(refusal?.sourceClass).toBe('stream');
      expect(refusal?.transient).toBe(true);
      expect(refusal?.message).toMatch(/retained source bytes/u);
      expect(refusal?.limitBytes).toBe(STREAM_SOURCE_BUDGET_BYTES);
      expect(f.transport.requestStats().outbound.refusalsOverBudget).toBe(1);
      expect(f.transport.nextOutboundSequence).toBe(sequenceBefore);
      expect(f.sink.binary.filter((chunk) => chunk.streamId === refusedId)).toHaveLength(0);
      // And an over-policy file never reaches here at all: the business policy refuses it before a
      // stream exists.

      // An ORDINARY request that cannot fit ITS class is refused the same way, and its own envelope
      // records which bound it reached instead of the generic send-failure reason. Two legal catalog-
      // sized requests fill the class first; their own answers never arrive, which is the point: the
      // SOURCE they hold is what the third request cannot fit behind.
      const five = 'z'.repeat(5 * 1024 * 1024);
      for (let index = 0; index < 2; index += 1) {
        void f.transport.request('agent.status', { blob: five }, 1, 5_000).catch(() => undefined);
      }
      await flush();
      expect(f.transport.requestStats().outbound.controlSourceBytes).toBeGreaterThan(
        2 * 5 * 1024 * 1024,
      );
      const refusedRequest = await f.transport
        .request('agent.status', { blob: five }, 1, 5_000)
        .catch((error: unknown) => error);
      expect(
        (refusedRequest as { envelope: { details: Record<string, unknown> } }).envelope.details,
      ).toMatchObject({
        not_sent_reason: 'send_budget_exceeded',
        outcome_recovery: OUTCOME_RECOVERY_NOT_SENT,
      });
      expect(f.transport.requestStats().outbound.refusalsOverBudget).toBe(2);
      expect(f.transport.nextOutboundSequence).toBe(sequenceBefore);

      // The reserved P0 class is NOT spendable by bulk work, so the cancel still goes out while every
      // other class is saturated.
      const cancel = f.transport.cancel(idFromBytes('cor', new Uint8Array(16)));
      f.sink.openHeld();
      await flush();
      await vi.advanceTimersByTimeAsync(0);
      await expect(cancel).resolves.toMatchObject({ disposition: 'accepted' });
      expect(f.sink.sent.map((message) => message.method)).toContain('aura.cancel');

      // The admitted stream settles normally and gives every byte back, and so do the two admitted
      // requests once their channel ends: an over-bound refusal left no task or timer behind.
      await expect(first).resolves.toBeUndefined();
      await vi.advanceTimersByTimeAsync(0);
      expect(f.transport.requestStats().outbound.pendingBytes).toBe(0);
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      // And the channel is still healthy enough to admit the SAME kind of legitimate object - which is
      // what makes the refusal above a transient capacity limit and not a permanent un-admittability.
      await expect(
        f.transport.sendBinaryStream(idFromBytes('str', new Uint8Array(16)), Buffer.alloc(forty)),
      ).resolves.toBeUndefined();
      expect(f.transport.requestStats().outbound.pendingBytes).toBe(0);
      expect(f.transport.requestStats().outbound.refusalsOverBudget).toBe(2);
      // The two admitted requests are still the REGISTRY's business (their frames are written and their
      // send tasks settled); the over-bound refusal left no task, timer or reservation of its own.
      expect(f.transport.requestStats().active).toBe(2);
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('hands the secret bytes from the caller to the send owner exactly once', () => {
    const caller = Buffer.from('synthetic-ownership-value');
    const ownership = new SecretInputOwnership(caller);
    expect(ownership.callerOwnsBytes).toBe(true);
    expect(ownership.takenOver).toBe(false);

    const sendCopy = ownership.takeOver();
    // One bounded copy, and the caller's buffer is empty from that moment on. Nothing here prints the
    // secret: length and ownership are the facts under test.
    expect(ownership.callerOwnsBytes).toBe(false);
    expect(ownership.takenOver).toBe(true);
    expect(caller.every((byte) => byte === 0)).toBe(true);
    expect(sendCopy.byteLength).toBe('synthetic-ownership-value'.length);
    // Taking over twice does NOT make a second copy: the sender keeps reading the one it owns.
    expect(ownership.takeOver()).toBe(sendCopy);

    ownership.release();
    expect(sendCopy.every((byte) => byte === 0)).toBe(true);
    expect(ownership.takenOver).toBe(false);
  });

  it('releases everything when a stream is cut off by a sink failure part-way through', async () => {
    const f = await fixture();
    const streamId = idFromBytes('str', new Uint8Array(16));
    try {
      // The NORMAL path runs first: chunk one really reaches the sink.
      const wroteBefore = f.transport.requestStats().outbound.wroteAnything;
      f.sink.failWriteAt(3);
      const result = await f.transport
        .sendBinaryStream(streamId, Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 3))
        .then(
          () => 'sent',
          (error: unknown) => error,
        );
      expect(result).not.toBe('sent');
      // This is a possibly-partial send, and it says so: the failure is a write abort, never a
      // "nothing of this was sent" refusal.
      expect(result).toBeInstanceOf(FrameWriteAbortedError);
      expect((result as Error).message).not.toMatch(/not_sent/u);
      expect(f.sink.binary.filter((chunk) => chunk.streamId === streamId)).toHaveLength(1);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outbound.pendingBytes).toBe(0);
      expect(f.transport.requestStats().outbound.wroteAnything).toBe(wroteBefore + 1);
      expect(f.transport.requestStats().outboundFrames.heldBytes).toBe(0);
    } finally {
      f.sink.openHeld();
      f.transport.close();
    }
  });

  it('returns every counter to its baseline after several streams and a close settle', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const streams = Array.from({ length: 3 }, () => idFromBytes('str', new Uint8Array(16)));
    try {
      f.sink.holdWriteAt(2);
      const pending = streams.map((streamId) =>
        f.transport.sendBinaryStream(streamId, Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 4)).then(
          () => 'sent' as const,
          (error: unknown) => error,
        ),
      );
      await vi.advanceTimersByTimeAsync(0);
      // The normal path ran first: one frame is really inside the sink and three tasks are alive.
      expect(f.sink.starts).toEqual([1, 2]);
      expect(f.transport.requestStats().outbound.watching).toBe(3);
      const peak = f.transport.requestStats().outbound.peakPendingBytes;
      expect(peak).toBeGreaterThan(0);
      expect(peak).toBeLessThanOrEqual(f.transport.requestStats().outbound.maxPendingBytes);

      // Close while the queue is full. Every waiter has to end and every byte has to go back.
      f.transport.close();
      f.sink.openHeld();
      await vi.advanceTimersByTimeAsync(0);
      await flush();
      const results = await Promise.all(pending);
      for (const result of results) expect(result).not.toBe('sent');

      const stats = f.transport.requestStats();
      expect(stats.outbound.watching).toBe(0);
      expect(stats.outbound.pendingBytes).toBe(0);
      expect(stats.outboundFrames.pendingBytes).toBe(0);
      expect(stats.outboundFrames.queuedBytes).toBe(0);
      expect(stats.outboundFrames.stagedBytes).toBe(0);
      expect(stats.outboundFrames.heldBytes).toBe(0);
      expect(stats.outboundFrames.heldFrames).toBe(0);
      expect(stats.outboundFrames.closed).toBe(true);
      expect(f.transport.requestStats().outbound.tasks).toBe(4);
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('refuses an unbuildable frame as not_sent and never as a partial write', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const before = f.transport.nextOutboundSequence;
      const writesBefore = f.sink.ordinals.length;
      // The payload is fine as JSON but cannot become a control frame: the encoder refuses it while
      // nothing of it exists. This is the "encoder refusal" branch, reached by a real request.
      const oversized = await f.transport
        .request('agent.status', { blob: 'x'.repeat(7 * 1024 * 1024) }, 1, 5_000)
        .catch((error: unknown) => error);
      expect(
        (oversized as { envelope: { details: Record<string, unknown> } }).envelope.details,
      ).toMatchObject({ outcome_recovery: OUTCOME_RECOVERY_NOT_SENT });
      expect((oversized as Error).message).toMatch(/6 MiB/u);
      expect(f.transport.nextOutboundSequence).toBe(before);
      expect(f.sink.ordinals.length).toBe(writesBefore);
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outbound.pendingBytes).toBe(0);
      expect(f.transport.requestStats().outbound.refusedBeforeWrite).toBe(1);

      // A P0 frame that could never fit its emergency bound is refused the same way, before any
      // number is spent, and the channel still works afterwards.
      const tooBigForP0 = await f.transport
        .request('agent.status', { blob: 'y'.repeat(40 * 1024) }, 0, 5_000)
        .catch((error: unknown) => error);
      expect(
        (tooBigForP0 as { envelope: { details: Record<string, unknown> } }).envelope.details,
      ).toMatchObject({ outcome_recovery: OUTCOME_RECOVERY_NOT_SENT });
      expect(f.transport.nextOutboundSequence).toBe(before);

      const healthy = f.transport.request('agent.status', {}, 1, 5_000);
      await flush();
      expect(f.sink.frames.at(-1)?.sequence).toBe(before);
      f.sink.deliver({
        jsonrpc: '2.0',
        id: f.sink.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await expect(healthy).resolves.toEqual({ state: 'completed' });
    } finally {
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });
});
