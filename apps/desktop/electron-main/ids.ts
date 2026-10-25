import { randomBytes } from 'node:crypto';
import { CROCKFORD, idFromBytes as encodeId } from '../shared/id-encoding.js';

type MainGeneratedIdPrefix = 'chn' | 'civ' | 'cor' | 'dcn' | 'rnd' | 'run' | 'str';

export const idFromBytes = (prefix: MainGeneratedIdPrefix, bytes: Uint8Array): string =>
  encodeId(prefix, bytes);

export const createAuraId = (prefix: MainGeneratedIdPrefix): string =>
  idFromBytes(prefix, randomBytes(16));

export const idToBytes = (prefix: MainGeneratedIdPrefix, value: string): Buffer => {
  const expected = `${prefix}_`;
  if (!value.startsWith(expected) || value.length !== expected.length + 26) {
    throw new RangeError(`Aura ID must use the ${prefix}_ prefix and a 26-character ULID`);
  }
  let numeric = 0n;
  for (const character of value.slice(expected.length)) {
    const digit = CROCKFORD.indexOf(character);
    if (digit < 0) throw new RangeError('Aura ID contains a non-canonical ULID character');
    numeric = (numeric << 5n) | BigInt(digit);
  }
  if (numeric >= 1n << 128n) throw new RangeError('Aura ID exceeds 128 bits');
  const bytes = Buffer.alloc(16);
  for (let index = bytes.length - 1; index >= 0; index -= 1) {
    bytes[index] = Number(numeric & 0xffn);
    numeric >>= 8n;
  }
  return bytes;
};
