/** Exact-shape record checks every validator builds on. */
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
