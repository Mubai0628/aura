import { vi, type Mock } from 'vitest';
import type { RuntimeHandshakeFinish } from '@aura/contracts/generated';
import { CoreTransport } from './core-transport.js';
import { idFromBytes } from './ids.js';
import { FrameDecoder, encodeBinaryFrame, encodeControlFrame } from './protocol.js';
import { connectMainPipe, type MainPipeConnection } from './main-pipe-bridge.js';

export const binding = { channelId: idFromBytes('chn', new Uint8Array(16)), connectionEpoch: 1n };
/** Frame kind byte from protocol.ts. Control frames are echoed into the inbound decoder below. */
export const CONTROL_FRAME_KIND = 1;
/** Frame kind byte for SecretInput: outbound only, so it is never echoed back to the decoder. */
export const SECRET_FRAME_KIND = 3;

interface TransportFixture {
  transport: CoreTransport;
  pipe: MainPipeConnection;
  sent: Record<string, unknown>[];
  kinds: number[];
  deliver: (message: unknown) => void;
  deliverRaw: (
    message: unknown,
    frameBinding: { channelId: string; connectionEpoch: bigint },
    frameSequence: bigint,
  ) => void;
  /** Push one chunk of a binary stream as the next inbound frame. */
  deliverBinary: (chunk: { streamId: string; chunkSequence: bigint; data: Buffer }) => void;
  onBinaryChunk: Mock;
  onClosed: Mock;
  onNotification: Mock;
  holdWrites: (kind?: number) => void;
  releaseWrites: () => void;
  fail: (kind?: number) => void;
}

export async function fixture(
  options: { onLifecycle?: (record: unknown) => void } = {},
): Promise<TransportFixture> {
  const sent: Record<string, unknown>[] = [];
  const kinds: number[] = [];
  let reader: ((bytes: Buffer) => void) | undefined;
  let sequence = 0n;
  const deliver = (message: unknown) => deliverRaw(message, binding, sequence++);
  /** Push an arbitrary frame so wrong-epoch and wrong-sequence negatives can be built. */
  const deliverRaw = (
    message: unknown,
    frameBinding: { channelId: string; connectionEpoch: bigint },
    frameSequence: bigint,
  ) => {
    const resolve = reader;
    if (resolve === undefined) throw new Error('No pending read');
    reader = undefined;
    resolve(encodeControlFrame(Buffer.from(JSON.stringify(message)), frameBinding, frameSequence));
  };
  const deliverBinary = (chunk: { streamId: string; chunkSequence: bigint; data: Buffer }) => {
    const resolve = reader;
    if (resolve === undefined) throw new Error('No pending read');
    reader = undefined;
    resolve(encodeBinaryFrame({ binding, endOfMessage: false, sequence: sequence++, ...chunk }));
  };
  const onNotification = vi.fn();
  const decoder = new FrameDecoder((frame) => {
    if (frame.type !== 'control') throw new Error('Expected control');
    const message = JSON.parse(frame.payload.toString()) as Record<string, unknown>;
    sent.push(message);
    if (message.method === 'runtime.handshake.finish') {
      deliver({
        jsonrpc: '2.0',
        id: message.id,
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
    }
  });
  // A controllable sink: a gate so a test can hold a frame inside the write, and an injected failure
  // so a test can make the sink fail after a possible partial write. Ungated by default, so every
  // other test keeps the immediate-write behaviour it already relies on.
  let writeGate: Promise<void> | undefined;
  let openGate: (() => void) | undefined;
  let gatedKind: number | undefined;
  let failingWrite: number | 'any' | undefined;
  /** Hold the next write, of one frame kind or of any kind. */
  const holdWrites = (kind?: number) => {
    gatedKind = kind;
    writeGate = new Promise<void>((resolve) => {
      openGate = resolve;
    });
  };
  const releaseWrites = () => {
    openGate?.();
    writeGate = undefined;
    openGate = undefined;
    gatedKind = undefined;
  };
  const fail = (kind?: number) => {
    failingWrite = kind ?? 'any';
  };
  const pipe: MainPipeConnection = {
    read: vi.fn(
      () =>
        new Promise<Buffer>((resolve) => {
          reader = resolve;
        }),
    ),
    write: vi.fn(async (bytes: Buffer) => {
      const kind = bytes[4];
      if (writeGate !== undefined && (gatedKind === undefined || gatedKind === kind)) {
        await writeGate;
      }
      if (failingWrite !== undefined && (failingWrite === 'any' || failingWrite === kind)) {
        failingWrite = undefined;
        throw new Error('synthetic write failure after a possible partial write');
      }
      if (kind !== undefined) kinds.push(kind);
      if (kind === CONTROL_FRAME_KIND) decoder.push(bytes);
    }),
    cancel: vi.fn(() => true),
    close: vi.fn(),
    info: () => ({ serverCreationTime: '1', serverPid: 1, serverWindowsSessionId: 1 }),
  };
  vi.mocked(connectMainPipe).mockResolvedValue(pipe);
  const onClosed = vi.fn();
  const onBinaryChunk = vi.fn();
  const transport = await CoreTransport.connect(
    {
      ...binding,
      addonPath: 'fixture',
      expectedServerPid: 1,
      namespaceObjectHash: 'fixture',
      pipeNonce: 'fixture',
      runtimeInstanceId: 'fixture',
      ...(options.onLifecycle === undefined ? {} : { onLifecycle: options.onLifecycle }),
    },
    { onClosed, onNotification, onBinaryChunk },
  );
  await transport.finishHandshake({
    handshake_kind: 'initial',
    transcript_hash: { algorithm: 'sha256', hex: 'a'.repeat(64) },
  } as RuntimeHandshakeFinish);
  return {
    transport,
    pipe,
    sent,
    kinds,
    deliver,
    deliverRaw,
    deliverBinary,
    onBinaryChunk,
    onClosed,
    onNotification,
    holdWrites,
    releaseWrites,
    fail,
  };
}
