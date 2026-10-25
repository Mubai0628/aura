/**
 * The bounded confirmation of ONE write's real outcome.
 *
 * THE DECISION THIS FILE IMPLEMENTS: a write whose caller stopped waiting is
 * automatically confirmed by ONE bounded `runtime.command_outcome` query, the EXISTING operation
 * state is updated with the authoritative result, and the write is never sent again. A timeout is
 * not a cancellation and never becomes one.
 *
 * WHAT IT IS NOT: it is not a retry, not a replay, not a second write, and not a guess. The module
 * has no way to send anything except the query: the caller's `send` is invoked exactly once, by the
 * caller, before this module is consulted at all.
 *
 * WHO OWNS WHAT:
 *  - The CALLER (the normal command entry point in `core-supervisor.ts`) owns the single write
 *    invocation, the transport that carried it, and the projection of a confirmed success back into
 *    the operation's existing success path.
 *  - THIS MODULE owns the entry decision, the ONE total budget, the finite attempt count, the finite
 *    concurrency, the per-operation record, and the verification of a `confirmed` answer.
 *  - Each `confirm()` call is one operation's own confirmation: it holds its own deadline and its own
 *    record, and it can only ever return into the promise that started it. There is no shared result
 *    slot a late answer could land in, so a late result cannot overwrite a later operation.
 *
 * WHAT A `confirmed` ANSWER MUST SATISFY before it is allowed to update anything (all four, or the
 * answer is refused and the original pending-confirmation state is kept):
 *  1. the invocation: `outcome.command_invocation_id` is the SAME id the transport put on the
 *     original frame (it is read from that failure's own envelope, never regenerated);
 *  2. the terminal: the outcome's `terminal` agrees with the terminal stored inside the response,
 *     extracted the way Core extracts it in `record_command_outcome`;
 *  3. the original command's response shape: the stored response is a JSON-RPC response addressed to
 *     the ORIGINAL correlation id, with exactly one terminal (`result` XOR `error`) and a
 *     well-formed typed error envelope when it is a failure - a response for another request, or a
 *     malformed one, belongs to nobody and is refused;
 *  4. the operation's own projection: a confirmed success value is handed to the same projection the
 *     normal success path applies (for the Agent surface that is the generated contract validator),
 *     so a confirmed success cannot bypass a check that a normal reply passes.
 *
 * WHAT KEEPS THE PENDING-CONFIRMATION STATE: `unknown`/`not_recorded_or_incomplete`, a failed query,
 * and an exhausted budget. None of them is ever reported as "the command did not execute" or as
 * "safe to retry": they are returned as `unconfirmed`, and the caller rethrows the ORIGINAL failure,
 * which is the one that carries the query-without-replay instruction.
 *
 * ENTRY GATES (a request that fails any of these is not confirmed, and nothing is sent):
 *  - the failure carries a typed envelope at all;
 *  - the failure is NOT the transport's own `not_sent` declaration (nothing reached the writer, so
 *    there is nothing to confirm);
 *  - the failure says it is unconfirmed (`isUnconfirmedCommandOutcome`: the query-without-replay
 *    hint, an `outcome_unknown` terminal, a channel terminal, or a bridge-level unconfirmed code);
 *  - the route is a KNOWN Core command that is not `read_only`. The transport mints a
 *    CommandInvocationId for every request, reads included, so the invocation id alone cannot decide
 *    this; `coreRoute(method).idempotency` is the contract fact that does, and it is the same
 *    predicate Core itself uses (`aura_ipc::routes::side_effecting`) to decide whether an outcome is
 *    recorded at all. Querying a `read_only` route can only ever answer `unknown`;
 *  - the route is not one of the two internal CONTROL routes. `aura.cancel` and
 *    `runtime.command_outcome` are control routes, and a confirmation must never recursively query a
 *    control request;
 *  - the CommandInvocationId and the correlation id are both canonically well-formed. A missing or
 *    malformed identity is not a confirmation, it is a missing measurement.
 */

