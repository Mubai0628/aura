import { describe, expect, it } from 'vitest';
import {
  authenticationMac,
  base64ToBytes,
  bytesToBase64,
  hex,
  hmacSha256,
} from './authentication.js';

const text = (value: string) => new Uint8Array(new TextEncoder().encode(value));

describe('the proof of pairing', () => {
  it('is HMAC-SHA256 by the RFC 4231 vectors', async () => {
    expect(hex(await hmacSha256(new Uint8Array(20).fill(0x0b), text('Hi There')))).toBe(
      'b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7',
    );
    expect(hex(await hmacSha256(text('Jefe'), text('what do ya want for nothing?')))).toBe(
      '5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843',
    );
  });

  it('binds the challenge, the extension and the protocol exactly as Core checks it', async () => {
    // The same vector `aura-chrome-domain`'s tests pin.
    const mac = await authenticationMac(
      new Uint8Array(32).fill(7),
      text('nonce-one'),
      'bheiijocpacefhgapeegbnekchcpedkj',
    );
    expect(mac).toBe('1ab19d4b8cbbeb8af9b05e05c7035e978f3b36b596eb988821f6ff0950fa223a');
  });

  it('keeps bytes through base64', () => {
    const bytes = new Uint8Array([0, 1, 254, 255, 128]);
    expect(Array.from(base64ToBytes(bytesToBase64(bytes)))).toEqual(Array.from(bytes));
  });
});
