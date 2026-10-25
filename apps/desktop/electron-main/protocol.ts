/**
 * The Aura IPC wire codec: frame encoders, the frame decoder, and the frame-size constants they are
 * bounded by.
 *
 * WHAT BELONGS HERE: how a frame is laid out on the wire and how it is read back. Nothing in this
 * file decides WHEN a frame is written - that is `frame-scheduler.ts` - and nothing here knows about
 * a request, a budget or a connection.
 */

import { Buffer, isUtf8 } from 'node:buffer';

import { idFromBytes, idToBytes } from './ids.js';

export const MAX_FRAME_LENGTH = 16 * 1024 * 1024;
// Provider Catalog V1 is transferred as one JSON-RPC result. The 6 MiB limit
// carries the frozen 5 MiB catalog ceiling plus its response envelope.
export const MAX_CONTROL_PAYLOAD_BYTES = 6 * 1024 * 1024;
export const MAX_BINARY_CHUNK_BYTES = 256 * 1024;
const BINARY_HEADER_BYTES = 25;
const AUTHENTICATED_HEADER_BYTES = 32;
const FRAME_LENGTH_PREFIX_BYTES = 4;
const FRAME_TYPE_CONTROL = 1;
const FRAME_TYPE_BINARY = 2;
const FRAME_TYPE_SECRET_INPUT = 3;
const SECRET_INPUT_FIXED_HEADER_BYTES = 40;
/**
 * The one bound a SecretInput payload is validated against, exported so the send path can size its own
 * source budget from the same number instead of copying it.
 */
export const MAX_SECRET_INPUT_BYTES = 2_560;

export type SecretInputOperation =
  | 'models_endpoint_create_with_credential'
  | 'models_endpoint_configure_credential'
  | 'models_endpoint_replace_credential'
  | 'tools_credential_create'
  | 'tools_credential_rotate'
  | 'terminal_sensitive_input'
  | 'mcp_secret_set';

export interface IpcFrameBinding {
  channelId: string;
  connectionEpoch: bigint;
}

export type DecodedFrame =
  | {
      channelId: string;
      connectionEpoch: bigint;
      payload: Buffer;
      sequence: bigint;
      type: 'control';
    }
  | {
      channelId: string;
      chunkSequence: bigint;
      connectionEpoch: bigint;
      data: Buffer;
      endOfMessage: boolean;
      sequence: bigint;
      streamId: string;
      type: 'binary';
    };

export const encodeSecretInputFrame = (input: {
  binding: IpcFrameBinding;
  commandInvocationId: string;
  correlationId: string;
  operation: SecretInputOperation;
  secret: Buffer;
  sequence: bigint;
}): Buffer => {
  const { secret } = input;
  try {
    validateSecretInputBytes(secret);
    const bodyLength =
      1 + AUTHENTICATED_HEADER_BYTES + SECRET_INPUT_FIXED_HEADER_BYTES + secret.byteLength;
    const frame = Buffer.alloc(bodyLength + 4);
    frame.writeUInt32LE(bodyLength, 0);
    frame[4] = FRAME_TYPE_SECRET_INPUT;
    idToBytes('chn', input.binding.channelId).copy(frame, 5);
    frame.writeBigUInt64LE(input.binding.connectionEpoch, 21);
    frame.writeBigUInt64LE(input.sequence, 29);
    const payloadOffset = 37;
    frame[payloadOffset] = 1;
    frame[payloadOffset + 1] = secretOperationDiscriminant(input.operation);
    idToBytes('cor', input.correlationId).copy(frame, payloadOffset + 2);
    idToBytes('civ', input.commandInvocationId).copy(frame, payloadOffset + 18);
    frame[payloadOffset + 34] = input.operation === 'terminal_sensitive_input' ? 2 : 1;
    frame[payloadOffset + 35] = 1;
    frame.writeUInt32LE(secret.byteLength, payloadOffset + 36);
    secret.copy(frame, payloadOffset + SECRET_INPUT_FIXED_HEADER_BYTES);
    return frame;
  } finally {
    secret.fill(0);
  }
};

const secretOperationDiscriminant = (operation: SecretInputOperation): number => {
  switch (operation) {
    case 'models_endpoint_create_with_credential':
      return 1;
    case 'models_endpoint_configure_credential':
      return 2;
    case 'models_endpoint_replace_credential':
      return 3;
    case 'tools_credential_create':
      return 4;
    case 'tools_credential_rotate':
      return 5;
    case 'terminal_sensitive_input':
      return 6;
    case 'mcp_secret_set':
      return 7;
  }
};

/**
 * The one validation rule for SecretInput bytes. Exported so the send path can validate the bytes it
 * will actually send BEFORE the command control frame is committed: an invalid secret must be a
 * request-local rejection with nothing on the wire, never half of an atomic binding.
 */
export const validateSecretInputBytes = (secret: Buffer): void => {
  if (secret.byteLength < 1 || secret.byteLength > MAX_SECRET_INPUT_BYTES || !isUtf8(secret)) {
    throw new Error('SecretInput bytes are invalid');
  }
  for (let index = 0; index < secret.byteLength; index += 1) {
    const byte = secret[index];
    if (byte === undefined) throw new Error('SecretInput bytes are invalid');
    if (byte <= 0x1f || byte === 0x7f) throw new Error('SecretInput bytes are invalid');
    if (
      byte === 0xe2 &&
      secret[index + 1] === 0x80 &&
      (secret[index + 2] === 0xa8 || secret[index + 2] === 0xa9)
    ) {
      throw new Error('SecretInput bytes are invalid');
    }
  }
};