import type {
  CommandInvocationId,
  CommandOutcome,
  CorrelationId,
  ErrorCategory,
  IpcErrorEnvelope,
  IpcRequestTerminal,
} from '@aura/contracts/generated';

import { coreRoute } from '../shared/generated/routes.js';
import {
  isUnconfirmedCommandOutcome,
  OUTCOME_RECOVERY_NOT_SENT,
} from '../shared/ipc-request-failure.js';
import { idToBytes } from './ids.js';
import { settleWithin } from './startup-attempt.js';

/**
 * The central declaration of every bound the automatic confirmation spends.
 *
 * These are internal engineering values, chosen once here, and every one of them is a BOUND rather
 * than a schedule:
 *  - `totalBudgetMs` is the SINGLE deadline of one operation's whole confirmation, measured on the
 *    monotonic clock. It is never re-initialised across an await: the retry pause, a wait for a
 *    reconnecting channel and every query attempt are all spent out of this one number, so the
 *    caller's extra wait is bounded no matter how the attempts interleave.
 *  - `attemptBudgetMs` is what ONE query attempt may spend. Deliberately smaller than the total so a
 *    second attempt can still fit inside the same deadline, and deliberately smaller than the
 *    control route's own 10 s transport budget so the caller stops waiting before the transport
 *    does. A query this module abandons is still bounded by the transport's own budget and by the
 *    control pool's own capacity; it is never left unbounded.
 *  - `maxAttempts` is finite on purpose: an outcome that was not recorded yet may be recorded a
 *    moment later, and a query that failed may succeed once, but this must never become polling.
 *  - `retryDelayMs` is the pause between attempts. It is spent from the same deadline, never added
 *    to it.
 *  - `maxConcurrent` bounds how many operations in this Main process may hold a confirmation at
 *    once. An operation that cannot get a slot within its own remaining budget keeps the
 *    pending-confirmation state instead of queueing forever, so a storm of timeouts cannot turn into
 *    an unbounded number of control queries.
 */
export const COMMAND_OUTCOME_CONFIRMATION_POLICY = {
  attemptBudgetMs: 4_000,
  maxAttempts: 2,
  maxConcurrent: 4,
  retryDelayMs: 250,
  totalBudgetMs: 6_000,
} as const;

export interface CommandOutcomeConfirmationPolicy {
  readonly attemptBudgetMs: number;
  readonly maxAttempts: number;
  readonly maxConcurrent: number;
  readonly retryDelayMs: number;
  readonly totalBudgetMs: number;
}

/**
 * The two internal control routes, refused a second time and independently of the transport's own
 * private table. Neither is reachable through a normal command entry point today; this is here so
 * that reaching one can never start a confirmation of itself.
 */
const CONTROL_ROUTES: ReadonlySet<string> = new Set(['aura.cancel', 'runtime.command_outcome']);

/** Why a failed request is not confirmed. Every reason is a deliberate non-start, not an error. */
export type ConfirmationSkipReason =
  | 'control-route'
  | 'failure-not-unconfirmed'
  | 'invocation-unusable'
  | 'no-failure-envelope'
  | 'not-sent'
  | 'route-not-a-command'
  | 'route-unknown';

/** How one operation's confirmation ended. Reported in the operation's own record. */
export type ConfirmationOutcome =
  | 'budget-exhausted'
  | 'concurrency-refused'
  | 'confirmed-cancelled'
  | 'confirmed-failed'
  | 'confirmed-succeeded'
  | 'outcome-unknown'
  | 'query-failed'
  | 'verification-refused';

