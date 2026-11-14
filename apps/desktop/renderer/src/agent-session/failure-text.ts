import { userMessage } from './user-message';

/** A field of an error envelope, an Error or any other object that carries one. */
const field = (value: unknown, name: 'code' | 'message'): string | undefined => {
  if (typeof value !== 'object' || value === null) return undefined;
  const found = (value as Record<string, unknown>)[name];
  return typeof found === 'string' ? found : undefined;
};

/** A request Core did not answer in time, or whose connection dropped while it was out. */
const NO_REPLY = ['ipc.request_timeout', 'request_timeout', 'Core request timed out'] as const;
const DROPPED = [
  'ipc.transport_lost',
  'Aura Core pipe disconnected',
  'Transport is closed',
] as const;

/** Aura's own connection to Core, by Main's codes and, for a thrown error, its message. */
const CAUSES: readonly (readonly [readonly string[], string])[] = [
  [NO_REPLY, 'Aura Core 没有及时回复'],
  [DROPPED, '与 Aura Core 的连接断开了'],
  [['ipc.core_not_ready'], 'Aura Core 还没有就绪'],
  [['ipc.local_overload'], '同时进行的操作太多'],
  [['ipc.request_not_sent'], '请求没有发出'],
];

/** Why a request failed when Aura's connection to Core is why, as a clause for a sentence. */
const matches = (error: unknown, keys: readonly string[]): boolean => {
  const code = field(error, 'code');
  const text = typeof error === 'string' ? error : (field(error, 'message') ?? '');
  return keys.some((key) => key === code || (code === undefined && text.startsWith(key)));
};

export function connectionCause(error: unknown): string | undefined {
  return CAUSES.find(([keys]) => matches(error, keys))?.[1];
}

/**
 * Whether a request went out but no answer came, so what it asked for may have been done; one
 * Main says was never sent is not.
 */
export function answerMissing(error: unknown): boolean {
  const details =
    typeof error === 'object' && error !== null
      ? (error as { details?: { outcome_recovery?: unknown } }).details
      : undefined;
  if (details?.outcome_recovery === 'not_sent') return false;
  return matches(error, NO_REPLY) || matches(error, DROPPED);
}

/** Whether the request was refused because what it acted on changed elsewhere first. */
export function isConflict(error: unknown): boolean {
  const code = field(error, 'code') ?? '';
  return /(?:revision_conflict|revision_stale|revision_mismatch|_conflict|\.stale|_stale)$/u.test(
    code,
  );
}

/**
 * One action that failed, in a sentence: `conflict` when what it acted on changed first, as it
 * says more than Core's general sentence for a change made elsewhere; else a known product
 * sentence or Core's own sentence when there is one; else what did not happen, why when Aura's
 * connection to Core is why, and `rest`: what that means and what to do next.
 */
export function failedAction(
  error: unknown,
  what: string,
  rest: string,
  options: { conflict?: string } = {},
): string {
  if (options.conflict !== undefined && isConflict(error)) return options.conflict;
  const cause = connectionCause(error);
  return userMessage(error, cause ? `${what}（${cause}），${rest}` : `${what}，${rest}`);
}

/**
 * `what` did not happen: Core's own sentence after a colon when it has one; else why in brackets
 * (`conflictCause` when what it acted on changed elsewhere first, or Aura's connection to Core),
 * then `rest`: what that means and what to do next.
 */
export function refusedAction(
  error: unknown,
  what: string,
  rest: string,
  options: { conflictCause?: string } = {},
): string {
  const cause =
    options.conflictCause !== undefined && isConflict(error)
      ? options.conflictCause
      : connectionCause(error);
  const said = cause === undefined && error !== undefined ? userMessage(error, '') : '';
  if (said) return `${what}：${said}`;
  return `${what}${cause ? `（${cause}）` : ''}，${rest}`;
}
