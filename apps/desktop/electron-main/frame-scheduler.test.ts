import { Buffer } from 'node:buffer';

import { describe, expect, it } from 'vitest';

import {
  FRAME_SCHEDULER_LIMITS,
  FrameBuildError,
  FrameScheduler,
  FrameWithdrawnError,
  MAX_FRAMES_PER_GROUP,
  type ScheduledFrame,
} from './frame-scheduler.js';
import { idFromBytes } from './ids.js';
import { encodeSecretInputFrame, MAX_CONTROL_PAYLOAD_BYTES } from './protocol.js';

const binding = { channelId: idFromBytes('chn', new Uint8Array(16)), connectionEpoch: 1n };

const frameOf = (bytes: Buffer): ScheduledFrame => ({
  build: () => bytes,
  sizeBytes: bytes.byteLength,
});

const text = (value: string): ScheduledFrame => {
  const bytes = Buffer.from(value, 'utf8');
  return frameOf(bytes);
};

describe('atomic groups and stream cursors are different structures', () => {
  it("keeps an atomic group's frames adjacent when a P0 control frame is already queued", async () => {
    const order: string[] = [];
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const scheduler = new FrameScheduler({
      write: async (bytes) => {
        const value = bytes.toString();
        order.push(value);
        if (value === 'command') await gate;
      },
    });

    const atomic = scheduler.enqueue({
      frames: [text('command'), text('secret')],
      kind: 'atomic',
      label: 'secret-binding',
      priority: 0,
    });
    const control = scheduler.enqueue({
      first: text('cancel'),
      kind: 'stream',
      label: 'control',
      next: () => undefined,
      priority: 0,
    });
    release();
    await Promise.all([atomic.done, control.done]);
    // The binding was already dispatched when the cancellation queued, and a started binding owes its
    // remaining frames: the control frame uses the boundary AFTER the binding, never a gap inside it.
    expect(order).toEqual(['command', 'secret', 'cancel']);
  });

  it('still refuses an atomic group larger than the two-frame binding', () => {
    const scheduler = new FrameScheduler({ write: async () => undefined });
    expect(() =>
      scheduler.enqueue({
        frames: [text('a'), text('b'), text('c')],
        kind: 'atomic',
        label: 'too-wide',
        priority: 0,
      }),
    ).toThrow(RangeError);
    expect(MAX_FRAMES_PER_GROUP).toBe(2);
    expect(scheduler.facts().writesStarted).toBe(0);
  });

  it('lets a control frame take the complete-frame boundary between two stream chunks', async () => {
    const order: string[] = [];
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const chunks = ['chunk-0', 'chunk-1', 'chunk-2'];
    let cursor = 1;
    const scheduler = new FrameScheduler({
      write: async (bytes) => {
        const value = bytes.toString();
        order.push(value);
        if (value === 'chunk-0') await gate;
      },
    });

    // THREE chunks: more than the atomic two-frame ceiling, which must not bound a stream at all.
    const stream = scheduler.enqueue({
      first: text(chunks[0] as string),
      kind: 'stream',
      label: 'bulk',
      next: () => (cursor < chunks.length ? text(chunks[cursor++] as string) : undefined),
      priority: 1,
      wanted: () => true,
    });
    const control = scheduler.enqueue({
      first: text('cancel'),
      kind: 'stream',
      label: 'control',
      next: () => undefined,
      priority: 0,
    });
    release();
    await Promise.all([stream.done, control.done]);
    // bulk -> control -> bulk -> bulk: the cancellation was inserted at a boundary between two
    // COMPLETE chunks. It did not replace a chunk, and the stream still sent all three.
    expect(order).toEqual(['chunk-0', 'cancel', 'chunk-1', 'chunk-2']);
  });

  it('zeroes a built SecretInput frame once the sink has settled it', async () => {
    let written: Buffer | undefined;
    let kind = -1;
    const scheduler = new FrameScheduler({
      write: async (bytes) => {
        written = bytes;
        kind = bytes[4] ?? -1;
      },
    });
    const secretBytes = Buffer.byteLength('fixture-value');
    /** The exact SecretInput frame size, spelled out here so this case does not depend on the owner. */
    const secretInputFrameSize = (bytes: number): number => 1 + 32 + 40 + bytes + 4;
    const handle = scheduler.enqueue({
      frames: [
        {
          build: () =>
            encodeSecretInputFrame({
              binding,
              commandInvocationId: idFromBytes('civ', new Uint8Array(16)),
              correlationId: idFromBytes('cor', new Uint8Array(16)),
              operation: 'models_endpoint_configure_credential',
              secret: Buffer.from('fixture-value', 'utf8'),
              sequence: 2n,
            }),
          release: (bytes) => bytes.fill(0),
          sizeBytes: secretInputFrameSize(secretBytes),
        },
      ],
      kind: 'atomic',
      label: 'secret-frame',
      priority: 1,
    });
    await handle.done;

    expect(kind).toBe(3);
    expect(written?.every((byte) => byte === 0)).toBe(true);
  });

  it('rejects an unbuildable frame without spending a number or closing the scheduler', async () => {
    const wrote: number[] = [];
    const scheduler = new FrameScheduler({
      write: async (bytes) => {
        wrote.push(bytes.byteLength);
      },
    });
    const started = scheduler.nextSequence;
    const refused = scheduler
      .enqueue({
        frames: [
          {
            build: () => {
              throw new RangeError('payload exceeds the 6 MiB protocol limit');
            },
            sizeBytes: MAX_CONTROL_PAYLOAD_BYTES + 1,
          },
        ],
        kind: 'atomic',
        label: 'oversized',
        priority: 1,
      })
      .done.catch((error: unknown) => error);
    expect(await refused).toBeInstanceOf(FrameBuildError);
    expect(scheduler.nextSequence).toBe(started);
    expect(scheduler.facts().closed).toBe(false);
    expect(scheduler.facts().writesFailed).toBe(0);

    // ...and the scheduler is still usable: the next frame carries the next number.
    const healthy = scheduler.enqueue({
      frames: [text('ok')],
      kind: 'atomic',
      label: 'healthy',
      priority: 1,
    });
    await healthy.done;
    expect(wrote).toEqual([2]);
    expect(scheduler.nextSequence).toBe(started + 1n);
  });

  it('never commits a number to a frame its owner took back at the boundary', async () => {
    const scheduler = new FrameScheduler({ write: async () => undefined });
    const before = scheduler.nextSequence;
    const withdrawn = scheduler
      .enqueue({
        frames: [text('gone')],
        kind: 'atomic',
        label: 'withdrawn',
        priority: 1,
        wanted: () => false,
      })
      .done.catch((error: unknown) => error);
    expect(await withdrawn).toBeInstanceOf(FrameWithdrawnError);
    expect(scheduler.nextSequence).toBe(before);
    expect(scheduler.facts().withdrawnGroups).toBe(1);
  });

  it('reserves room for one maximum P1 frame under bulk load', async () => {
    let releaseFirstWrite: (() => void) | undefined;
    const firstWrite = new Promise<void>((resolve) => {
      releaseFirstWrite = resolve;
    });
    const writes: number[] = [];
    const scheduler = new FrameScheduler({
      write: async (bytes) => {
        writes.push(bytes.byteLength);
        if (writes.length === 1) await firstWrite;
      },
    });
    const sized = (size: number): ScheduledFrame => ({
      build: () => Buffer.alloc(size),
      sizeBytes: size,
    });

    const blocker = scheduler.enqueue({
      frames: [sized(1)],
      kind: 'atomic',
      label: 'blocker',
      priority: 1,
    });
    const bulkA = scheduler.enqueue({
      frames: [sized(5 * 1024 * 1024)],
      kind: 'atomic',
      label: 'bulkA',
      priority: 2,
    });
    const bulkB = scheduler.enqueue({
      frames: [sized(4.75 * 1024 * 1024)],
      kind: 'atomic',
      label: 'bulkB',
      priority: 2,
    });
    const maximumP1FrameBytes = MAX_CONTROL_PAYLOAD_BYTES + 4 + 1 + 32;
    const catalog = scheduler.enqueue({
      frames: [sized(maximumP1FrameBytes)],
      kind: 'atomic',
      label: 'catalog',
      priority: 1,
    });

    releaseFirstWrite?.();
    await catalog.done;
    // The reserved P1 bytes are not spendable by the bulk classes, so the catalog frame is written
    // whole, straight after the frame that was already in progress.
    expect(writes[1]).toBe(maximumP1FrameBytes);
    await Promise.all([blocker.done, bulkA.done, bulkB.done]);
    expect(scheduler.facts().writesFailed).toBe(0);
  });

  it('accounts for every bucket it really holds, not only the queue', async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const chunk = 1024;
    const chunks = 3;
    let writes = 0;
    let cursor = 1;
    const scheduler = new FrameScheduler({
      write: async () => {
        writes += 1;
        if (writes === 1) await gate;
      },
    });
    const sized = (size: number): ScheduledFrame => ({
      build: () => Buffer.alloc(size),
      sizeBytes: size,
    });

    const stream = scheduler.enqueue({
      first: sized(chunk),
      kind: 'stream',
      label: 'bulk',
      next: () => (cursor < chunks ? (cursor++, sized(chunk)) : undefined),
      priority: 1,
      wanted: () => true,
    });
    // Two more tasks that cannot claim a place while the wire is busy: they wait in the pending list.
    const pendingA = scheduler.enqueue({
      frames: [sized(chunk)],
      kind: 'atomic',
      label: 'pending-a',
      priority: 1,
    });
    const pendingB = scheduler.enqueue({
      frames: [sized(chunk)],
      kind: 'atomic',
      label: 'pending-b',
      priority: 1,
    });

    const busy = scheduler.facts();
    expect(busy.queuedFrames).toBe(0);
    expect(busy.pendingFrames).toBe(2);
    expect(busy.inFlightBytes).toBe(chunk);
    expect(busy.heldBytes).toBe(chunk + 2 * chunk);

    release();
    for (let turn = 0; turn < 16; turn += 1) await Promise.resolve();
    await Promise.all([stream.done, pendingA.done, pendingB.done]);

    const facts = scheduler.facts();
    // Five FRAMES were really written (three chunks plus two single-frame groups); `writesSucceeded`
    // counts resolved GROUPS, which is a different number on purpose.
    expect(facts.writesStarted).toBe(chunks + 2);
    expect(facts.writesSucceeded).toBe(3);
    // Every bucket is reported separately, and the sum is what the scheduler really held: at the peak
    // it was the frame inside the sink plus the frames waiting for it, not only the queue.
    expect(facts.pendingBytes).toBe(0);
    expect(facts.queuedBytes).toBe(0);
    expect(facts.stagedBytes).toBe(0);
    expect(facts.heldBytes).toBe(0);
    expect(facts.heldFrames).toBe(0);
    expect(facts.peakHeldBytes).toBeGreaterThanOrEqual(3 * chunk);
    expect(facts.peakHeldBytes).toBeLessThanOrEqual(FRAME_SCHEDULER_LIMITS.sharedBytes);
    expect(facts.peakQueuedBytes).toBeLessThanOrEqual(FRAME_SCHEDULER_LIMITS.sharedBytes);
  });
});