/** One operation's confirmation record. Identity, counts and classification only - never a value. */
export interface ConfirmationRecord {
  readonly attempts: number;
  readonly budgetMs: number;
  readonly command: string;
  readonly commandInvocationId: CommandInvocationId;
  readonly correlationId: CorrelationId;
  readonly elapsedMs: number;
  /** The classified code of the last failed query attempt, never its message. */
  readonly lastFailureCode?: string | undefined;
  readonly outcome: ConfirmationOutcome;
  /**
   * The classified code of the ORIGINAL failure that made this operation eligible for confirmation -
   * the failure the normal entry point caught, not a query failure and not a message. It is what makes
   * "this operation really failed locally first" a fact of the product's own record instead of
   * something only the caller ever saw.
   */
  readonly originalFailureCode: string;
  /** Which authenticated channel would answer right now: recorded for the report, never content. */
  readonly queryChannel: 'none' | 'original' | 'reconnected';
  /** Where the answer came from: the Core's ledger, or the original failure kept as pending. */
  readonly source: 'original-failure' | 'outcome-query';
  /**
   * The `IpcRequestTerminal` the CONFIRMED outcome carried, present exactly when this record's
   * `source` is `outcome-query`. It is the contract value the Core's ledger holds for this
   * invocation - a classification, never a response body.
   */
  readonly terminal?: IpcRequestTerminal | undefined;
}

export type ConfirmationResult =
  | { kind: 'confirmed-failure'; envelope: IpcErrorEnvelope; record: ConfirmationRecord }
  | { kind: 'confirmed-success'; record: ConfirmationRecord; value: unknown }
  | { kind: 'unconfirmed'; record: ConfirmationRecord };

/** The typed failure envelope of a rejected request, with the fields a decision is made from. */
export interface CommandFailureFacts {
  readonly code: string;
  readonly correlationId: string | undefined;
  readonly details: Record<string, unknown>;
  readonly terminal: string;
}

/** The decision the entry gate makes, with the identity the query is then keyed by. */
export interface ConfirmationEntry {
  readonly command: string;
  readonly commandInvocationId: CommandInvocationId;
  readonly correlationId: CorrelationId;
  /** The classified code of the failure this operation entered confirmation with. Never a message. */
  readonly failureCode: string;
}

export type ConfirmationDecision =
  { entry: ConfirmationEntry; kind: 'enter' } | { kind: 'skip'; reason: ConfirmationSkipReason };

/**
 * One confirmed answer, once it has passed every check above. A refusal is deliberately not an error:
 * it is "this answer is not this operation's authoritative result", which keeps the pending state
 * instead of fabricating either a success or a failure.
 */
export type ConfirmedOutcomeVerdict =
  | { kind: 'confirmed-failure'; envelope: IpcErrorEnvelope; terminal: IpcRequestTerminal }
  | { kind: 'confirmed-success'; terminal: 'succeeded'; value: unknown }
  | { kind: 'refused' }
  | { kind: 'unknown' };

export interface ConfirmationQueryRequest<Owner> {
  readonly commandInvocationId: CommandInvocationId;
  readonly correlationId: CorrelationId;
  readonly owner: Owner;
  /**
   * What this attempt may spend, already inside the operation's remaining total budget. The port
   * MUST NOT outlive it: it computes its own deadline from this number and passes what is left to
   * the transport.
   */
  readonly remainingMs: number;
}

/**
 * Everything the module needs from its owner. The module itself never touches a transport, a promise
 * registry or a Core process: it cannot send anything except `query`.
 */
