/**
 * Windows notifications Aura shows: a new version, and what a conversation the user is not
 * looking at needs or has done. Each kind is a switch in 设置 → 通知, all on until turned off.
 */
export const NOTIFICATION_PREFERENCES_CHANNEL = 'aura:v1:notification-preferences';
/** The renderer says which conversation is in front, or none. */
export const NOTIFICATION_FOREGROUND_CHANNEL = 'aura:v1:notification-foreground';
/** Main asks the window to show what a clicked notification was about. */
export const NOTIFICATION_OPEN_CHANNEL = 'aura:v1:notification-open';

export interface NotificationPreferences {
  /** A new version of Aura can be downloaded. */
  updates: boolean;
  /** A conversation waits for the user to approve an operation. */
  approvals: boolean;
  /** A conversation asks the user a question, or waits for a decision. */
  questions: boolean;
  /** A conversation finished its task. */
  completions: boolean;
  /** A conversation stopped on an error, or paused where it cannot go on by itself. */
  failures: boolean;
}

export const NOTIFICATION_KINDS = [
  'updates',
  'approvals',
  'questions',
  'completions',
  'failures',
] as const satisfies readonly (keyof NotificationPreferences)[];

export const DEFAULT_NOTIFICATION_PREFERENCES: NotificationPreferences = {
  updates: true,
  approvals: true,
  questions: true,
  completions: true,
  failures: true,
};

/** Preferences in exactly their shape; a missing or wrong switch is its default. */
export function readNotificationPreferences(value: unknown): NotificationPreferences {
  const record =
    typeof value === 'object' && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {};
  const read = { ...DEFAULT_NOTIFICATION_PREFERENCES };
  for (const kind of NOTIFICATION_KINDS)
    if (typeof record[kind] === 'boolean') read[kind] = record[kind];
  return read;
}

export type NotificationPreferencesCommand =
  { kind: 'get' } | { kind: 'set'; preferences: NotificationPreferences };

export function isNotificationPreferencesCommand(
  value: unknown,
): value is NotificationPreferencesCommand {
  if (typeof value !== 'object' || value === null) return false;
  const command = value as Record<string, unknown>;
  if (command.kind === 'get') return Object.keys(command).length === 1;
  if (command.kind !== 'set' || Object.keys(command).length !== 2) return false;
  const preferences = command.preferences;
  return (
    typeof preferences === 'object' &&
    preferences !== null &&
    Object.keys(preferences).length === NOTIFICATION_KINDS.length &&
    NOTIFICATION_KINDS.every(
      (kind) => typeof (preferences as Record<string, unknown>)[kind] === 'boolean',
    )
  );
}

/** What a clicked notification opens. */
export type NotificationTarget = { kind: 'session'; sessionId: string } | { kind: 'updates' };

/** What is in front while 设置 → 关于与更新 shows, in place of a conversation. */
export const UPDATES_IN_FRONT = 'aura:updates';

export function readNotificationTarget(value: unknown): NotificationTarget | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const target = value as Record<string, unknown>;
  if (target.kind === 'updates') return { kind: 'updates' };
  return target.kind === 'session' && typeof target.sessionId === 'string'
    ? { kind: 'session', sessionId: target.sessionId }
    : undefined;
}

export interface NotificationsApiV1 {
  notificationPreferences(
    command: NotificationPreferencesCommand,
  ): Promise<NotificationPreferences>;
  /** The conversation in front, or none; a notification about it is not shown. */
  setForegroundSession(sessionId: string | null): void;
  onNotificationOpen(listener: (target: NotificationTarget) => void): () => void;
}
