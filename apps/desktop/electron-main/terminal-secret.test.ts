import { describe, expect, it } from 'vitest';
import { encodeSecretInputFrame, type SecretInputOperation } from './protocol.js';
describe('terminal SecretInput frame', () => {
  it('binds the dedicated terminal operation and slot and wipes the source', () => {
    const secret = Buffer.from('temporary-terminal-value');
    const frame = encodeSecretInputFrame({
      binding: { channelId: 'chn_01ARZ3NDEKTSV4RRFFQ69G5FAV', connectionEpoch: 1n },
      commandInvocationId: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      correlationId: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      operation: 'terminal_sensitive_input' as SecretInputOperation,
      secret,
      sequence: 1n,
    });
    expect(frame[38]).toBe(6);
    expect(frame[71]).toBe(2);
    expect(secret.every((byte) => byte === 0)).toBe(true);
    frame.fill(0);
  });
});