export interface ConfirmationPorts<Owner> {
  /** Which authenticated channel would answer an outcome query right now. For the record only. */
  channel(owner: Owner): 'none' | 'original' | 'reconnected';
  /** Monotonic milliseconds. The deadline is measured on this clock only. */
  now(): number;
  /** Emitted once per confirmation, after it settled. Identity and counts only. */
  onEvent?(record: ConfirmationRecord): void;
  /** The authoritative outcome of ONE invocation, bounded by `remainingMs`. */
  query(request: ConfirmationQueryRequest<Owner>): Promise<CommandOutcome>;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const TERMINALS: ReadonlySet<string> = new Set([
  'succeeded',
  'failed',
  'cancelled',
  'transport_lost',
  'protocol_violation',
  'core_restarted',
]);

const ERROR_CATEGORIES: ReadonlySet<string> = new Set<ErrorCategory>([
  'validation',
  'compatibility',
  'not_found',
  'conflict',
  'permission',
  'unavailable',
  'cancelled',
  'transport',
  'protocol',
  'internal',
]);

const hasOwn = (record: Record<string, unknown>, key: string): boolean =>
  Object.prototype.hasOwnProperty.call(record, key);

/**
 * The typed envelope of a rejected request, read structurally.
 *
 * Structural on purpose: the class identity of a transport error belongs to the transport, and the
 * confirmation must keep reading the envelope the transport really attached even while that file is
 * being reworked. A message is deliberately not part of this: it can carry a pipe path or a nonce.
 */
export const readCommandFailure = (error: unknown): CommandFailureFacts | undefined => {
  if (!isRecord(error) || !isRecord(error.envelope)) return undefined;
  const { code, correlation_id: correlationId, details, terminal } = error.envelope;
  if (typeof code !== 'string' || typeof terminal !== 'string') return undefined;
  return {
    code,
    correlationId: typeof correlationId === 'string' ? correlationId : undefined,
    details: isRecord(details) ? details : {},
    terminal,
  };
};

/** True when the value is a canonically well-formed Aura id with the expected prefix. */
const isCanonicalId = (prefix: 'civ' | 'cor', value: unknown): value is string => {
  if (typeof value !== 'string') return false;
  try {
    idToBytes(prefix, value);
    return true;
  } catch {
    return false;
  }
};

/**
 * Whether a failure of `command` may enter confirmation, and with which identity.
 *
 * The order is deliberate: the cheapest, most definite refusals first, and the identity last - a
 * request whose invocation cannot be trustworthily extracted has no confirmation to run, and one is
 * never minted for it.
 */
export const confirmationEntry = (command: string, error: unknown): ConfirmationDecision => {
  const failure = readCommandFailure(error);
  if (failure === undefined) return { kind: 'skip', reason: 'no-failure-envelope' };
  if (failure.details.outcome_recovery === OUTCOME_RECOVERY_NOT_SENT) {
    return { kind: 'skip', reason: 'not-sent' };
  }
  if (!isUnconfirmedCommandOutcome(failure)) {
    return { kind: 'skip', reason: 'failure-not-unconfirmed' };
  }
  if (CONTROL_ROUTES.has(command)) return { kind: 'skip', reason: 'control-route' };
  const route = coreRoute(command);
  if (route === undefined) return { kind: 'skip', reason: 'route-unknown' };
  if (route.idempotency === 'read_only' || route.idempotency === 'transient_input')
    return { kind: 'skip', reason: 'route-not-a-command' };
  const commandInvocationId = failure.details.command_invocation_id;
  if (!isCanonicalId('civ', commandInvocationId) || !isCanonicalId('cor', failure.correlationId)) {
    return { kind: 'skip', reason: 'invocation-unusable' };
  }
  return {
    entry: {
      command,
      commandInvocationId: commandInvocationId as CommandInvocationId,
      correlationId: failure.correlationId as CorrelationId,
      failureCode: failure.code,
    },
    kind: 'enter',
  };
};

/**
 * Verify one `CommandOutcome` against the original operation.
 *
 * Refusal is the answer for anything that is not provably THIS operation's authoritative result,
 * including a well-formed response for another correlation id: adopting it would contaminate this
 * operation with another one's result.
 */
export const verifyConfirmedOutcome = (
  entry: ConfirmationEntry,
  outcome: unknown,
): ConfirmedOutcomeVerdict => {
  if (!isRecord(outcome)) return { kind: 'refused' };
  if (outcome.outcome !== 'confirmed') return { kind: 'unknown' };
  if (outcome.command_invocation_id !== entry.commandInvocationId) return { kind: 'refused' };
  const terminal = outcome.terminal;
  if (typeof terminal !== 'string' || !TERMINALS.has(terminal)) return { kind: 'refused' };
  const response = outcome.response;
  if (!isRecord(response) || response.jsonrpc !== '2.0' || response.id !== entry.correlationId) {
    return { kind: 'refused' };
  }
  const hasResult = hasOwn(response, 'result');
  const hasError = hasOwn(response, 'error');
  if (hasResult === hasError) return { kind: 'refused' };
  if (isRecord(response.result)) {
    // `succeeded` is the only terminal a stored success can carry; anything else would be a failure
    // reported as a result.
    if (response.result.terminal !== terminal) return { kind: 'refused' };
    if (terminal !== 'succeeded') return { kind: 'refused' };
    if (!hasOwn(response.result, 'value')) return { kind: 'refused' };
    return { kind: 'confirmed-success', terminal: 'succeeded', value: response.result.value };
  }
  if (!isRecord(response.error)) return { kind: 'refused' };
  if (terminal === 'succeeded') return { kind: 'refused' };
  if (!Number.isSafeInteger(response.error.code) || typeof response.error.message !== 'string') {
    return { kind: 'refused' };
  }
  const data = response.error.data;
  if (!isRecord(data)) return { kind: 'refused' };
  if (data.terminal !== terminal) return { kind: 'refused' };
  if (
    typeof data.code !== 'string' ||
    data.correlation_id !== entry.correlationId ||
    typeof data.category !== 'string' ||
    !ERROR_CATEGORIES.has(data.category) ||
    !isRecord(data.details) ||
    typeof data.message !== 'string' ||
    typeof data.retryable !== 'boolean' ||
    typeof data.terminal !== 'string'
  ) {
    return { kind: 'refused' };
  }
  return {
    envelope: data as unknown as IpcErrorEnvelope,
    kind: 'confirmed-failure',
    terminal: terminal as IpcRequestTerminal,
  };
};

/**
 * One shared confirmation owner for a Main process, and one independent confirmation per operation.
 *
 * The instance is shared so that the finite concurrency bound is a bound on the PROCESS; the state of
 * a confirmation (`deadline`, `attempts`, record) lives inside the single `confirm()` call, so two
 * operations can never observe or overwrite each other's result.
 */
export class CommandOutcomeConfirmation<Owner> {
  readonly #policy: CommandOutcomeConfirmationPolicy;
  readonly #ports: ConfirmationPorts<Owner>;
  #active = 0;
  #waiting: Array<(granted: boolean) => void> = [];

