import { AURA_ID_PREFIXES_V1 } from './generated/id-prefixes.js';

const ID_PATTERN = /^(?<prefix>[a-z]{3})_(?<ulid>[0-9A-HJKMNP-TV-Z]{26})$/;
const KNOWN_PREFIXES: ReadonlySet<string> = new Set(AURA_ID_PREFIXES_V1);

export interface ParsedAuraId {
  prefix: string;
  ulid: string;
}

export const parseAuraId = (value: string): ParsedAuraId => {
  const match = ID_PATTERN.exec(value);
  const prefix = match?.groups?.prefix;
  const ulid = match?.groups?.ulid;
  if (prefix === undefined || ulid === undefined || !KNOWN_PREFIXES.has(prefix)) {
    throw new TypeError(`Invalid Aura ID: ${value}`);
  }
  return { prefix, ulid };
};

export const compareAuraIds = (left: string, right: string): number => {
  const parsedLeft = parseAuraId(left);
  const parsedRight = parseAuraId(right);
  if (parsedLeft.prefix !== parsedRight.prefix) {
    throw new TypeError('Aura IDs can only be ordered within the same strong ID type.');
  }
  return parsedLeft.ulid.localeCompare(parsedRight.ulid, 'en-US');
};
