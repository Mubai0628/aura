import { Buffer } from 'node:buffer';
import { createHash } from 'node:crypto';
import { vi } from 'vitest';
import { CoreTransport } from '../core-transport.js';
import type { RuntimeHandshakeFinish } from '@aura/contracts/generated';
import { idFromBytes } from '../ids.js';
import { connectMainPipe, type MainPipeConnection } from '../main-pipe-bridge.js';
import { FrameDecoder, encodeControlFrame } from '../protocol.js';

/** The controlled pipe sink and frame readers the outbound send tests share. */
export const binding = { channelId: idFromBytes('chn', new Uint8Array(16)), connectionEpoch: 1n };

export const FRAME_TYPE_CONTROL = 1;

export const FRAME_TYPE_BINARY = 2;

export const FRAME_TYPE_SECRET_INPUT = 3;

/** One frame the sink really accepted, with the sequence number it carried on the wire. */
export interface ObservedFrame {
  readonly kind: number;
  readonly length: number;
  readonly ordinal: number;
  readonly sequence: bigint;
}

/** One binary chunk the sink really accepted, decoded the way the Core would decode it. */
export interface ReceivedChunk {
  readonly chunkSequence: bigint;
  readonly data: Buffer;
  readonly endOfMessage: boolean;
  readonly sequence: bigint;
  readonly streamId: string;
}

/**
 * A controllable sink with the same surface the native connection has.
 *
 * It can hold the Nth write, fail the Nth write, and - like the real addon - settle a held write when
 * the connection is cancelled. That last part is not decoration: it is what makes `close()` finite
 * here instead of leaving a promise nobody can settle.
 *
 * It also DECODES everything it accepts, so a case can assert what a receiver would really get back:
 * reassembled bytes, `chunkSequence`, `endOfMessage` placement and connection-sequence contiguity.
 */
export class ControlledSink implements MainPipeConnection {
  readonly binary: ReceivedChunk[] = [];
  readonly frames: ObservedFrame[] = [];
  readonly kinds: number[] = [];
  readonly ordinals: number[] = [];
  readonly starts: number[] = [];
  readonly sent: Record<string, unknown>[] = [];
  /** What a receiver that CONSUMES each chunk saw, without keeping another copy of the payload. */
  readonly consumedChunkSequences: bigint[] = [];
  readonly consumedEndOfMessage: boolean[] = [];
  readonly consumedSequences: bigint[] = [];
  readonly consumedStreamIds: string[] = [];
  consumedBytes = 0;
  consumedChunks = 0;
  cancelCalls = 0;
  closeCalls = 0;
  #consumeBinary: boolean;
  #consumedDigest = createHash('sha256');
  #decoder: FrameDecoder;
  #failOrdinals = new Set<number>();
  #gates = new Map<number, { promise: Promise<void>; release: () => void }>();
  #nextOrdinal = 0;
  #reader: ((bytes: Buffer) => void) | undefined;
  #sequence = 0n;

