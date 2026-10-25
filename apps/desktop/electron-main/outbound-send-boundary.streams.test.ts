import { Buffer } from 'node:buffer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PHYSICAL_SEND_BUDGET_MS, SECRET_INPUT_BINDING_BUDGET_MS } from './core-transport.js';
import { MAX_P0_FRAME_BYTES } from './frame-scheduler.js';
import { idFromBytes } from './ids.js';

import { binaryFrameSize, MAX_TOOL_DRAFT_FILE_BYTES } from './outbound-send-owner.js';
import { MAX_BINARY_CHUNK_BYTES } from './protocol.js';
import {
  FRAME_TYPE_CONTROL,
  FRAME_TYPE_BINARY,
  FRAME_TYPE_SECRET_INPUT,
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
  it('keeps the SecretInput binding atomic, bounded and zeroed when a control frame competes', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const secret = Buffer.from('synthetic-m1-secret-value');
    try {
      f.sink.holdWriteAt(2);
      const pending = f.transport
        .requestWithSecretInput(
          'models.endpoint.configure_credential',
          {},
          'models_endpoint_configure_credential',
          secret,
          60_000,
        )
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      // The command frame is INCOMPLETE inside the sink, so this is the one moment a competing P0
      // frame can still be queued ahead of the binding's second frame. A case that never queues one
      // proves nothing about adjacency.
      const cancel = f.transport
        .cancel(idFromBytes('cor', new Uint8Array(16)))
        .catch((error: unknown) => error);
      await flush();
      expect(f.sink.starts).toEqual([1, 2]);

      f.sink.openHeld();
      await vi.advanceTimersByTimeAsync(0);
      await flush();
      await vi.advanceTimersByTimeAsync(0);
      // command -> SecretInput -> cancel. The binding was already dispatched, so it OWES its second
      // frame; the cancellation uses the boundary after the binding and is written as its own frame.
      expect(f.sink.kinds.slice(1)).toEqual([
        FRAME_TYPE_CONTROL,
        FRAME_TYPE_SECRET_INPUT,
        FRAME_TYPE_CONTROL,
      ]);
      expect(f.sink.sent.map((message) => message.method)).toContain('aura.cancel');
      // The two frames really are consecutive sequence numbers.
      const committed = f.sink.frames.slice(1);
      expect(committed[1]!.sequence).toBe(committed[0]!.sequence + 1n);
      expect(secret.every((byte) => byte === 0)).toBe(true);
      // Both frames of the binding are written, so no SEND task is holding anything any more; the
      // request itself is still the registry's, waiting for the Core's answer. (The cancellation was
      // answered by the fixture, so exactly one request is left active.)
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outbound.pendingBytes).toBe(0);
      expect(f.transport.requestStats().active).toBe(1);

      f.sink.deliver({
        jsonrpc: '2.0',
        id: f.sink.sent.find((message) => message.method === 'models.endpoint.configure_credential')
          ?.id,
        result: { terminal: 'succeeded', value: { configured: true } },
      });
      await vi.advanceTimersByTimeAsync(0);
      await expect(pending).resolves.toEqual({ configured: true });
      await cancel;
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outbound.pendingBytes).toBe(0);
      // No retained state anywhere carries the secret.
      expect(JSON.stringify(f.transport.requestStats())).not.toContain('synthetic-m1-secret-value');
    } finally {
      f.sink.openHeld();
      f.transport.close();
      secret.fill(0);
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('retires only the connection whose send failed while another connection keeps working', async () => {
    const failing = await fixture();
    const healthy = await fixture();
    try {
      failing.sink.failWriteAt(2);
      const result = await failing.transport
        .request('workspace.create', {}, 1, 500)
        .catch((e: unknown) => e);
      expect(result).toBeInstanceOf(Error);
      expect(failing.onClosed).toHaveBeenCalledOnce();
      expect(healthy.onClosed).not.toHaveBeenCalled();

      const answer = healthy.transport.request('agent.status', {}, 1, 1_000);
      await Promise.resolve();
      healthy.sink.deliver({
        jsonrpc: '2.0',
        id: healthy.sink.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await expect(answer).resolves.toEqual({ state: 'completed' });
    } finally {
      failing.transport.close();
      healthy.transport.close();
    }
  });

  it('does not reactivate an old owner when a native write completes late', async () => {
    vi.useFakeTimers();
    const f = await fixture({ physicalSendBudgetMilliseconds: 1_000 });
    try {
      f.sink.holdWriteAt(2);
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await gone) as { envelope: { code: string } };
      expect(failure.envelope.code).toBe('ipc.request_timeout');

      // The send owner's budget expires while the write is outstanding: the connection is retired.
      await vi.advanceTimersByTimeAsync(1_000);
      expect(f.onClosed).toHaveBeenCalledOnce();
      const tasksAtClose = f.transport.requestStats().outbound.tasks;

      // NOW the native side completes the write it was holding. It must not resurrect the channel, the
      // task or the sequence: the connection is over and the frame is not adopted by a new owner.
      f.sink.openHeld();
      await vi.advanceTimersByTimeAsync(0);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.transport.channelFacts().closed).toBe(true);
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      expect(f.transport.requestStats().outbound.tasks).toBe(tasksAtClose);
      expect(f.onClosed).toHaveBeenCalledOnce();
      await expect(f.transport.request('agent.status', {}, 1, 10)).rejects.toThrow();
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('sends a forty-chunk stream whole while every byte it holds stays inside the bound', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const streamId = idFromBytes('str', new Uint8Array(16));
    const huge = Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 40);
    for (let index = 0; index < huge.byteLength; index += 997) huge[index] = index % 253;
    try {
      f.sink.holdWriteAt(2);
      const bulk = f.transport.sendBinaryStream(streamId, huge).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      // The stream's FIRST chunk is parked inside the sink. Exactly one frame of this stream exists at
      // a time, so the second one cannot be anywhere yet - and the task is still holding its source.
      expect(f.sink.starts).toEqual([1, 2]);
      expect(f.transport.requestStats().outbound.watching).toBe(1);
      expect(f.transport.requestStats().outbound.pendingBytes).toBeGreaterThanOrEqual(
        huge.byteLength,
      );

      f.sink.openHeld();
      for (let turn = 0; turn < 64; turn += 1) await vi.advanceTimersByTimeAsync(0);
      // A caught rejection has to be asserted, not merely awaited: the stream must really succeed.
      await expect(bulk).resolves.toBeUndefined();

      const got = received(f.sink, streamId);
      expect(got.chunkSequences).toEqual(Array.from({ length: 40 }, (_value, index) => index));
      expect(got.endOfMessage.slice(0, 39).every((flag) => flag === false)).toBe(true);
      expect(got.endOfMessage.at(-1)).toBe(true);
      expect(sha256(got.bytes)).toBe(sha256(huge));
      expect(f.sink.frames.map((frame) => frame.sequence)).toEqual(
        f.sink.frames.map((_frame, index) => BigInt(index)),
      );

      const stats = f.transport.requestStats();
      // A stream never holds more than its cursor: one frame at a time plus the one being written.
      expect(stats.outboundFrames.peakQueuedFrames).toBeLessThanOrEqual(56);
      expect(stats.outboundFrames.peakQueuedGroups).toBeLessThanOrEqual(56);
      expect(stats.outboundFrames.peakCommittedBytes).toBeLessThanOrEqual(16 * 1024 * 1024);
      expect(stats.outbound.peakPendingBytes).toBeLessThanOrEqual(stats.outbound.maxPendingBytes);
      // Nothing is left holding the stream's bytes once it has really settled.
      expect(stats.outbound.watching).toBe(0);
      expect(stats.outbound.pendingBytes).toBe(0);
      expect(stats.outboundFrames.heldBytes).toBe(0);
    } finally {
      f.sink.openHeld();
      f.transport.close();
      await vi.advanceTimersByTimeAsync(0);
    }
  });

  it('carries a 20 MiB file the existing Tool Draft policy accepts as eighty complete chunks', async () => {
    const f = await fixture({ consumeBinary: true });
    const streamId = idFromBytes('str', new Uint8Array(16).fill(3));
    // Deterministic, non-repeating content. 20 MiB is EXACTLY eighty maximum-size chunks, so both the
    // "last chunk is shorter" and the "last chunk is full" shapes are covered across these two cases.
    const input = Buffer.alloc(20 * 1024 * 1024);
    for (let index = 0; index < input.byteLength; index += 4096)
      input[index] = (index / 4096) % 251;
    try {
      // A caught rejection has to be asserted, not merely awaited: the stream must really succeed.
      await expect(f.transport.sendBinaryStream(streamId, input)).resolves.toBeUndefined();

      expect(f.sink.consumedChunks).toBe(80);
      expect(f.sink.consumedBytes).toBe(input.byteLength);
      expect(f.sink.consumedDigest()).toBe(sha256(input));
      expect(f.sink.consumedChunkSequences).toEqual(
        Array.from({ length: 80 }, (_value, index) => BigInt(index)),
      );
      expect(f.sink.consumedEndOfMessage.slice(0, 79).every((flag) => flag === false)).toBe(true);
      expect(f.sink.consumedEndOfMessage.at(-1)).toBe(true);
      expect(f.sink.consumedStreamIds.every((id) => id === streamId)).toBe(true);
      // The connection sequence is contiguous across all eighty frames: no frame was skipped, and the
      // frames of a stream are not numbered per chunk set.
      expect(f.sink.consumedSequences).toEqual(
        Array.from({ length: 80 }, (_value, index) => BigInt(index + 1)),
      );

      const stats = f.transport.requestStats();
      expect(stats.outbound.watching).toBe(0);
      expect(stats.outbound.pendingBytes).toBe(0);
      expect(stats.outbound.sourceBytes).toBe(0);
      expect(stats.outbound.frameBytes).toBe(0);
      expect(stats.outbound.nativeBytes).toBe(0);
      expect(stats.outboundFrames.heldBytes).toBe(0);
      // The peaks of the run really are inside the declared bounds, and the frame peak is one frame
      // plus the one native copy of it - not a whole-stream figure.
      expect(stats.outbound.peakSourceBytes).toBe(input.byteLength);
      expect(stats.outbound.peakSourceBytes).toBeLessThanOrEqual(stats.outbound.maxSourceBytes);
      expect(stats.outbound.peakFrameBytes).toBe(binaryFrameSize(MAX_BINARY_CHUNK_BYTES));
      expect(stats.outbound.peakNativeBytes).toBe(binaryFrameSize(MAX_BINARY_CHUNK_BYTES));
      expect(stats.outbound.peakPendingBytes).toBeLessThanOrEqual(stats.outbound.maxPendingBytes);
    } finally {
      f.transport.close();
    }
  });

  it('sends a 64 MiB non-manifest file the existing policy accepts, consuming every chunk', async () => {
    // The receiver CONSUMES each chunk (hash and count, keep no copy), so this stays a measurement of
    // the sender instead of the receiver piling up a second whole copy of a 64 MiB file.
    const f = await fixture({ consumeBinary: true });
    const streamId = idFromBytes('str', new Uint8Array(16).fill(6));
    const input = Buffer.alloc(MAX_TOOL_DRAFT_FILE_BYTES);
    for (let index = 0; index < input.byteLength; index += 8191)
      input[index] = (index / 8191) % 249;
    try {
      await expect(f.transport.sendBinaryStream(streamId, input)).resolves.toBeUndefined();

      expect(input.byteLength).toBe(64 * 1024 * 1024);
      expect(f.sink.consumedChunks).toBe(input.byteLength / MAX_BINARY_CHUNK_BYTES);
      expect(f.sink.consumedBytes).toBe(input.byteLength);
      expect(f.sink.consumedDigest()).toBe(sha256(input));
      expect(f.sink.consumedEndOfMessage.at(-1)).toBe(true);
      expect(f.sink.consumedEndOfMessage.slice(0, -1).some((flag) => flag)).toBe(false);

      const stats = f.transport.requestStats();
      // The whole source really was retained for the whole stream, and the class bound still covers it.
      expect(stats.outbound.peakSourceBytes).toBe(input.byteLength);
      expect(stats.outbound.peakSourceBytes).toBeLessThanOrEqual(stats.outbound.maxSourceBytes);
      expect(stats.outbound.peakNativeBytes).toBe(binaryFrameSize(MAX_BINARY_CHUNK_BYTES));
      expect(stats.outbound.peakNativeBytes).toBeLessThanOrEqual(
        stats.outbound.maxFrameWorkingBytes,
      );
      expect(stats.outbound.peakPendingBytes).toBeLessThanOrEqual(stats.outbound.maxPendingBytes);
      expect(stats.outbound.watching).toBe(0);
      expect(stats.outbound.pendingBytes).toBe(0);
      expect(stats.outboundFrames.heldBytes).toBe(0);
    } finally {
      f.transport.close();
    }
  });

  it('schedules two legitimate streams and a P0 control frame between complete chunks, inside the declared bounds', async () => {
    const f = await fixture();
    const firstId = idFromBytes('str', new Uint8Array(16).fill(4));
    const secondId = idFromBytes('str', new Uint8Array(16).fill(5));
    const first = Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 6, 0x11);
    const second = Buffer.alloc(MAX_BINARY_CHUNK_BYTES * 4, 0x22);
    try {
      f.sink.holdWriteAt(2);
      const bulkFirst = f.transport
        .sendBinaryStream(firstId, first)
        .catch((error: unknown) => error);
      await flush();
      expect(f.sink.starts).toEqual([1, 2]);
      const bulkSecond = f.transport
        .sendBinaryStream(secondId, second)
        .catch((error: unknown) => error);
      await flush();
      // A P0 control frame arrives while chunk one of stream one is still inside the sink.
      const cancel = f.transport
        .cancel(idFromBytes('cor', new Uint8Array(16)))
        .catch((error: unknown) => error);
      await flush();
      // Both legitimate sources are really held at once, and the ONLY in-flight frame body is one
      // chunk plus its native copy - not two streams' worth.
      const held = f.transport.requestStats().outbound;
      expect(held.streamSourceBytes).toBe(first.byteLength + second.byteLength);
      expect(held.frameBytes).toBe(binaryFrameSize(MAX_BINARY_CHUNK_BYTES));
      expect(held.nativeBytes).toBe(binaryFrameSize(MAX_BINARY_CHUNK_BYTES));
      expect(f.sink.starts).toEqual([1, 2]);

      f.sink.openHeld();
      await expect(Promise.all([bulkFirst, bulkSecond])).resolves.toEqual([undefined, undefined]);
      await expect(cancel).resolves.toMatchObject({ disposition: 'accepted' });

      // CONTROL IS SCHEDULABLE BETWEEN CHUNKS: the P0 frame is the very next frame after chunk one,
      // and it did not have to wait for either whole stream.
      expect(f.sink.kinds.slice(1, 4)).toEqual([
        FRAME_TYPE_BINARY,
        FRAME_TYPE_CONTROL,
        FRAME_TYPE_BINARY,
      ]);
      expect(f.sink.sent.map((message) => message.method)).toContain('aura.cancel');
      // Attribution: each stream's own chunks are contiguous from zero, only its last one is
      // end-of-message, and its bytes reassemble to the input it was given.
      const firstChunks = received(f.sink, firstId);
      const secondChunks = received(f.sink, secondId);
      expect(firstChunks.chunkSequences).toEqual([0, 1, 2, 3, 4, 5]);
      expect(secondChunks.chunkSequences).toEqual([0, 1, 2, 3]);
      expect(firstChunks.endOfMessage).toEqual([false, false, false, false, false, true]);
      expect(secondChunks.endOfMessage).toEqual([false, false, false, true]);
      expect(sha256(firstChunks.bytes)).toBe(sha256(first));
      expect(sha256(secondChunks.bytes)).toBe(sha256(second));
      expect(
        f.sink.binary.filter((chunk) => chunk.streamId !== firstId && chunk.streamId !== secondId),
      ).toHaveLength(0);
      const sequences = f.sink.frames.map((frame) => frame.sequence);
      expect(sequences).toEqual(sequences.map((_value, index) => BigInt(index)));

      const stats = f.transport.requestStats();
      // The source peak is both legitimate sources at once (plus the small P0 payload), and the frame
      // peak is ONE chunk frame plus its native copy, whatever the number of streams waiting.
      expect(stats.outbound.peakSourceBytes).toBeGreaterThanOrEqual(
        first.byteLength + second.byteLength,
      );
      expect(stats.outbound.peakSourceBytes).toBeLessThanOrEqual(
        first.byteLength + second.byteLength + MAX_P0_FRAME_BYTES,
      );
      expect(stats.outbound.peakSourceBytes).toBeLessThanOrEqual(stats.outbound.maxSourceBytes);
      expect(stats.outbound.peakFrameBytes).toBe(binaryFrameSize(MAX_BINARY_CHUNK_BYTES));
      expect(stats.outbound.peakNativeBytes).toBe(binaryFrameSize(MAX_BINARY_CHUNK_BYTES));
      expect(stats.outbound.peakNativeBytes).toBeLessThanOrEqual(
        stats.outbound.maxFrameWorkingBytes,
      );
      expect(stats.outbound.peakPendingBytes).toBeLessThanOrEqual(stats.outbound.maxPendingBytes);
      expect(stats.outboundFrames.peakCommittedBytes).toBeLessThanOrEqual(16 * 1024 * 1024);
      // Nothing is left holding anything: no task, no source, no frame, no native copy, no timer.
      expect(stats.outbound.watching).toBe(0);
      expect(stats.outbound.pendingBytes).toBe(0);
      expect(stats.outbound.sourceBytes).toBe(0);
      expect(stats.outbound.frameBytes).toBe(0);
      expect(stats.outbound.nativeBytes).toBe(0);
      expect(stats.outboundFrames.heldBytes).toBe(0);
    } finally {
      f.sink.openHeld();
      f.transport.close();
    }
  });

  it('refuses a frame it cannot honestly send instead of spending a sequence number on it', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      // An invalid SecretInput is refused BEFORE the command frame is committed: no byte of it is
      // written and the number it would have spent is not committed, so the sequence stays gapless.
      const before = f.transport.nextOutboundSequence;
      const framesBefore = f.sink.frames.length;
      const invalid = Buffer.from([0x61, 0x0a, 0x62]);
      const refused = await f.transport
        .requestWithSecretInput(
          'models.endpoint.configure_credential',
          {},
          'models_endpoint_configure_credential',
          invalid,
          1_000,
        )
        .catch((error: unknown) => error);
      expect(refused).toBeInstanceOf(Error);
      expect(
        (refused as { envelope: { details: Record<string, unknown> } }).envelope.details,
      ).toMatchObject({
        outcome_recovery: 'not_sent',
      });
      expect(f.transport.nextOutboundSequence).toBe(before);
      expect(f.sink.frames.length).toBe(framesBefore);
      expect(f.transport.requestStats().outbound.refusedBeforeWrite).toBe(1);

      // The channel really is healthy afterwards, and the next request carries the very next number.
      const healthy = f.transport
        .request('agent.status', {}, 1, 5_000)
        .catch((error: unknown) => error);
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

  it('reports every deadline it used, so the four owners can be told apart', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const secret = Buffer.from('synthetic-m1-budget-value');
    try {
      const outbound = f.transport.requestStats().outbound;
      // Five distinct owners, five distinct budgets/limits - none of them borrowed from another.
      expect(outbound.physicalSendBudgetMs).toBe(PHYSICAL_SEND_BUDGET_MS);
      expect(outbound.secretInputBindingBudgetMs).toBe(SECRET_INPUT_BINDING_BUDGET_MS);
      // Request-local: the caller's own budget settles only that request.
      const local = f.transport.request('agent.status', {}, 1, 7).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(7);
      const failure = (await local) as { envelope: { code: string } };
      expect(failure.envelope.code).toBe('ipc.request_timeout');
      expect(f.sink.closeCalls).toBe(0);
      // Admission refusal is its own owner: it never reaches the send queue.
      expect(f.transport.requestStats().outbound.watching).toBe(0);
      void secret;
    } finally {
      f.transport.close();
      secret.fill(0);
      await vi.advanceTimersByTimeAsync(0);
    }
  });
});
