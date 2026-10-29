import { exactRecord, isSafeUnsigned, isTimestamp, recordWithOptional } from './common.js';

/** The long work that holds Core's stores. */
const BUSY_ACTIVITIES = new Set(['deleting_session', 'removing_workspace', 'finishing_deletion']);

const isCoreBusy = (value: unknown): boolean =>
  exactRecord(value, ['activity', 'since']) &&
  typeof value.activity === 'string' &&
  BUSY_ACTIVITIES.has(value.activity) &&
  isTimestamp(value.since);

/** Main's wait for Core's heartbeat, since the last one it received. */
const isCoreWaiting = (value: unknown): boolean =>
  exactRecord(value, ['since']) && isTimestamp(value.since);

/**
 * The Core health payload; `busy` is present only while long work holds the stores, `waiting`
 * only while Main waits for Core's heartbeat.
 */
export const isCoreHealthPayload = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['read_only', 'restart_attempt', 'safe_mode', 'state', 'status_code', 'status_message'],
    ['busy', 'waiting'],
  ) &&
  typeof value.state === 'string' &&
  typeof value.read_only === 'boolean' &&
  typeof value.safe_mode === 'boolean' &&
  isSafeUnsigned(value.restart_attempt) &&
  typeof value.status_code === 'string' &&
  typeof value.status_message === 'string' &&
  (value.busy === undefined || value.busy === null || isCoreBusy(value.busy)) &&
  (value.waiting === undefined || value.waiting === null || isCoreWaiting(value.waiting));