  constructor(
    onControl: (message: Record<string, unknown>) => void,
    options: { consumeBinary?: boolean } = {},
  ) {
    // A 64 MiB stream must not make the RECEIVER pile up a second whole copy: with `consumeBinary` the
    // sink hashes and counts each chunk and keeps no reference to it, so the measurement stays a
    // measurement of the SENDER. Retention is the default because most cases assert per-chunk detail.
    this.#consumeBinary = options.consumeBinary === true;
    this.#decoder = new FrameDecoder((frame) => {
      if (frame.type === 'binary') {
        this.consumedBytes += frame.data.byteLength;
        this.consumedChunks += 1;
        this.consumedChunkSequences.push(frame.chunkSequence);
        this.consumedEndOfMessage.push(frame.endOfMessage);
        this.consumedSequences.push(frame.sequence);
        this.consumedStreamIds.push(frame.streamId);
        this.#consumedDigest.update(frame.data);
        if (this.#consumeBinary) return;
        this.binary.push({
          chunkSequence: frame.chunkSequence,
          data: Buffer.from(frame.data),
          endOfMessage: frame.endOfMessage,
          sequence: frame.sequence,
          streamId: frame.streamId,
        });
        return;
      }
      const message = JSON.parse(frame.payload.toString()) as Record<string, unknown>;
      this.sent.push(message);
      onControl(message);
    });
  }

  /** The digest of everything this receiver consumed, in order. */
  consumedDigest(): string {
    return this.#consumedDigest.digest('hex');
  }

  info(): { serverCreationTime: string; serverPid: number; serverWindowsSessionId: number } {
    return { serverCreationTime: '1', serverPid: 1, serverWindowsSessionId: 1 };
  }

  failWriteAt(ordinal: number): void {
    this.#failOrdinals.add(ordinal);
  }

  /**
   * Hold exactly ONE write, by the ordinal it will have.
   *
   * Per-ordinal on purpose: a test that must show what happens at a COMPLETE-FRAME boundary has to be
   * able to release the frame being written without also releasing the one behind it. A gate that is
   * already open stays open, so a later write to the same ordinal is not blocked by a released gate.
   */
  holdWriteAt(ordinal: number): void {
    if (this.#gates.has(ordinal)) return;
    let open: () => void = () => undefined;
    const promise = new Promise<void>((resolve) => {
      open = resolve;
    });
    this.#gates.set(ordinal, { promise, release: open });
  }

  /** Let every held write finish. Called by a test, and by `cancel` the way the addon does. */
  openHeld(): void {
    for (const gate of this.#gates.values()) gate.release();
  }

  /**
   * Let exactly ONE held write finish, leaving every other held write where it is.
   *
   * A case that has to show "this frame finished and the next one did not" cannot use `openHeld`: a
   * released gate stays open, so releasing one ordinal would release every write behind it too.
   */
  openWriteAt(ordinal: number): void {
    this.#gates.get(ordinal)?.release();
  }

  /** Deliver a JSON-RPC response to the transport, as the Core would. */
  deliver(message: unknown): void {
    const resolve = this.#reader;
    if (resolve === undefined) throw new Error('no read was waiting');
    this.#reader = undefined;
    resolve(encodeControlFrame(Buffer.from(JSON.stringify(message)), binding, this.#sequence++));
  }

  read(): Promise<Buffer> {
    return new Promise<Buffer>((resolve) => {
      this.#reader = resolve;
    });
  }

  async write(bytes: Buffer): Promise<void> {
    const ordinal = ++this.#nextOrdinal;
    this.ordinals.push(ordinal);
    this.starts.push(ordinal);
    const kind = bytes[4] ?? -1;
    const gate = this.#gates.get(ordinal);
    if (gate !== undefined) await gate.promise;
    if (this.#failOrdinals.delete(ordinal)) {
      throw new Error('synthetic write failure after a possible partial write');
    }
    const sequence = bytes.readBigUInt64LE(29);
    const frame: ObservedFrame = { kind, length: bytes.byteLength, ordinal, sequence };
    this.frames.push(frame);
    this.kinds.push(kind);
    // Control and binary frames are decoded the way a receiver would. A SecretInput frame is NOT fed
    // to the decoder: the production decoder refuses one from the peer by design, and this sink stands
    // in for the Core's writer, not for its reader.
    if (kind !== FRAME_TYPE_SECRET_INPUT) this.#decoder.push(bytes);
  }

  cancel(): boolean {
    this.cancelCalls += 1;
    // The real addon cancels its outstanding overlapped I/O on cancel, which settles a held write. A
    // sink that ignored cancellation would leave the transport holding a promise nobody can settle.
    this.openHeld();
    return true;
  }

  close(): void {
    this.closeCalls += 1;
  }
}

export async function fixture(
  options: {
    consumeBinary?: boolean;
    physicalSendBudgetMilliseconds?: number;
    secretInputBindingBudgetMilliseconds?: number;
  } = {},
): Promise<{
  sink: ControlledSink;
  transport: CoreTransport;
  onClosed: ReturnType<typeof vi.fn>;
}> {
  const onClosed = vi.fn();
  const sink = new ControlledSink((message) => {
    const id = message.id;
    if (message.method === 'runtime.handshake.finish') {
      queueMicrotask(() => {
        sink.deliver({
          jsonrpc: '2.0',
          id,
          result: {
            terminal: 'succeeded',
            value: {
              channel_id: binding.channelId,
              connection_epoch: '1',
              handshake_kind: 'initial',
              transcript_hash: { algorithm: 'sha256', hex: 'a'.repeat(64) },
            },
          },
        });
      });
      return;
    }
    if (message.method === 'aura.cancel') {
      // A real Core always answers a control frame. A fixture that left one unanswered would turn the
      // test's own control request into a pending waiter rather than the timing question under test.
      queueMicrotask(() => {
        sink.deliver({
          jsonrpc: '2.0',
          id,
          result: { terminal: 'succeeded', value: { disposition: 'accepted' } },
        });
      });
      return;
    }
    if (message.method === 'runtime.command_outcome') {
      queueMicrotask(() => {
        sink.deliver({
          jsonrpc: '2.0',
          id,
          result: {
            terminal: 'succeeded',
            value: { terminal: 'succeeded', value: { replayed: false } },
          },
        });
      });
    }
  }, options);
  vi.mocked(connectMainPipe).mockResolvedValue(sink);
  const transport = await CoreTransport.connect(
    {
      ...binding,
      addonPath: 'fixture',
      expectedServerPid: 1,
      namespaceObjectHash: 'fixture',
      pipeNonce: 'fixture',
      runtimeInstanceId: 'fixture',
      ...(options.physicalSendBudgetMilliseconds === undefined
        ? {}
        : { physicalSendBudgetMilliseconds: options.physicalSendBudgetMilliseconds }),
      ...(options.secretInputBindingBudgetMilliseconds === undefined
        ? {}
        : {
            secretInputBindingBudgetMilliseconds: options.secretInputBindingBudgetMilliseconds,
          }),
    },
    { onBinaryChunk: vi.fn(), onClosed, onNotification: vi.fn() },
  );
  await transport.finishHandshake({
    handshake_kind: 'initial',
    transcript_hash: { algorithm: 'sha256', hex: 'a'.repeat(64) },
  } as RuntimeHandshakeFinish);
  return { onClosed, sink, transport };
}

/**
 * Let every queued microtask run.
 *
 * The transport's send path is promise-based on purpose (a write is not a synchronous step), so a test
 * that has just enqueued work has to let those turns happen before it reads the result. This is the
 * event barrier the cases below use instead of sleeping on wall-clock time.
 */
export const flush = async (): Promise<void> => {
  for (let turn = 0; turn < 8; turn += 1) await Promise.resolve();
};

export const sha256 = (bytes: Buffer): string => createHash('sha256').update(bytes).digest('hex');

/**
 * What a receiver really rebuilt from the frames the sink accepted.
 *
 * This is the receiver's side of the wire, decoded with the production `FrameDecoder`: it is what
 * makes "all the bytes arrived", "`endOfMessage` is on the last chunk only" and "`chunkSequence` is
 * contiguous" observable facts about the run instead of claims about the code.
 */
export function received(
  sink: ControlledSink,
  streamId: string,
): {
  bytes: Buffer;
  chunkSequences: number[];
  endOfMessage: boolean[];
  lengths: number[];
  sequences: bigint[];
} {
  const chunks = sink.binary.filter((chunk) => chunk.streamId === streamId);
  return {
    bytes: Buffer.concat(chunks.map((chunk) => chunk.data)),
    chunkSequences: chunks.map((chunk) => Number(chunk.chunkSequence)),
    endOfMessage: chunks.map((chunk) => chunk.endOfMessage),
    lengths: chunks.map((chunk) => chunk.data.byteLength),
    sequences: chunks.map((chunk) => chunk.sequence),
  };
}