  constructor(
    ports: ConfirmationPorts<Owner>,
    policy: CommandOutcomeConfirmationPolicy = COMMAND_OUTCOME_CONFIRMATION_POLICY,
  ) {
    this.#ports = ports;
    this.#policy = policy;
  }

  /** How many confirmations hold a slot right now. For tests and diagnostics. */
  get active(): number {
    return this.#active;
  }

  async confirm(entry: ConfirmationEntry, owner: Owner): Promise<ConfirmationResult> {
    const startedAt = this.#ports.now();
    const deadline = startedAt + this.#policy.totalBudgetMs;
    const record = (
      outcome: ConfirmationOutcome,
      attempts: number,
      source: ConfirmationRecord['source'],
      lastFailureCode?: string,
      terminal?: IpcRequestTerminal,
    ): ConfirmationRecord => ({
      attempts,
      budgetMs: this.#policy.totalBudgetMs,
      command: entry.command,
      commandInvocationId: entry.commandInvocationId,
      correlationId: entry.correlationId,
      elapsedMs: Math.max(0, Math.round(this.#ports.now() - startedAt)),
      ...(lastFailureCode === undefined ? {} : { lastFailureCode }),
      outcome,
      originalFailureCode: entry.failureCode,
      queryChannel: this.#ports.channel(owner),
      source,
      ...(terminal === undefined ? {} : { terminal }),
    });
    const pending = (
      outcome: ConfirmationOutcome,
      attempts: number,
      lastFailureCode?: string,
    ): ConfirmationResult => {
      const settled = record(outcome, attempts, 'original-failure', lastFailureCode);
      this.#ports.onEvent?.(settled);
      return { kind: 'unconfirmed', record: settled };
    };
    if (!(await this.#acquire(deadline))) return pending('concurrency-refused', 0);
    try {
      let attempts = 0;
      let lastFailureCode: string | undefined;
      let sawUnknown = false;
      while (attempts < this.#policy.maxAttempts) {
        const remaining = deadline - this.#ports.now();
        if (remaining <= 0) break;
        if (attempts > 0) {
          const pause = Math.min(this.#policy.retryDelayMs, remaining);
          if (pause > 0) await delay(pause);
          if (this.#ports.now() >= deadline) break;
        }
        const attemptBudget = Math.max(
          1,
          Math.min(this.#policy.attemptBudgetMs, deadline - this.#ports.now()),
        );
        attempts += 1;
        // The attempt is bounded HERE as well as inside the port, so a port that never answers cannot
        // hold this operation past its own deadline.
        const attempt = await settleWithin(
          this.#ports.query({
            commandInvocationId: entry.commandInvocationId,
            correlationId: entry.correlationId,
            owner,
            remainingMs: attemptBudget,
          }),
          attemptBudget,
          undefined,
          () => undefined,
        );
        if (attempt.kind !== 'value') {
          lastFailureCode =
            attempt.kind === 'error'
              ? (readCommandFailure(attempt.error)?.code ?? 'unclassified')
              : 'attempt-deadline';
          sawUnknown = false;
          continue;
        }
        const verdict = verifyConfirmedOutcome(entry, attempt.value);
        if (verdict.kind === 'unknown') {
          // `unknown`/`not_recorded_or_incomplete`: the outcome may be recorded a moment later, so
          // one more attempt is allowed - never more, and never as "did not execute".
          sawUnknown = true;
          lastFailureCode = undefined;
          continue;
        }
        if (verdict.kind === 'refused') return pending('verification-refused', attempts);
        if (verdict.kind === 'confirmed-success') {
          const settled = record(
            'confirmed-succeeded',
            attempts,
            'outcome-query',
            undefined,
            'succeeded',
          );
          this.#ports.onEvent?.(settled);
          return { kind: 'confirmed-success', record: settled, value: verdict.value };
        }
        const settled = record(
          verdict.terminal === 'cancelled' ? 'confirmed-cancelled' : 'confirmed-failed',
          attempts,
          'outcome-query',
          undefined,
          verdict.terminal,
        );
        this.#ports.onEvent?.(settled);
        return { envelope: verdict.envelope, kind: 'confirmed-failure', record: settled };
      }
      return pending(
        attempts === 0 || lastFailureCode === undefined
          ? sawUnknown
            ? 'outcome-unknown'
            : 'budget-exhausted'
          : 'query-failed',
        attempts,
        lastFailureCode,
      );
    } finally {
      this.#release();
    }
  }

  /**
   * Take one of the finite confirmation slots, waiting no longer than this operation's own deadline.
   * A refusal is a bounded answer, not an unbounded queue: the operation keeps its
   * pending-confirmation state.
   */
  async #acquire(deadline: number): Promise<boolean> {
    if (this.#active < this.#policy.maxConcurrent) {
      this.#active += 1;
      return true;
    }
    const remaining = deadline - this.#ports.now();
    if (remaining <= 0) return false;
    return new Promise<boolean>((resolve) => {
      const waiter = (granted: boolean): void => {
        clearTimeout(timer);
        resolve(granted);
      };
      const timer = setTimeout(() => {
        this.#waiting = this.#waiting.filter((candidate) => candidate !== waiter);
        resolve(false);
      }, remaining);
      this.#waiting.push(waiter);
    });
  }

  /** Hand the slot to the next waiter, or give it back to the pool. */
  #release(): void {
    const next = this.#waiting.shift();
    if (next === undefined) {
      this.#active -= 1;
      return;
    }
    // The slot is handed over directly: `#active` is already counting this waiter.
    next(true);
  }
}

const delay = async (milliseconds: number): Promise<void> =>
  new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });
