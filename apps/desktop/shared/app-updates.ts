// Aura's own updates: what the window can ask Main to do, and the state Main reports back.

export const APP_UPDATE_CHANNEL = 'aura:v1:app-update';
export const APP_UPDATE_STATE_CHANNEL = 'aura:v1:app-update-state';

export type AppUpdateCommand =
  | { command: 'state' }
  | { command: 'check' }
  | { command: 'download' }
  | { command: 'pause' }
  | { command: 'restart' }
  | { command: 'roll_back'; restore_data: boolean }
  | { command: 'dismiss_notice' }
  | { command: 'set_auto_check'; enabled: boolean };

export type AppUpdatePhase =
  | 'idle'
  | 'checking'
  | 'up_to_date'
  | 'available'
  | 'downloading'
  | 'paused'
  | 'applying'
  | 'ready'
  | 'error';

export interface AppUpdateRelease {
  version: string;
  published_at: string | null;
  size: number;
  notes: string;
}

/** A version the launcher tried and put back. */
export interface AppUpdateNotice {
  version: string;
  kept_version: string;
  detail: string;
  /** The failed version was older: going back, which may need the data from before the update. */
  going_back: boolean;
  /** The data from before the current version first started is still there to put back. */
  data_restorable: boolean;
}

export interface AppUpdateState {
  /** False when this Aura cannot update itself (development, or no installed setup program). */
  available: boolean;
  current: string;
  previous: string | null;
  /** A version that waits for the next start. */
  waiting: string | null;
  phase: AppUpdatePhase;
  latest: AppUpdateRelease | null;
  progress: { done: number; total: number } | null;
  error: string | null;
  checked_at: string | null;
  auto_check: boolean;
  notice: AppUpdateNotice | null;
}

const PHASES: readonly AppUpdatePhase[] = [
  'idle',
  'checking',
  'up_to_date',
  'available',
  'downloading',
  'paused',
  'applying',
  'ready',
  'error',
];

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const isText = (value: unknown): value is string => typeof value === 'string';
const isTextOrNull = (value: unknown): value is string | null => value === null || isText(value);
const isCount = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0;

export function isAppUpdateCommand(value: unknown): value is AppUpdateCommand {
  if (!isRecord(value) || !isText(value.command)) return false;
  const keys = Object.keys(value).sort().join(',');
  switch (value.command) {
    case 'state':
    case 'check':
    case 'download':
    case 'pause':
    case 'restart':
    case 'dismiss_notice':
      return keys === 'command';
    case 'roll_back':
      return keys === 'command,restore_data' && typeof value.restore_data === 'boolean';
    case 'set_auto_check':
      return keys === 'command,enabled' && typeof value.enabled === 'boolean';
    default:
      return false;
  }
}

export const UNAVAILABLE_UPDATE_STATE: AppUpdateState = {
  available: false,
  current: '',
  previous: null,
  waiting: null,
  phase: 'idle',
  latest: null,
  progress: null,
  error: null,
  checked_at: null,
  auto_check: false,
  notice: null,
};

function readRelease(value: unknown): AppUpdateRelease | null | undefined {
  if (value === null) return null;
  if (
    isRecord(value) &&
    isText(value.version) &&
    isTextOrNull(value.published_at) &&
    isCount(value.size) &&
    isText(value.notes)
  ) {
    return {
      version: value.version,
      published_at: value.published_at,
      size: value.size,
      notes: value.notes,
    };
  }
  return undefined;
}

function readNotice(value: unknown): AppUpdateNotice | null | undefined {
  if (value === null) return null;
  if (
    isRecord(value) &&
    isText(value.version) &&
    isText(value.kept_version) &&
    isText(value.detail) &&
    typeof value.going_back === 'boolean' &&
    typeof value.data_restorable === 'boolean'
  ) {
    return {
      version: value.version,
      kept_version: value.kept_version,
      detail: value.detail,
      going_back: value.going_back,
      data_restorable: value.data_restorable,
    };
  }
  return undefined;
}

/** The state Main sent, or the unavailable state when it does not have the expected shape. */
export function readAppUpdateState(value: unknown): AppUpdateState {
  if (!isRecord(value)) return UNAVAILABLE_UPDATE_STATE;
  const latest = readRelease(value.latest);
  const notice = readNotice(value.notice);
  const progress =
    value.progress === null
      ? null
      : isRecord(value.progress) && isCount(value.progress.done) && isCount(value.progress.total)
        ? { done: value.progress.done, total: value.progress.total }
        : undefined;
  if (
    typeof value.available !== 'boolean' ||
    !isText(value.current) ||
    !isTextOrNull(value.previous) ||
    !isTextOrNull(value.waiting) ||
    !PHASES.includes(value.phase as AppUpdatePhase) ||
    latest === undefined ||
    notice === undefined ||
    progress === undefined ||
    !isTextOrNull(value.error) ||
    !isTextOrNull(value.checked_at) ||
    typeof value.auto_check !== 'boolean'
  ) {
    return UNAVAILABLE_UPDATE_STATE;
  }
  return {
    available: value.available,
    current: value.current,
    previous: value.previous,
    waiting: value.waiting,
    phase: value.phase as AppUpdatePhase,
    latest,
    progress,
    error: value.error,
    checked_at: value.checked_at,
    auto_check: value.auto_check,
    notice,
  };
}
