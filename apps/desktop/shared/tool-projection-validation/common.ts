import { exactRecord, recordWithOptional, isRecord } from '../projection-validation/common.js';

export const isCanonicalHash = (value: unknown, algorithm: 'blake3' | 'sha256'): boolean =>
  exactRecord(value, ['algorithm', 'canonicalization_version', 'hex']) &&
  value.algorithm === algorithm &&
  value.canonicalization_version === 'rfc8785_jcs_v1' &&
  typeof value.hex === 'string' &&
  /^[0-9a-f]{64}$/u.test(value.hex);

export const isToolProjectionError = (value: unknown): boolean =>
  exactRecord(value, ['category', 'code', 'message', 'retryable']) &&
  typeof value.category === 'string' &&
  typeof value.code === 'string' &&
  value.code.startsWith('tool.') &&
  typeof value.message === 'string' &&
  typeof value.retryable === 'boolean';

export const isOptionalNullable = (
  value: unknown,
  validator: (candidate: unknown) => boolean,
): boolean => value === undefined || value === null || validator(value);

export const isToolActivityCursor = (value: unknown): boolean =>
  typeof value === 'string' &&
  value.length > 0 &&
  value.length <= 4_096 &&
  /^[A-Za-z0-9._-]+$/u.test(value);

export const isToolVersion = (value: unknown): boolean =>
  recordWithOptional(value, ['definition_revision'], ['semantic_version']) &&
  isRevision(value.definition_revision) &&
  isOptionalNullableString(value.semantic_version);

export const isToolRuntimeKey = (value: unknown): boolean =>
  typeof value === 'string' &&
  value.length > 0 &&
  value.length <= 64 &&
  /^[a-z0-9]+(?:[-_][a-z0-9]+)*$/u.test(value);

export const isJsonValue = (value: unknown, depth = 0): boolean => {
  if (depth > 64) return false;
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return true;
  if (typeof value === 'number') return Number.isFinite(value);
  if (Array.isArray(value)) return value.every((item) => isJsonValue(item, depth + 1));
  if (!isRecord(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return (
    (prototype === Object.prototype || prototype === null) &&
    Object.values(value).every((item) => isJsonValue(item, depth + 1))
  );
};

export const isToolOutcome = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  if (value.type === 'succeeded' || value.type === 'failed') {
    return exactRecord(value, ['type']);
  }
  if (value.type === 'cancelled') {
    return exactRecord(value, ['reason', 'type']) && typeof value.reason === 'string';
  }
  if (value.type === 'not_executed' || value.type === 'indeterminate') {
    return exactRecord(value, ['reason', 'type']) && typeof value.reason === 'string';
  }
  return false;
};

export const isToolDraftStatus = (value: unknown): boolean =>
  isOneOf(value, [
    'editing',
    'validating',
    'validation_failed',
    'ready_for_test',
    'testing',
    'test_failed',
    'ready_to_apply',
    'applying',
    'applied',
    'discarded',
    'recovery_required',
  ]);

export const isToolBatchStatus = (value: unknown): boolean =>
  isOneOf(value, [
    'pending_admission',
    'admitted',
    'running',
    'cancelling',
    'completed',
    'recovery_required',
  ]);

export const isToolCallStatus = (value: unknown): boolean =>
  isOneOf(value, [
    'pending_admission',
    'waiting_dependency',
    'waiting_permission',
    'waiting_approval',
    'waiting_resource',
    'queued',
    'running',
    'cancelling',
    'finalized',
    'recovery_required',
  ]);

export const isConformanceStatus = (value: unknown): boolean =>
  isOneOf(value, ['queued', 'running', 'passed', 'failed', 'cancelled', 'recovery_required']);

export const isSideEffectState = (value: unknown): boolean =>
  isOneOf(value, [
    'not_started',
    'no_effect',
    'fully_applied',
    'partially_applied',
    'compensated',
    'unknown',
  ]);

export const isOptionalNullableString = (value: unknown): boolean =>
  value === undefined || value === null || typeof value === 'string';

export const isOptionalNullableRecord = (value: unknown): boolean =>
  value === undefined || value === null || isRecord(value);

export const isTimestamp = (value: unknown): boolean =>
  typeof value === 'string' && !Number.isNaN(Date.parse(value));

export const isRevision = (value: unknown): value is string =>
  typeof value === 'string' && /^(0|[1-9][0-9]*)$/u.test(value);

export const isPositiveRevision = (value: unknown): value is string =>
  typeof value === 'string' && /^[1-9][0-9]*$/u.test(value);

export const isSafeUnsigned = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;

export const isAuraId = (value: unknown, prefix: string): value is string =>
  typeof value === 'string' &&
  new RegExp(`^${prefix}_[0-7][0-9A-HJKMNP-TV-Z]{25}$`, 'u').test(value);

export const isOneOf = <Value extends string>(
  value: unknown,
  allowed: readonly Value[],
): value is Value => typeof value === 'string' && allowed.includes(value as Value);
