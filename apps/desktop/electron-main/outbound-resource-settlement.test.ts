import { Buffer } from 'node:buffer';

import { afterEach, describe, expect, it, vi } from 'vitest';

import { FrameScheduler } from './frame-scheduler.js';
import { idFromBytes } from './ids.js';
import {
  controlFrameSize,
  createTaskKey,
  OutboundSendOwner,
  SecretInputOwnership,
} from './outbound-send-owner.js';
import { MAX_BINARY_CHUNK_BYTES } from './protocol.js';

const binding = { channelId: idFromBytes('chn', new Uint8Array(16)), connectionEpoch: 1n };

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

function makeOwner(scheduler: FrameScheduler, physicalSendBudgetMs = 120_000): OutboundSendOwner {
  return new OutboundSendOwner(
    binding,
    scheduler,
    {
      onWriteStarted: () => undefined,
      onSettled: () => undefined,
      onUnrecoverable: () => undefined,
    },
    { physicalSendBudgetMs },
  );
}

function ownerWithRetirement(
  scheduler: FrameScheduler,
  physicalSendBudgetMs: number,
): { retired: Error[]; sender: OutboundSendOwner } {
  const retired: Error[] = [];
  const sender = new OutboundSendOwner(
    binding,
    scheduler,
    {
      onSettled: () => undefined,
      onUnrecoverable: (error) => retired.push(error),
      onWriteStarted: () => undefined,
    },
    { physicalSendBudgetMs },
  );
  return { retired, sender };
}

function requestSpec() {
  return {
    commandInvocationId: undefined,
    correlationId: idFromBytes('cor', new Uint8Array(16)),
    framePayload: Buffer.from('{}'),
    priority: 1 as const,
  };
}

