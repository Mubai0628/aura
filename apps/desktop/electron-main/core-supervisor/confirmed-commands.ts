import { CORE_UNREACHABLE_MESSAGE } from '../core-unavailable.js';
import type { CommandInvocationId, CommandOutcome } from '@aura/contracts/generated';
import type { CoreTransport } from '../core-transport.js';
import {
  CommandOutcomeConfirmation,
  confirmationEntry,
  readCommandFailure,
  type ConfirmationQueryRequest,
  type ConfirmationResult,
} from '../command-outcome-confirmation.js';
import { settleWithin } from '../startup-attempt.js';
import type { CoreConfirmationOwner } from './config.js';
import { ConfirmedCommandFailure, readErrorCode, readFailureEnvelope } from './errors.js';

/** What the confirmed-command machinery reads from the supervisor that owns it. */
export interface ConfirmedCommandsHost {
  /** The authenticated channel, if there is one now. */
  transport(): CoreTransport | undefined;
  /** The Core process generation and runtime instance that own the outcome ledger now. */
  coreGeneration(): number;
  coreRuntimeInstanceId(): string;
  /** A same-process reconnect in progress, if any. */
  reconnecting(): Promise<void> | undefined;
  log(event: string, fields: Record<string, unknown>): Promise<void>;
  /** `CoreSupervisor.queryCommandOutcome`: the product's one `runtime.command_outcome` query. */
  queryCommandOutcome(commandInvocationId: CommandInvocationId): Promise<CommandOutcome>;
}

/**
 * Every normal command Main sends, with its automatic, bounded outcome confirmation.
 */
export class ConfirmedCommands {
  readonly #host: ConfirmedCommandsHost;

