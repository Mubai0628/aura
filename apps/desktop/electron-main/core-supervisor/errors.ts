import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { type CoreStartupFailureStage } from '../../shared/core-startup-diagnostics.js';
import { isRecord } from './handshake.js';

/** How a startup, reconnect or confirmed command fails, and how a failure is read. */
export class CompatibilityFailure extends Error {}

export class PairingInvalidated extends Error {}

/**
 * A command whose outcome the automatic confirmation CONFIRMED: the Core's own stored error for that
 * invocation, carried as this operation's failure.
 *
 * The envelope is the real one - the code, the terminal, the message and the side-effect description
 * the Core recorded - so a confirmed failure or cancellation reaches the Renderer with its real
 * semantics instead of being downgraded to "perhaps it ran" or replaced by a synthesized internal
 * error. It is deliberately NOT the transport's timeout envelope: a confirmed terminal is definite,
 * and `isUnconfirmedCommandOutcome` therefore reads false for it.
 */
export class ConfirmedCommandFailure extends Error {
  readonly envelope: IpcErrorEnvelope;

  constructor(envelope: IpcErrorEnvelope) {
    super(envelope.message);
    this.name = 'ConfirmedCommandFailure';
    this.envelope = envelope;
  }
}

/**
 * The recovery's absolute deadline was spent while a handshake half or a
 * projection-sync stage was still pending. Deliberately distinct from an RPC
 * failure: on this the channel is closed, the recovery owner is cancelled, and
 * the recovery ends on its budget instead of retrying the stage.
 */
export class ReconnectDeadlineExpired extends Error {
  readonly code = 'ERR_AURA_CORE_RECONNECT_DEADLINE';

  constructor() {
    super('Aura Core reconnect deadline expired');
    this.name = 'ReconnectDeadlineExpired';
  }
}

/**
 * Same-process recovery spent its whole monotonic budget without an authenticated
 * channel.
 *
 * A per-attempt timeout bounds one attempt; it is not a bound on recovery. Without this
 * the loop could keep starting attempts while the window still said "recovering", which
 * is the failure the incident showed in a different place: not a wrong answer, but no
 * answer. It is deliberately a distinct error from {@link PairingInvalidated}, because
 * the recovery is the opposite one — a pairing that changed is retried with a fresh
 * Core, while an exhausted budget means this Core has to go through the existing
 * controlled exit and restart path.
 */
export class ReconnectBudgetExhausted extends Error {
  readonly attempts: number;
  readonly budgetMilliseconds: number;
  readonly lastError: unknown;

  constructor(budgetMilliseconds: number, attempts: number, lastError: unknown) {
    super(
      `Aura Core reconnect did not finish within ${budgetMilliseconds}ms after ${attempts} attempt(s)`,
    );
    this.name = 'ReconnectBudgetExhausted';
    this.budgetMilliseconds = budgetMilliseconds;
    this.attempts = attempts;
    this.lastError = lastError;
  }
}

/** Stage costs of one startup attempt, recorded next to the failure reason. */
export interface CoreStartupTimings {
  connectMilliseconds: number;
  handshakeMilliseconds: number;
  projectionSyncMilliseconds: number;
  totalMilliseconds: number;
}

/**
 * A startup budget expired before the Core became reachable. The last transport
 * error stays only as the cause: `describeCoreStartupFailure` persists the
 * classified code, never the raw message.
 */
export const startupAttemptEndedError = (): Error =>
  Object.assign(new Error('Aura Core startup attempt ended'), {
    code: 'ERR_AURA_CORE_START_ABORTED',
  });

export const startupFailureIsAbandoned = (error: unknown): boolean =>
  error instanceof CoreStartupStageFailure &&
  isRecord(error.originalError) &&
  error.originalError.code === 'ERR_AURA_CORE_START_ABORTED';

export const startupBudgetExceededError = (
  phase: 'connect',
  milliseconds: number,
  cause: unknown,
): Error =>
  Object.assign(new Error(`Aura Core ${phase} did not complete within ${milliseconds}ms`), {
    code: 'ERR_AURA_CORE_START_TIMEOUT',
    cause,
  });

/** The Core process exited before it ever published its pipe. */
export const coreExitBeforeReadyError = (exit: {
  code: number | null;
  signal: NodeJS.Signals | null;
}): Error =>
  Object.assign(new Error('Aura Core exited before it became reachable'), {
    code: 'ERR_AURA_CORE_EXITED_EARLY',
    exitCode: exit.code,
    exitSignal: exit.signal,
  });

/** Only a classified code is copied out of an abandoned attempt. */
export const readErrorCode = (error: unknown): string | undefined =>
  isRecord(error) && typeof error.code === 'string' && /^[A-Z][A-Z0-9_]{1,31}$/u.test(error.code)
    ? error.code
    : undefined;

/**
 * The typed envelope of a rejected request, read structurally.
 *
 * A message is deliberately not part of this: it can carry a pipe path or a bootstrap nonce, and
 * the classification only needs the fields the transport itself set.
 */
export const readFailureEnvelope = (
  error: unknown,
): { code: string; terminal: string } | undefined => {
  if (!isRecord(error) || !isRecord(error.envelope)) return undefined;
  const { code, terminal } = error.envelope;
  return typeof code === 'string' && typeof terminal === 'string' ? { code, terminal } : undefined;
};

/**
 * Extra, already classified startup detail for the failure log. Only the exit
 * status and a system error code are copied: raw messages can contain a pipe
 * path or a bootstrap nonce and are never persisted.
 */
export const startupFailureDetail = (error: unknown): Record<string, unknown> => {
  const original = error instanceof CoreStartupStageFailure ? error.originalError : error;
  if (!isRecord(original)) return {};
  const detail: Record<string, unknown> = {};
  if (typeof original.exitCode === 'number' || original.exitCode === null) {
    detail.coreExitCode = original.exitCode;
  }
  if (typeof original.exitSignal === 'string') detail.coreExitSignal = original.exitSignal;
  const cause = original.cause;
  if (isRecord(cause) && typeof cause.code === 'string' && /^E[A-Z0-9_]{1,31}$/u.test(cause.code)) {
    detail.causeCode = cause.code;
  }
  return detail;
};

export class CoreStartupStageFailure extends Error {
  readonly stage: CoreStartupFailureStage;

  constructor(
    stage: CoreStartupFailureStage,
    readonly originalError: unknown,
  ) {
    super('Aura Core startup failed');
    this.name = 'CoreStartupStageFailure';
    this.stage = stage;
  }
}