describe('outbound retained-resource settlement', () => {
  it('removes an unsent group at queue expiry while a healthy preceding write continues', async () => {
    vi.useFakeTimers();
    const scheduler = new FrameScheduler({
      write: async () => new Promise<void>((resolve) => setTimeout(resolve, 70)),
    });
    const sender = makeOwner(scheduler, 100);
    const first = sender.register(requestSpec(), createTaskKey('first'));
    const second = sender.register(requestSpec(), createTaskKey('second'));
    const third = sender.register(requestSpec(), createTaskKey('queue-expiry'));
    try {
      await vi.advanceTimersByTimeAsync(101);
      expect(third.state()).toBe('withdrawn');
      expect(third.outcome()).toMatchObject({ withdrewBeforeWrite: true, wroteAnything: false });
      expect(second.state()).toBe('writing');
      expect(scheduler.facts()).toMatchObject({ queuedGroups: 0, pendingGroups: 0 });
      expect(sender.watching).toBe(1);
      expect(sender.facts().pendingBytes).toBeGreaterThan(0);
    } finally {
      await vi.advanceTimersByTimeAsync(110);
      await Promise.all([first.done, second.done, third.done]);
    }
    expect(sender.facts().pendingBytes).toBe(0);
    expect(scheduler.facts().heldBytes).toBe(0);
    expect(scheduler.facts().writesStarted).toBe(2);
  });

  it('reserves the send-owned secret copy and the complete frame held by the sink', async () => {
    let release!: () => void;
    let secondStarted!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const ready = new Promise<void>((resolve) => {
      secondStarted = resolve;
    });
    let calls = 0;
    let heldFrameBytes = 0;
    const scheduler = new FrameScheduler({
      write: async (bytes) => {
        calls += 1;
        if (calls === 2) {
          heldFrameBytes = bytes.byteLength;
          secondStarted();
          await gate;
        }
      },
    });
    const sender = makeOwner(scheduler);
    const spec = requestSpec();
    const secretLength = 2_560;
    const ownership = new SecretInputOwnership(Buffer.alloc(secretLength, 0x61));
    const task = sender.register(
      {
        ...spec,
        commandInvocationId: idFromBytes('civ', new Uint8Array(16)),
        secret: { operation: 'models_endpoint_create_with_credential', ownership },
      },
      createTaskKey('secret-retention'),
    );
    try {
      await ready;
      expect(ownership.takenOver).toBe(true);
      const retainedBytes = spec.framePayload.byteLength + secretLength + heldFrameBytes;
      expect(sender.facts().pendingBytes).toBeGreaterThanOrEqual(retainedBytes);
    } finally {
      release();
      await task.done;
    }
    expect(ownership.takenOver).toBe(false);
    expect(sender.facts().pendingBytes).toBe(0);
    expect(sender.watching).toBe(0);
  });

  it('keeps the native write copy reserved until the REAL write promise settles, exactly once', async () => {
    vi.useFakeTimers();
    let release!: () => void;
    const inSink = new Promise<void>((resolve) => {
      release = resolve;
    });
    // The sink behaves like the addon: it takes the bytes and stays inside the write until it is done.
    const scheduler = new FrameScheduler({
      write: async () => {
        await inSink;
      },
    });
    const { retired, sender } = ownerWithRetirement(scheduler, 100);
    const payload = Buffer.alloc(64 * 1024, 0x41);
    const frameBytes = controlFrameSize(payload.byteLength);
    const task = sender.register(
      { ...requestSpec(), framePayload: payload },
      createTaskKey('stalled-native-write'),
    );
    try {
      await vi.advanceTimersByTimeAsync(0);
      // Every holder of these bytes is in ONE figure while the write is outstanding: the source this
      // task retains, the frame the scheduler built, and the copy the native side made of it.
      const held = sender.facts();
      expect(held.sourceBytes).toBe(payload.byteLength);
      expect(held.frameBytes).toBe(frameBytes);
      expect(held.nativeBytes).toBe(frameBytes);
      expect(held.pendingBytes).toBe(payload.byteLength + 2 * frameBytes);
      expect(held.pendingBytes).toBeLessThanOrEqual(held.maxPendingBytes);
      expect(held.nativeBytes).toBeLessThanOrEqual(held.maxFrameWorkingBytes);

      // The physical deadline fires while the write is outstanding. The task really ends and the
      // connection is retired - but the native copy is still being read, so it may NOT be reported as
      // released, and nothing more may be admitted on the strength of it.
      await vi.advanceTimersByTimeAsync(101);
      expect(task.settled()).toBe(true);
      expect(task.outcome()).toMatchObject({
        timedOutPhysically: true,
        withdrewBeforeWrite: false,
        wroteAnything: true,
      });
      expect(retired).toHaveLength(1);
      const stuck = sender.facts();
      expect(stuck.sourceBytes).toBe(payload.byteLength);
      expect(stuck.frameBytes).toBe(frameBytes);
      expect(stuck.nativeBytes).toBe(frameBytes);
      expect(stuck.pendingBytes).toBe(payload.byteLength + 2 * frameBytes);
      expect(stuck.watching).toBe(0);

      // The real write promise now settles. That is the ONLY event that gives the working bytes back.
      release();
      await vi.advanceTimersByTimeAsync(1);
      const after = sender.facts();
      expect(after.frameBytes).toBe(0);
      expect(after.nativeBytes).toBe(0);
      expect(after.pendingBytes).toBe(0);
      expect(after.peakNativeBytes).toBe(frameBytes);
      expect(after.peakPendingBytes).toBe(payload.byteLength + 2 * frameBytes);

      // A late duplicate settle (a deadline firing again, a retirement the sink never saw) must not
      // drive any counter negative or resurrect the task.
      await vi.advanceTimersByTimeAsync(500);
      expect(sender.facts().nativeBytes).toBe(0);
      expect(sender.facts().frameBytes).toBe(0);
      expect(sender.facts().pendingBytes).toBe(0);
      expect(retired).toHaveLength(1);
    } finally {
      release();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('settles every one of many waiting requests instead of letting the queue admit only a few', async () => {
    // The reproduced defect this guards: counting requests that are still WAITING against the queue's
    // committed-frame allowance made a burst of requests emit a small prefix and then stop dead. The
    // pending list has its own accounting, and this is what proves it still drains completely.
    const count = 200;
    const written: Buffer[] = [];
    const scheduler = new FrameScheduler({
      write: async (bytes: Buffer) => {
        written.push(bytes);
      },
    });
    const sender = makeOwner(scheduler);
    const tasks = Array.from({ length: count }, (_value, index) =>
      sender.register(requestSpec(), createTaskKey(`burst-${String(index)}`)),
    );
    await Promise.all(tasks.map((task) => task.done));

    // Every request was admitted, numbered and written - not a prefix of them.
    expect(written).toHaveLength(count);
    expect(scheduler.facts().writesStarted).toBe(count);
    expect(scheduler.facts().writesSucceeded).toBe(count);
    expect(new Set(written.map((bytes) => bytes.readBigUInt64LE(29))).size).toBe(count);
    expect(sender.facts().watching).toBe(0);
    expect(sender.facts().pendingBytes).toBe(0);
    expect(sender.facts().controlSourceBytes).toBe(0);
    expect(sender.facts().refusalsOverBudget).toBe(0);
    expect(scheduler.facts().heldBytes).toBe(0);
    // The peaks really did exceed what one scheduling turn can hold, so the burst was not a trickle.
    expect(scheduler.facts().peakPendingGroups).toBeGreaterThan(1);
    expect(scheduler.facts().peakCommittedBytes).toBeLessThanOrEqual(16 * 1024 * 1024);
  });

  it('keeps a failed stream source reserved until its outstanding physical write finishes', async () => {
    vi.useFakeTimers();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const scheduler = new FrameScheduler({ write: async () => gate });
    const sender = new OutboundSendOwner(
      binding,
      scheduler,
      {
        onSettled: () => undefined,
        onUnrecoverable: (error) => scheduler.close(error),
        onWriteStarted: () => undefined,
      },
      { physicalSendBudgetMs: 100 },
    );
    const sourceBytes = 4 * MAX_BINARY_CHUNK_BYTES;
    const result = sender
      .sendBinaryStream(
        idFromBytes('str', new Uint8Array(16).fill(9)),
        Buffer.alloc(sourceBytes, 0x41),
        createTaskKey('stalled-stream-source'),
      )
      .then(
        () => undefined,
        (error: unknown) => error,
      );
    try {
      await vi.advanceTimersByTimeAsync(101);
      expect(await result).toBeInstanceOf(Error);
      expect(sender.facts()).toMatchObject({
        sourceBytes,
        streamSourceBytes: sourceBytes,
        watching: 0,
      });
      expect(scheduler.facts().inFlightFrames).toBe(1);
      release();
      await vi.advanceTimersByTimeAsync(0);
      expect(sender.facts()).toMatchObject({ frameBytes: 0, nativeBytes: 0, sourceBytes: 0 });
      expect(scheduler.facts().writesStarted).toBe(1);
    } finally {
      release();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('public release cannot return source capacity while a frame write still owns its builder', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const scheduler = new FrameScheduler({ write: async () => gate });
    const sender = makeOwner(scheduler);
    const task = sender.register(
      { ...requestSpec(), framePayload: Buffer.alloc(1024 * 1024, 0x20) },
      createTaskKey('release-still-in-flight'),
    );
    try {
      task.release();
      expect(sender.facts().controlSourceBytes).toBe(1024 * 1024);
      expect(task.settled()).toBe(false);
      expect(scheduler.facts().inFlightFrames).toBe(1);
    } finally {
      release();
      await task.done;
    }
    expect(sender.facts().pendingBytes).toBe(0);
  });

  it('stops feeding a partially sent stream at a complete-frame boundary without claiming a withdrawal', async () => {
    vi.useFakeTimers();
    const budget = 100;
    // Every write is ONE COMPLETE FRAME that finishes well inside its own budget, so the stream that is
    // stopped here is stopped at a real frame boundary and never in the middle of a frame.
    const scheduler = new FrameScheduler({
      write: async () => {
        await new Promise((resolve) => setTimeout(resolve, 70));
      },
    });
    const sender = makeOwner(scheduler, budget);
    const partial = Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 2, 0x33);
    const bulk = sender
      .sendBinaryStream(
        idFromBytes('str', new Uint8Array(16).fill(8)),
        partial,
        createTaskKey('partial-stream'),
      )
      .then(
        () => 'sent',
        (error: unknown) => error,
      );
    // One P0 atomic binding holds the scheduling turn for two complete frames (140 ms), which is longer
    // than the stream's own queue deadline - and it is a legitimate task in its own right.
    const binding = sender.register(
      {
        ...requestSpec(),
        commandInvocationId: idFromBytes('civ', new Uint8Array(16)),
        priority: 0,
        secret: {
          operation: 'models_endpoint_configure_credential',
          ownership: new SecretInputOwnership(Buffer.alloc(32, 0x61)),
        },
      },
      createTaskKey('p0-atomic-binding'),
    );
    try {
      // Chunk one goes out immediately, and chunk two is staged behind it.
      await vi.advanceTimersByTimeAsync(1);
      expect(scheduler.facts().writesStarted).toBe(1);
      expect(sender.facts().streamSourceBytes).toBe(partial.byteLength);

      // Chunk one is complete, the cursor has staged chunk two, and the P0 binding now holds the turn.
      await vi.advanceTimersByTimeAsync(75);
      expect(scheduler.facts().writesStarted).toBe(2);
      expect(sender.facts().streamSourceBytes).toBe(partial.byteLength);

      // The staged continuation misses its OWN queue deadline while the binding is mid-write: the
      // deadline is counted from the moment the frame became resident, not from the task's start.
      await vi.advanceTimersByTimeAsync(budget + 5);
      expect(scheduler.facts().inFlightFrames).toBe(1);
      // Its source AND its held frame declaration are both gone: the scheduler is not left holding a
      // view over a buffer the owner has already given back.
      expect(sender.facts().streamSourceBytes).toBe(0);
      expect(scheduler.facts().stagedBytes + scheduler.facts().queuedBytes).toBe(0);
      expect(scheduler.facts().closed).toBe(false);

      // The healthy write in front of it really continues, and the stream's caller hears the honest
      // "not every chunk went out" rejection rather than a silent success.
      await vi.advanceTimersByTimeAsync(1_000);
      expect(scheduler.facts().writesStarted).toBe(3);
      expect(await bulk).toBeInstanceOf(Error);
      await binding.done;

      const settled = sender.facts();
      // A stream that really wrote bytes may never be counted as "withdrawn before any write".
      expect(settled.withdrewBeforeWrite).toBe(0);
      expect(settled.watching).toBe(0);
      expect(settled.pendingBytes).toBe(0);
      expect(settled.sourceBytes).toBe(0);
      expect(settled.nativeBytes).toBe(0);
      expect(scheduler.facts().heldBytes).toBe(0);
      expect(scheduler.facts().closed).toBe(false);
    } finally {
      await vi.advanceTimersByTimeAsync(1_000);
      await binding.done;
      await bulk;
    }
  });
});
