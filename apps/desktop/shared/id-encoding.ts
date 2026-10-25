// Portable encoding extracted from Main; no Node or Electron dependency.
export const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

export const idFromBytes = (prefix: string, bytes: Uint8Array): string => {
  if (bytes.byteLength !== 16) {
    throw new RangeError('Aura IDs require exactly 16 bytes');
  }
  let value = 0n;
  for (const byte of bytes) {
    value = (value << 8n) | BigInt(byte);
  }
  let body = '';
  for (let index = 0; index < 26; index += 1) {
    body = `${CROCKFORD[Number(value & 31n)]}${body}`;
    value >>= 5n;
  }
  return `${prefix}_${body}`;
};
