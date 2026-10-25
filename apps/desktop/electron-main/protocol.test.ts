import { Buffer } from 'node:buffer';

import { describe, expect, it } from 'vitest';

import { idFromBytes } from './ids.js';
import {
  FrameDecoder,
  MAX_CONTROL_PAYLOAD_BYTES,
  encodeControlFrame,
  encodeSecretInputFrame,
} from './protocol.js';

const binding = {
  channelId: idFromBytes(
    'chn',
    Uint8Array.from({ length: 16 }, (_, index) => index + 32),
  ),
  connectionEpoch: 4n,
};

describe('SecretInput V1 codec', () => {
  it('emits the frozen golden payload and clears the source bytes', () => {
    const secret = Buffer.from([0x61, 0x62, 0x63]);
    const frame = encodeSecretInputFrame({
      binding,
      commandInvocationId: idFromBytes(
        'civ',
        Uint8Array.from({ length: 16 }, (_, index) => index + 16),
      ),
      correlationId: idFromBytes(
        'cor',
        Uint8Array.from({ length: 16 }, (_, index) => index),
      ),
      operation: 'models_endpoint_create_with_credential',
      secret,
      sequence: 9n,
    });

    expect([...secret]).toEqual([0, 0, 0]);
    expect([...frame.subarray(37)]).toEqual([
      0x01, 0x01, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
      0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
      0x1c, 0x1d, 0x1e, 0x1f, 0x01, 0x01, 0x03, 0x00, 0x00, 0x00, 0x61, 0x62, 0x63,
    ]);
  });

  it('rejects invalid UTF-8 or forbidden controls and clears the input', () => {
    for (const secret of [Buffer.from([0xff]), Buffer.from([0x61, 0x0a, 0x62])]) {
      expect(() =>
        encodeSecretInputFrame({
          binding,
          commandInvocationId: idFromBytes('civ', new Uint8Array(16)),
          correlationId: idFromBytes('cor', new Uint8Array(16)),
          operation: 'models_endpoint_replace_credential',
          secret,
          sequence: 1n,
        }),
      ).toThrow('SecretInput bytes are invalid');
      expect(secret.every((byte) => byte === 0)).toBe(true);
    }
  });
});

describe('catalog-sized P1 control frames', () => {
  it.each([2 * 1024 * 1024, 5 * 1024 * 1024])(
    'round-trips a %i-byte payload in one frame',
    (payloadSize) => {
      const payload = Buffer.alloc(payloadSize, 0x43);
      const encoded = encodeControlFrame(payload, binding, 12n);
      let decodedLength = 0;
      const decoder = new FrameDecoder((frame) => {
        expect(frame.type).toBe('control');
        if (frame.type === 'control') decodedLength = frame.payload.byteLength;
      });

      decoder.push(encoded);
      expect(decodedLength).toBe(payloadSize);
    },
  );

  it('rejects an oversized payload before any frame exists', () => {
    expect(() =>
      encodeControlFrame(Buffer.alloc(MAX_CONTROL_PAYLOAD_BYTES + 1), binding, 1n),
    ).toThrow('control payload exceeds the 6 MiB protocol limit');
  });
});