export const encodeControlFrame = (
  payload: Buffer,
  binding: IpcFrameBinding,
  sequence: bigint,
): Buffer => {
  if (payload.byteLength > MAX_CONTROL_PAYLOAD_BYTES) {
    throw new RangeError('control payload exceeds the 6 MiB protocol limit');
  }
  const bodyLength = payload.byteLength + 1 + AUTHENTICATED_HEADER_BYTES;
  const frame = Buffer.alloc(bodyLength + 4);
  frame.writeUInt32LE(bodyLength, 0);
  frame[4] = FRAME_TYPE_CONTROL;
  idToBytes('chn', binding.channelId).copy(frame, 5);
  frame.writeBigUInt64LE(binding.connectionEpoch, 21);
  frame.writeBigUInt64LE(sequence, 29);
  payload.copy(frame, 37);
  return frame;
};

export const encodeBinaryFrame = (input: {
  binding: IpcFrameBinding;
  chunkSequence: bigint;
  data: Buffer;
  endOfMessage: boolean;
  sequence: bigint;
  streamId: string;
}): Buffer => {
  if (input.data.byteLength > MAX_BINARY_CHUNK_BYTES) {
    throw new RangeError('binary chunk exceeds the 256 KiB protocol limit');
  }
  const bodyLength = 1 + AUTHENTICATED_HEADER_BYTES + BINARY_HEADER_BYTES + input.data.byteLength;
  const frame = Buffer.alloc(bodyLength + FRAME_LENGTH_PREFIX_BYTES);
  frame.writeUInt32LE(bodyLength, 0);
  frame[4] = FRAME_TYPE_BINARY;
  idToBytes('chn', input.binding.channelId).copy(frame, 5);
  frame.writeBigUInt64LE(input.binding.connectionEpoch, 21);
  frame.writeBigUInt64LE(input.sequence, 29);
  const payloadOffset = 37;
  idToBytes('str', input.streamId).copy(frame, payloadOffset);
  frame.writeBigUInt64LE(input.chunkSequence, payloadOffset + 16);
  frame[payloadOffset + 24] = input.endOfMessage ? 1 : 0;
  input.data.copy(frame, payloadOffset + BINARY_HEADER_BYTES);
  return frame;
};

export class FrameDecoder {
  readonly #onFrame: (frame: DecodedFrame) => void;
  #buffer: Buffer<ArrayBufferLike> = Buffer.alloc(0);

  constructor(onFrame: (frame: DecodedFrame) => void) {
    this.#onFrame = onFrame;
  }

  push(chunk: Buffer): void {
    this.#buffer = this.#buffer.byteLength === 0 ? chunk : Buffer.concat([this.#buffer, chunk]);
    while (this.#buffer.byteLength >= 4) {
      const bodyLength = this.#buffer.readUInt32LE(0);
      if (bodyLength < 1 || bodyLength > MAX_FRAME_LENGTH) {
        if (this.#buffer[4] === FRAME_TYPE_SECRET_INPUT) {
          this.#buffer.fill(0);
          throw new Error('invalid Aura SecretInput frame');
        }
        throw new Error(`invalid Aura IPC frame length: ${bodyLength}`);
      }
      if (this.#buffer.byteLength < bodyLength + 4) return;
      const body = this.#buffer.subarray(4, bodyLength + 4);
      this.#buffer = this.#buffer.subarray(bodyLength + 4);
      const frameType = body[0];
      if (body.byteLength < 1 + AUTHENTICATED_HEADER_BYTES) {
        if (frameType === FRAME_TYPE_SECRET_INPUT) body.fill(0);
        throw new Error('truncated Aura IPC authenticated frame header');
      }
      const channelId = idFromBytes('chn', body.subarray(1, 17));
      const connectionEpoch = body.readBigUInt64LE(17);
      const sequence = body.readBigUInt64LE(25);
      const authenticatedPayload = body.subarray(1 + AUTHENTICATED_HEADER_BYTES);
      if (frameType === FRAME_TYPE_CONTROL) {
        const payload = authenticatedPayload;
        if (payload.byteLength > MAX_CONTROL_PAYLOAD_BYTES) {
          throw new Error('Aura IPC control frame exceeded 6 MiB');
        }
        this.#onFrame({ channelId, connectionEpoch, payload, sequence, type: 'control' });
        continue;
      }
      if (frameType === FRAME_TYPE_SECRET_INPUT) {
        authenticatedPayload.fill(0);
        throw new Error('Core sent an unexpected SecretInput frame');
      }
      if (frameType !== FRAME_TYPE_BINARY) {
        throw new Error(`unknown Aura IPC frame type: ${String(frameType)}`);
      }
      const payload = authenticatedPayload;
      if (payload.byteLength < BINARY_HEADER_BYTES) {
        throw new Error('truncated Aura IPC binary header');
      }
      const flags = payload[24];
      if (flags === undefined || (flags & 0xfe) !== 0) {
        throw new Error('Aura IPC binary frame contained unknown flags');
      }
      const data = payload.subarray(BINARY_HEADER_BYTES);
      if (data.byteLength > MAX_BINARY_CHUNK_BYTES) {
        throw new Error('Aura IPC binary chunk exceeded 256 KiB');
      }
      this.#onFrame({
        channelId,
        chunkSequence: payload.readBigUInt64LE(16),
        connectionEpoch,
        data,
        endOfMessage: (flags & 1) === 1,
        sequence,
        streamId: idFromBytes('str', payload.subarray(0, 16)),
        type: 'binary',
      });
    }
  }
}
