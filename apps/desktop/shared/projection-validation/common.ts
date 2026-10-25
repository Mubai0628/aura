export const isTimestamp = (value: unknown): boolean =>
  typeof value === 'string' && !Number.isNaN(Date.parse(value));

export type AuraIdPrefix =
  | 'agt'
  | 'cps'
  | 'crd'
  | 'end'
  | 'ers'
  | 'lin'
  | 'mav'
  | 'mdl'
  | 'mdr'
  | 'pbs'
  | 'rnd'
  | 'run'
  | 'ses'
  | 'skl'
  | 'stp'
  | 'tcl'
  | 'tol'
  | 'wlo'
  | 'wsp';

export const isAuraId = (value: unknown, prefix: AuraIdPrefix): value is string =>
  typeof value === 'string' &&
  new RegExp(`^${prefix}_[0-7][0-9A-HJKMNP-TV-Z]{25}$`, 'u').test(value);

export const isContentHash = (value: unknown): boolean =>
  exactRecord(value, ['algorithm', 'hex']) &&
  (value.algorithm === 'blake3' || value.algorithm === 'sha256') &&
  typeof value.hex === 'string' &&
  /^[0-9a-f]{64}$/u.test(value.hex);

export const isRevision = (value: unknown): value is string =>
  typeof value === 'string' && /^(0|[1-9][0-9]*)$/u.test(value);

export const isSafeUnsigned = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;

export const exactRecord = <Keys extends readonly string[]>(
  value: unknown,
  keys: Keys,
): value is Record<Keys[number], unknown> => recordWithOptional(value, keys, []);

export const recordWithOptional = <
  Required extends readonly string[],
  Optional extends readonly string[],
>(
  value: unknown,
  required: Required,
  optional: Optional,
): value is Record<Required[number], unknown> & Partial<Record<Optional[number], unknown>> => {
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  if (!required.every((key) => keys.includes(key))) return false;
  const allowed = new Set<string>([...required, ...optional]);
  return keys.every((key) => allowed.has(key));
};

export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** The thinking levels a model profile may name. */
export const REASONING_EFFORTS: ReadonlySet<unknown> = new Set([
  'off',
  'minimal',
  'low',
  'medium',
  'high',
  'xhigh',
  // Its own level now, not another name for xhigh.
  'max',
]);
