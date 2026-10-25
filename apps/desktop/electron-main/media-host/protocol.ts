import type { MediaCommand, MediaIdentity } from './types.js';

export const record = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
export const exact = (value: Record<string, unknown>, keys: string): boolean =>
  Object.keys(value).sort().join(',') === keys.split(',').sort().join(',');
export const integer = (value: unknown, min: number, max: number): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max;

export function mediaIdentity(value: unknown, runtime: string): MediaIdentity | undefined {
  if (!record(value)) return undefined;
  const { command_id, runtime_instance_id, session_id } = value;
  return typeof command_id === 'string' &&
    /^mcm-[A-Za-z0-9_-]{1,128}$/u.test(command_id) &&
    runtime_instance_id === runtime &&
    typeof runtime_instance_id === 'string' &&
    typeof session_id === 'string' &&
    /^ses_[0-9A-HJKMNP-TV-Z]{26}$/u.test(session_id)
    ? { command_id, runtime_instance_id, session_id }
    : undefined;
}
export function readMediaCommand(value: unknown, runtime: string): MediaCommand | undefined {
  if (
    !record(value) ||
    !mediaIdentity(value, runtime) ||
    !exact(
      value,
      'command_id,runtime_instance_id,session_id,file_size,media_type,timeout_ms,action',
    ) ||
    !integer(value.file_size, 1, Number.MAX_SAFE_INTEGER) ||
    !integer(value.timeout_ms, 1, 30_000) ||
    typeof value.media_type !== 'string' ||
    value.media_type.length > 128 ||
    !/^(?:video\/[a-z0-9.+-]+|application\/octet-stream)$/iu.test(value.media_type) ||
    !record(value.action)
  )
    return undefined;
  const action = value.action;
  if (action.kind === 'probe') {
    if (!exact(action, 'kind')) return undefined;
  } else if (action.kind === 'frames') {
    if (
      !exact(action, 'kind,times,count,sheet,max_side') ||
      typeof action.sheet !== 'boolean' ||
      !integer(action.max_side, 64, 2048)
    )
      return undefined;
    if (action.times === null) {
      if (!integer(action.count, 1, 12)) return undefined;
    } else if (
      action.count !== null ||
      !Array.isArray(action.times) ||
      action.times.length < 1 ||
      action.times.length > 12 ||
      !action.times.every((time) => typeof time === 'number' && Number.isFinite(time) && time >= 0)
    )
      return undefined;
  } else return undefined;
  return value as unknown as MediaCommand;
}
