import type { CommandInvocationId, CorrelationId } from '@aura/contracts/generated';
import type { IpcFrameBinding } from '../protocol.js';
import type { RequestStage, RetiredRecord } from '../request-registry.js';
import type { CoreTransportLifecycleRecord, TransportLifecycleFacts } from './facts.js';

/**
 * The transport's acceptance-layer lifecycle seam.
 * `undefined` sink in every product run: emission then costs one comparison - it does not
 * allocate, await, or reorder anything. A throwing observer is counted and swallowed.
 */
export class TransportLifecycleSeam {
  #sink: ((record: CoreTransportLifecycleRecord) => void) | undefined;
  #failures = 0;
  #lastFailure: string | null = null;
  #seen = 0;
  readonly #binding: IpcFrameBinding;

  constructor(
    binding: IpcFrameBinding,
    sink: ((record: CoreTransportLifecycleRecord) => void) | undefined,
  ) {
    this.#binding = binding;
    this.#sink = sink;
  }

  attach(sink: ((record: CoreTransportLifecycleRecord) => void) | undefined): void {
    this.#sink = sink;
  }

  get attached(): boolean {
    return this.#sink !== undefined;
  }

  get failures(): number {
    return this.#failures;
  }

  get seen(): number {
    return this.#seen;
  }

  /** How the observer seam itself behaved. Counts and one message; never request content. */
  facts(): TransportLifecycleFacts {
    return {
      failures: this.#failures,
      last_failure: this.#lastFailure,
      observer_attached: this.#sink !== undefined,
      seen: this.#seen,
    };
  }

  /**
   * Report the ONE legal late terminal this transport just consumed - as a record about THAT request.
   *
   * WHY IT EXISTS: the registry's own `late` counter says how many late terminals the CHANNEL consumed,
   * which cannot be attributed to any one request. "This designated request's real terminal arrived
   * after its caller gave up" is a fact about a single request id, and this is where the transport still
   * holds that identity: the record the registry just spent.
   *
   * WHAT IT CARRIES: exactly the send-stage identity (request id, CommandInvocationId, method, channel
   * id, connection epoch) and the new stage. `params`, a secret and a nonce have no member on the record
   * type, so none of them can reach an observer through this path.
   *
   * WHAT IT MAY NOT DO: it runs AFTER the terminal has been consumed and returns nothing, so it cannot
   * settle, resolve, reject, reorder or replay anything. `emit` counts and swallows a throwing
   * observer, so an acceptance observer can never turn a real request into a failure.
   *
   * WHEN IT STAYS SILENT: only when the consumed record carries no method, which cannot happen for a
   * request this transport really sent (the method is a required argument of every request path). Leaving
   * the fact unreported is deliberate: a fabricated placeholder would be a worse answer than a missing
   * measurement, and a missing measurement makes an acceptance fact `not_exercised` instead of green.
   */
  lateTerminalConsumed(consumed: RetiredRecord<CorrelationId> | undefined): void {
    if (
      consumed === undefined ||
      typeof consumed.method !== 'string' ||
      consumed.method.length === 0
    ) {
      return;
    }
    this.emit(consumed.method, consumed.id, consumed.commandInvocationId, 'late-terminal-consumed');
  }

  emit(
    method: string,
    id: CorrelationId,
    commandInvocationId: CommandInvocationId | undefined,
    stage: RequestStage,
  ): void {
    const sink = this.#sink;
    if (sink === undefined) return;
    const record: CoreTransportLifecycleRecord =
      commandInvocationId === undefined
        ? {
            channel_id: this.#binding.channelId,
            connection_epoch: Number(this.#binding.connectionEpoch),
            method,
            request_id: id,
            stage,
          }
        : {
            channel_id: this.#binding.channelId,
            command_invocation_id: commandInvocationId,
            connection_epoch: Number(this.#binding.connectionEpoch),
            method,
            request_id: id,
            stage,
          };
    this.#seen += 1;
    try {
      sink(record);
    } catch (error) {
      this.#failures += 1;
      this.#lastFailure = error instanceof Error ? error.message : String(error);
    }
  }
}
