// REAL NAMED PIPE + REAL NATIVE ADDON + A LEGITIMATE 20 MiB+ STREAM.
//
// WHAT IS REAL HERE: the production `OutboundSendOwner` and the production `FrameScheduler` - the same
// two classes `CoreTransport` drives - writing through the REAL `aura_main_pipe.node` addon into a REAL
// Windows named pipe created by this probe. The frames on the wire are the production encoders' bytes,
// and the copy the addon makes of each one is its own `operation->bytes.assign`.
//
// WHAT THIS IS NOT: the peer is THIS probe's receiver, not the Core. There is no `aura-core.exe`, no
// handshake, no JSON-RPC reply and no Rust staging policy in this case; the transport-level integration
// and the production staging chain are covered elsewhere. The receiver CONSUMES every chunk - it hashes
// and counts them and keeps no copy - so what is observed is the sender plus the addon's own copy, not
// a second whole buffer inside the probe.
import { Buffer } from 'node:buffer';
import { createHash } from 'node:crypto';
import { createServer, type Server, type Socket } from 'node:net';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, describe, expect, it } from 'vitest';

import { idFromBytes } from '../../../apps/desktop/electron-main/ids.js';
import { FrameScheduler } from '../../../apps/desktop/electron-main/frame-scheduler.js';
import {
  connectMainPipe,
  type MainPipeConnection,
} from '../../../apps/desktop/electron-main/main-pipe-bridge.js';
import {
  binaryFrameSize,
  createTaskKey,
  OutboundSendOwner,
  type OutboundSendOwnerFacts,
} from '../../../apps/desktop/electron-main/outbound-send-owner.js';
import {
  FrameDecoder,
  MAX_BINARY_CHUNK_BYTES,
} from '../../../apps/desktop/electron-main/protocol.js';

const ADDON_PATH = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../../../apps/desktop/native/aura-main-pipe/build/Release/aura_main_pipe.node',
);

// The addon builds `\\.\pipe\Aura\<32 lowercase hex>\run_<26 Crockford>\<nonce>` and validates each
// part, so this probe has to name exactly that pipe - which is what makes the connection real.
const namespaceObjectHash = 'e'.repeat(32);
const pipeNonce = 'f'.repeat(32);
const runtimeInstanceId = `run_${'0123456789ABCDEFGHJKMNPQRS'.slice(0, 26)}`;
const pipePath = `\\\\.\\pipe\\Aura\\${namespaceObjectHash}\\${runtimeInstanceId}\\${pipeNonce}`;
const binding = { channelId: idFromBytes('chn', new Uint8Array(16).fill(21)), connectionEpoch: 1n };

/** One stream this receiver really rebuilt from the OS pipe, consumed chunk by chunk. */
interface Receipt {
  bytes: number;
  chunkSequences: bigint[];
  chunks: number;
  digest: ReturnType<typeof createHash>;
  endOfMessage: boolean[];
  frames: number[];
  sequences: bigint[];
  streamIds: Set<string>;
}

/**
 * The receiver side of the real named pipe.
 *
 * It splits the byte stream on the wire's own 4-byte length prefix, so a frame counts as received only
 * when the length the sender declared has really arrived. Control and binary frames go to the
 * PRODUCTION `FrameDecoder`; the decoded chunk is hashed and counted and then dropped, so the probe
 * never holds a second copy of the stream.
 */
class PipeReceiver {
  readonly streams = new Map<string, Receipt>();
  readonly frames: { bytes: number; declaredBytes: number; kind: number }[] = [];
  readonly sockets: Socket[] = [];
  error: string | null = null;
  #buffer: Buffer = Buffer.alloc(0);
  #decoder: FrameDecoder;