  /**
   * The one owner of the automatic outcome confirmation for this Main process.
   *
   * ONE instance, so the finite concurrency bound covers the whole process; ONE confirmation per
   * operation, held inside the `confirm()` call that started it and returning only into that
   * operation's own promise. Nothing here is shared between two operations, which is what keeps a
   * late answer from a slow query out of a later operation's result.
   *
   * The record it emits is identity, counts and a classification - never a params object, a
   * credential or a response value - and it is written through the supervisor's existing
   * `logs/main.jsonl` sink.
   */
  readonly #confirmation = new CommandOutcomeConfirmation<CoreConfirmationOwner>({
    channel: (owner) => this.#confirmationChannel(owner),
    now: () => performance.now(),
    onEvent: (record) =>
      void this.#host.log('command_outcome_confirmation', {
        attempts: record.attempts,
        budgetMs: record.budgetMs,
        command: record.command,
        commandInvocationId: record.commandInvocationId,
        correlationId: record.correlationId,
        elapsedMs: record.elapsedMs,
        lastFailureCode: record.lastFailureCode,
        // The failure the NORMAL entry point caught, and the terminal the Core's ledger named: both are
        // classified contract values (an error code and an IpcRequestTerminal), so the product's own
        // record can show that a real local failure was really confirmed without exposing a body.
        originalFailureCode: record.originalFailureCode,
        outcome: record.outcome,
        queryChannel: record.queryChannel,
        source: record.source,
        terminal: record.terminal,
      }),
    query: (request) => this.#queryOutcomeWithin(request),
  });

  constructor(host: ConfirmedCommandsHost) {
    this.#host = host;
  }

  /**
   * Run ONE normal command with the automatic, bounded outcome confirmation.
   *
   * THIS IS THE NORMAL PRODUCT CALL CHAIN: every P03/P04/P05/P07/Agent command Main serves goes
   * through here, so the owner-confirmed behaviour ("a timed-out write is confirmed by a bounded
   * query and updates the existing state, and is never resent") is reached from the real entry point
   * and not from a helper only a test calls.
   *
   * THE WRITE IS INVOKED EXACTLY ONCE, on the line below, and only the confirmation module's own
   * reading path can run afterwards. There is no branch in this method - and no method in the
   * confirmation module - that could send `command` again: the query it performs is
   * `runtime.command_outcome`, keyed by the invocation id the transport already put on the original
   * frame.
   *
   * WHAT EACH ENDING DOES:
   *  - `confirmed-success`: the value goes through `accept`, which is the same projection the normal
   *    success path applies, so the operation's existing success consumption is reused unchanged.
   *  - `confirmed-failure`: the Core's OWN stored error envelope is thrown, so the real error, its
   *    terminal and its side-effect description reach the caller instead of a synthesized one.
   *  - `unconfirmed` (`unknown`, a failed query, an exhausted budget): the ORIGINAL failure is
   *    rethrown, which is what keeps the operation in its pending-confirmation state and never turns
   *    "not confirmed" into "did not execute" or "safe to retry".
   */
  async run<Result>(options: {
    accept: (value: unknown) => Result;
    command: string;
    send: (transport: CoreTransport) => Promise<unknown>;
  }): Promise<Result> {
    const transport = this.#host.transport();
    if (transport === undefined) throw new Error(CORE_UNREACHABLE_MESSAGE);
    const owner: CoreConfirmationOwner = {
      coreGeneration: this.#host.coreGeneration(),
      coreRuntimeInstanceId: this.#host.coreRuntimeInstanceId(),
      writeTransport: transport,
    };
    let value: unknown;
    try {
      value = await options.send(transport);
    } catch (error) {
      const decision = confirmationEntry(options.command, error);
      if (decision.kind === 'skip') {
        // The identity of the failure this decision was made about, when its envelope carries one.
        // A skip is a real product decision about ONE command, and an acceptance reader has to be
        // able to tie it to the invocation Main really sent; without the identity it could only be
        // matched by method name, which is not an association. Read from the same typed envelope
        // the decision was made from - never invented, and omitted when the envelope has none.
        const failure = readCommandFailure(error);
        const invocation = failure?.details.command_invocation_id;
        void this.#host.log('command_outcome_confirmation_skipped', {
          command: options.command,
          ...(typeof failure?.correlationId === 'string'
            ? { correlation_id: failure.correlationId }
            : {}),
          ...(typeof invocation === 'string' ? { command_invocation_id: invocation } : {}),
          reason: decision.reason,
        });
        throw error;
      }
      let confirmation: ConfirmationResult;
      try {
        confirmation = await this.#confirmation.confirm(decision.entry, owner);
      } catch (confirmationError) {
        // A failure of the confirmation module itself is THIS operation's own business: it is
        // recorded under its own event so it cannot be mistaken for the command's result, and the
        // operation keeps its original pending state. It is deliberately not folded into a generic
        // internal error and it cannot affect another operation, because each `confirm()` call owns
        // its own deadline and record. Log only a classified code: unexpected errors may still
        // contain private response data, even though this owner never received params.
        void this.#host.log('command_outcome_confirmation_failed', {
          command: options.command,
          code:
            readFailureEnvelope(confirmationError)?.code ??
            readErrorCode(confirmationError) ??
            'unclassified',
        });
        throw error;
      }
      if (confirmation.kind === 'confirmed-success') return options.accept(confirmation.value);
      if (confirmation.kind === 'confirmed-failure') {
        throw new ConfirmedCommandFailure(confirmation.envelope);
      }
      throw error;
    }
    return options.accept(value);
  }

  /**
   * ONE bounded `runtime.command_outcome` query for ONE invocation.
   *
   * `CoreSupervisor.queryCommandOutcome` is the product's only implementation of that route, and the
   * confirmation is now its normal product caller, so the recovery instruction the transport itself
   * carries is reachable from a real write instead of only from a test.
   *
   * THREE THINGS THIS DELIBERATELY DOES:
   *  - It asks only the SAME Core process that could have run the write. After a Core restart the
   *    authoritative ledger is gone, and an `unknown` from a new process would be a statement about
   *    the wrong ledger, so the query is not sent at all and the operation keeps its pending state.
   *    A reconnect to the same process is a different thing: the ledger is intact, so the query may
   *    be answered by the reconnected channel.
   *  - It spends the wait for a reconnecting channel out of the SAME remaining budget the caller
   *    passed, never out of a fresh deadline, so a recovery wait and the query cannot add up to more
   *    than one bound.
   *  - It bounds the query with the remaining budget. The transport's own control-route budget (10 s)
   *    still bounds the underlying request, so an abandoned attempt is bounded twice.
   */
  async #queryOutcomeWithin(
    request: ConfirmationQueryRequest<CoreConfirmationOwner>,
  ): Promise<CommandOutcome> {
    const { owner } = request;
    if (!this.#sameCoreProcess(owner)) {
      throw new Error(
        'the Core that could answer this outcome query is no longer the supervised one',
      );
    }
    const deadline = performance.now() + Math.max(0, request.remainingMs);
    const reconnect = this.#host.reconnecting();
    if (this.#host.transport() === undefined && reconnect !== undefined) {
      await settleWithin(
        reconnect.then(() => undefined),
        Math.max(0, deadline - performance.now()),
        undefined,
        () => undefined,
      );
      if (!this.#sameCoreProcess(owner)) {
        throw new Error('the Core changed while this outcome query waited for a channel');
      }
    }
    const remaining = Math.max(0, deadline - performance.now());
    if (remaining <= 0) {
      throw new Error('the outcome query budget was spent waiting for an authenticated channel');
    }
    const attempt = await settleWithin(
      this.#host.queryCommandOutcome(request.commandInvocationId),
      remaining,
      undefined,
      () => undefined,
    );
    if (attempt.kind === 'value') return attempt.value;
    if (attempt.kind === 'error') throw attempt.error;
    throw new Error(`the outcome query did not answer within ${Math.round(remaining)}ms`);
  }

  /**
   * Which authenticated channel would answer an outcome query for `owner` right now.
   *
   * Recorded for the report only. `reconnected` means the same Core process is being asked over a
   * new channel - the ledger that holds the answer is the same one, which is why that is allowed.
   */
  #confirmationChannel(owner: CoreConfirmationOwner): 'none' | 'original' | 'reconnected' {
    const transport = this.#host.transport();
    if (transport === undefined || !this.#sameCoreProcess(owner)) return 'none';
    return transport === owner.writeTransport ? 'original' : 'reconnected';
  }

  /**
   * The logical-operation generation check, kept separate from the channel epoch.
   *
   * The channel a confirmation may ask is decided by "is this still the Core process that owns the
   * ledger", not by "is this the same socket": a new connection to the same process is the same
   * authority, a new process is not.
   */
  #sameCoreProcess(owner: CoreConfirmationOwner): boolean {
    return (
      owner.coreGeneration === this.#host.coreGeneration() &&
      owner.coreRuntimeInstanceId === this.#host.coreRuntimeInstanceId()
    );
  }
}