  constructor() {
    this.#decoder = new FrameDecoder((frame) => {
      if (frame.type !== 'binary') return;
      const receipt =
        this.streams.get(frame.streamId) ??
        (() => {
          const fresh: Receipt = {
            bytes: 0,
            chunkSequences: [],
            chunks: 0,
            digest: createHash('sha256'),
            endOfMessage: [],
            frames: [],
            sequences: [],
            streamIds: new Set<string>(),
          };
          this.streams.set(frame.streamId, fresh);
          return fresh;
        })();
      receipt.bytes += frame.data.byteLength;
      receipt.chunkSequences.push(frame.chunkSequence);
      receipt.chunks += 1;
      receipt.digest.update(frame.data);
      receipt.endOfMessage.push(frame.endOfMessage);
      receipt.frames.push(frame.data.byteLength);
      receipt.sequences.push(frame.sequence);
      receipt.streamIds.add(frame.streamId);
    });
  }

  push(chunk: Buffer): void {
    this.#buffer = this.#buffer.byteLength === 0 ? chunk : Buffer.concat([this.#buffer, chunk]);
    for (;;) {
      if (this.#buffer.byteLength < 4) return;
      const declaredBytes = this.#buffer.readUInt32LE(0);
      if (this.#buffer.byteLength < declaredBytes + 4) return;
      const frame = this.#buffer.subarray(0, declaredBytes + 4);
      this.#buffer = this.#buffer.subarray(declaredBytes + 4);
      const kind = frame[4];
      if (kind === undefined) {
        this.error = 'a received frame carried no frame kind';
        return;
      }
      this.frames.push({ bytes: frame.byteLength, declaredBytes: declaredBytes + 4, kind });
      try {
        this.#decoder.push(Buffer.from(frame));
      } catch (error) {
        this.error = error instanceof Error ? error.message : String(error);
      }
    }
  }

  receipt(streamId: string): Receipt {
    const found = this.streams.get(streamId);
    if (found === undefined) throw new Error(`stream ${streamId} was never received`);
    return found;
  }
}

const open: { connection: MainPipeConnection; receiver: PipeReceiver; server: Server }[] = [];
afterEach(() => {
  for (const entry of open.splice(0)) {
    try {
      entry.connection.close();
    } catch {
      // The probe's own teardown must not become the failure under test.
    }
    for (const socket of entry.receiver.sockets) socket.destroy();
    entry.server.close();
  }
});

/** A real named-pipe server in THIS process, plus the real addon connected to it. */
async function openRealPipe(): Promise<{ connection: MainPipeConnection; receiver: PipeReceiver }> {
  const receiver = new PipeReceiver();
  const server = createServer((socket) => {
    receiver.sockets.push(socket);
    socket.on('data', (chunk: Buffer) => receiver.push(chunk));
    socket.on('error', () => undefined);
  });
  await new Promise<void>((resolveListen, rejectListen) => {
    server.once('error', rejectListen);
    server.listen(pipePath, () => resolveListen());
  });
  const connection = await connectMainPipe({
    addonPath: ADDON_PATH,
    expectedServerPid: process.pid,
    namespaceObjectHash,
    pipeNonce,
    runtimeInstanceId,
  });
  open.push({ connection, receiver, server });
  return { connection, receiver };
}

const settle = async (): Promise<void> => {
  for (let turn = 0; turn < 40; turn += 1) await Promise.resolve();
  await new Promise((resolveTurn) => setTimeout(resolveTurn, 400));
};

describe('real named pipe: a legitimate 20 MiB stream through the real addon', () => {
  it('sends every chunk as a complete frame and accounts for the native write copy', async () => {
    const { connection, receiver } = await openRealPipe();
    // The sink IS the real addon's `write`. The probe samples the owner's own facts immediately after
    // the call returns, which is exactly the moment the addon already holds its copy of this frame and
    // its promise has not settled yet.
    let samples: OutboundSendOwnerFacts[] = [];
    let ordinal = 0;
    const scheduler = new FrameScheduler({
      write: async (bytes: Buffer) => {
        ordinal += 1;
        const writing = connection.write(bytes);
        // Sampled AFTER the addon call returned - its `bytes.assign` has already happened - and while
        // that write's promise is still outstanding.
        if (ordinal <= 2) samples = [...samples, owner.facts()];
        await writing;
      },
    });
    const retired: Error[] = [];
    const owner = new OutboundSendOwner(binding, scheduler, {
      onSettled: () => undefined,
      onUnrecoverable: (error) => retired.push(error),
      onWriteStarted: () => undefined,
    });

    const streamId = idFromBytes('str', new Uint8Array(16).fill(31));
    // Deliberately NOT chunk-aligned: 80 full chunks plus a short one, so the last chunk really is the
    // smaller frame the receiver must still accept as end-of-message.
    const input = Buffer.alloc(20 * 1024 * 1024 + 4_096);
    for (let index = 0; index < input.byteLength; index += 4093)
      input[index] = (index / 4093) % 251;
    const expected = createHash('sha256').update(input).digest('hex');
    const frames = Math.ceil(input.byteLength / MAX_BINARY_CHUNK_BYTES);

    const task = owner.sendBinaryStream(streamId, input, createTaskKey('realpipe-20mib'));
    await task;
    await settle();

    expect(receiver.error).toBeNull();
    // Every frame arrived COMPLETE: the length the sender declared is the length that arrived.
    expect(receiver.frames).toHaveLength(frames);
    for (const frame of receiver.frames) expect(frame.bytes).toBe(frame.declaredBytes);
    expect(receiver.frames.every((frame) => frame.kind === 2)).toBe(true);

    const receipt = receiver.receipt(streamId);
    expect(receipt.chunks).toBe(frames);
    expect(receipt.chunks).toBe(81);
    expect(receipt.bytes).toBe(input.byteLength);
    expect(receipt.digest.digest('hex')).toBe(expected);
    expect(receipt.streamIds).toEqual(new Set([streamId]));
    expect(receipt.chunkSequences).toEqual(
      Array.from({ length: frames }, (_value, index) => BigInt(index)),
    );
    expect(receipt.endOfMessage.slice(0, -1).some((flag) => flag)).toBe(false);
    expect(receipt.endOfMessage.at(-1)).toBe(true);
    expect(receipt.frames.at(-1)).toBe(4_096);
    expect(receipt.sequences).toEqual(
      Array.from({ length: frames }, (_value, index) => BigInt(index)),
    );

    // While the REAL addon held its copy, the owner's own diagnostics named all three holders of these
    // bytes: the retained source, the encoded frame and the native copy of it. The second sample is the
    // next frame's write, so it also shows the previous frame's working bytes really were given back by
    // its own promise - the working figure is ONE frame plus ONE copy, never two frames' worth.
    const firstFrameBytes = binaryFrameSize(MAX_BINARY_CHUNK_BYTES);
    expect(samples).toHaveLength(2);
    for (const sample of samples) {
      expect(sample.frameBytes).toBe(firstFrameBytes);
      expect(sample.nativeBytes).toBe(firstFrameBytes);
      expect(sample.sourceBytes).toBe(input.byteLength);
      expect(sample.pendingBytes).toBe(input.byteLength + 2 * firstFrameBytes);
      expect(sample.pendingBytes).toBeLessThanOrEqual(sample.maxPendingBytes);
      expect(sample.nativeBytes).toBeLessThanOrEqual(sample.maxFrameWorkingBytes);
    }

    const facts = owner.facts();
    expect(facts.peakSourceBytes).toBe(input.byteLength);
    expect(facts.peakSourceBytes).toBeLessThanOrEqual(facts.maxSourceBytes);
    expect(facts.peakFrameBytes).toBe(firstFrameBytes);
    expect(facts.peakNativeBytes).toBe(firstFrameBytes);
    expect(facts.peakNativeBytes).toBeLessThanOrEqual(facts.maxFrameWorkingBytes);
    expect(facts.peakPendingBytes).toBeLessThanOrEqual(facts.maxPendingBytes);
    expect(facts.pendingBytes).toBe(0);
    expect(facts.nativeBytes).toBe(0);
    expect(facts.sourceBytes).toBe(0);
    expect(facts.watching).toBe(0);
    expect(facts.refusalsOverBudget).toBe(0);
    expect(retired).toEqual([]);
  }, 120_000);
});
