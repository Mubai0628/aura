import type {
  CommandInvocationId,
  CommandOutcome,
  CorrelationId,
  IpcRequestTerminal,
  RuntimeHandshakeComplete,
  RuntimeHandshakeFinish,
  RuntimeHandshakeRequest,
  RuntimeHandshakeResponse,
} from '@aura/contracts/generated';
import { createAuraId } from './ids.js';
import { createOpeningWithdrawal } from './terminal-opening-withdraw.js';
import {
  isTransientInput,
  errorEnvelope,
  notSentEnvelope,
  requestRecoveryEnvelope,
  closedTransportEnvelope,
} from './request-failure-envelopes.js';
import {
  connectMainPipe,
  type MainPipeConnection,
  type MainPipeConnectionInfo,
} from './main-pipe-bridge.js';
import { FrameScheduler, type QueuePriority } from './frame-scheduler.js';
import { FrameDecoder, type IpcFrameBinding, type SecretInputOperation } from './protocol.js';
import {
  createTaskKey,
  OutboundSendOwner,
  type OutboundTask,
  type OutboundTaskEnding,
  type OutboundTaskKey,
  PHYSICAL_SEND_BUDGET_MS,
  SECRET_INPUT_BINDING_BUDGET_MS as SECRET_INPUT_BINDING_BUDGET,
  SecretInputOwnership,
} from './outbound-send-owner.js';
import { settleWithin, withDeadline } from './startup-attempt.js';
import {
  isProvablyUnsent,
  CONTROL_UNRESOLVED_TERMINAL_CAPACITY,
  MAX_ACTIVE_REQUESTS,
  MAX_CONTROL_REQUESTS,
  RequestAdmissionError,
  RequestRegistry,
  RETIRED_CAPACITY,
  RETIRED_TTL_MS,
  type RequestClass,
  UNRESOLVED_TERMINAL_CAPACITY,
  type SettlementTrigger,
} from './request-registry.js';
import {
  OUTCOME_RECOVERY_NOT_SENT,
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
} from '../shared/ipc-request-failure.js';
import { TransportLifecycleSeam } from './core-transport/lifecycle-seam.js';
import { InboundStreams } from './core-transport/inbound-streams.js';
import type {
  CoreTransportChannelFacts,
  CoreTransportHandlers,
  CoreTransportLifecycleRecord,
  CoreTransportRequestStats,
  TransportLifecycleFacts,
} from './core-transport/facts.js';
import {
  HANDSHAKE_REQUEST_TIMEOUT_MS,
  INTERNAL_CONTROL_ROUTES,
  type InternalControlRoute,
} from './core-transport/routes.js';
import {
  CoreRpcError,
  type JsonRpcFailure,
  type JsonRpcRequest,
  type JsonRpcSuccess,
  type PendingRequest,
  type Settlement,
  isIpcErrorEnvelope,
  isRecord,
  sendFailureReason,
} from './core-transport/rpc.js';
export { CoreRpcError } from './core-transport/rpc.js';
export type {
  CoreTransportLifecycleRecord,
  CoreTransportChannelFacts,
  CoreTransportHandlers,
} from './core-transport/facts.js';

export { errorEnvelope } from './request-failure-envelopes.js';

/**
 * The atomic SecretInput binding and one physical send each have their OWN budget, and both live with
 * their owner in `outbound-send-owner.ts`. They are re-exported here so every existing caller and
 * diagnostic keeps one import site:
 *
 *  - `SECRET_INPUT_BINDING_BUDGET_MS` bounds the whole "command frame + SecretInput frame" binding
 *    once the command frame has been committed. After that point the Core may already be holding half
 *    of the operation, and a framed stream cannot be re-synchronised.
 *  - `PHYSICAL_SEND_BUDGET_MS` bounds ONE physical send that has begun writing and stopped
 *    progressing. It is the budget that retires this connection honestly - the sink may hold half a
 *    frame - and it is deliberately independent of every caller's request budget.
 */
export { PHYSICAL_SEND_BUDGET_MS, SECRET_INPUT_BINDING_BUDGET as SECRET_INPUT_BINDING_BUDGET_MS };

export class CoreTransport {
  readonly #binding: IpcFrameBinding;
  readonly #handlers: CoreTransportHandlers;
  /**
   * The single owner of "which requests exist on this channel". It replaces the bare pending map so
   * that active/retired/stage decisions live in one place instead of being scattered across the send
   * path, the timer and the control handler.
   *
   * Bounded on purpose: `maxActive` and `unresolvedCapacity` bound ordinary business work,
   * `controlCapacity` and `controlUnresolvedCapacity` bound this file's internal control routes in
   * their OWN pools (so a saturated business pool cannot refuse the cancel/outcome-query path that
   * drains it), and the retired pair bounds the completed history. See request-registry.ts for the
   * policy and for the real upper bound of each pool.
   */
  readonly #requests = new RequestRegistry<CorrelationId, PendingRequest>({
    controlCapacity: MAX_CONTROL_REQUESTS,
    controlUnresolvedCapacity: CONTROL_UNRESOLVED_TERMINAL_CAPACITY,
    maxActive: MAX_ACTIVE_REQUESTS,
    retiredCapacity: RETIRED_CAPACITY,
    retiredTtlMs: RETIRED_TTL_MS,
    unresolvedCapacity: UNRESOLVED_TERMINAL_CAPACITY,
  });
  readonly #pipe: MainPipeConnection;
  /** Inbound binary streams: their next chunk and byte offset (core-transport/inbound-streams.ts). */
  readonly #streams = new InboundStreams();
  /**
   * The channel's ONE physical-send owner. It owns frame preparation, the outbound sequence numbers
   * and every per-send budget; this class owns the connection, the registry and settlement. Splitting
   * them is what keeps "one slow write" from becoming "one slow channel" without putting a second
   * scheduling authority next to the registry.
   */
  readonly #send: OutboundSendOwner;
  readonly #scheduler: FrameScheduler;
  #authenticated = false;
  #closed = false;
  /** The acceptance-layer lifecycle seam. */
  readonly #lifecycle: TransportLifecycleSeam;
  #nextInboundSequence = 0n;

  private constructor(
    pipe: MainPipeConnection,
    binding: IpcFrameBinding,
    handlers: CoreTransportHandlers,
    lifecycleSink?: (record: CoreTransportLifecycleRecord) => void,
    limits?: {
      physicalSendBudgetMilliseconds?: number;
      secretInputBindingBudgetMilliseconds?: number;
    },
  ) {
    this.#binding = binding;
    this.#pipe = pipe;
    this.#handlers = handlers;
    this.#lifecycle = new TransportLifecycleSeam(binding, lifecycleSink);
    this.#scheduler = new FrameScheduler(pipe);
    this.#send = new OutboundSendOwner(
      binding,
      this.#scheduler,
      {
        onSettled: (task, ending) => this.#onTaskSettled(task, ending),
        onUnrecoverable: (reason) =>
          this.#failTransport('transport_lost', reason, 'transport_lost'),
        onWriteStarted: (task) => this.#onWriteStarted(task),
      },
      {
        ...(limits?.physicalSendBudgetMilliseconds === undefined
          ? {}
          : { physicalSendBudgetMs: limits.physicalSendBudgetMilliseconds }),
        ...(limits?.secretInputBindingBudgetMilliseconds === undefined
          ? {}
          : { secretInputBindingBudgetMs: limits.secretInputBindingBudgetMilliseconds }),
      },
    );
    const decoder = new FrameDecoder((frame) => {
      if (this.#closed) return;
      try {
        if (frame.connectionEpoch < this.#binding.connectionEpoch) return;
        if (
          frame.channelId !== this.#binding.channelId ||
          frame.connectionEpoch !== this.#binding.connectionEpoch
        ) {
          throw new Error('IPC frame did not belong to the active authenticated channel');
        }
        if (frame.sequence !== this.#nextInboundSequence) {
          throw new Error('IPC frame sequence was duplicate, reversed, or missing');
        }
        this.#nextInboundSequence += 1n;
        if (frame.type === 'control') this.#handleControl(frame.payload);
        else this.#handleBinary(frame);
      } catch (error) {
        this.#failTransport('protocol_violation', error, 'protocol_violation');
      }
    });
    void this.#readLoop(decoder);
  }

  static async connect(
    options: {
      addonPath: string;
      channelId: string;
      connectionEpoch: bigint;
      connectTimeoutMilliseconds?: number;
      expectedServerCreationTime?: string;
      expectedServerPid: number;
      expectedServerWindowsSessionId?: number;
      namespaceObjectHash: string;
      /**
       * Identity-only send-stage observer. Omitted by every product caller; the acceptance layer
       * attaches one through the supervisor's `observeClientLifecycle`.
       */
      onLifecycle?: (record: CoreTransportLifecycleRecord) => void;
      /**
       * How long ONE physical send may fail to make progress before this connection is retired.
       *
       * Omitted by every product caller, which then gets `PHYSICAL_SEND_BUDGET_MS`. It is injectable
       * because the budget is a property of the CHANNEL, not of a request: a test that has to observe
       * retirement inside a fake clock shortens it here instead of shortening a production budget for
       * everything.
       */
      physicalSendBudgetMilliseconds?: number;
      pipeNonce: string;
      runtimeInstanceId: string;
      /**
       * How long the WHOLE atomic SecretInput binding may take once its command frame is dispatched.
       *
       * Omitted by every product caller, which then gets `SECRET_INPUT_BINDING_BUDGET_MS`. It is
       * injectable for the same reason the physical-send budget is: the binding budget is a property
       * of the channel's obligations, not of a request, and a test that has to observe a binding that
       * cannot finish uses a fake clock with a short declared budget instead of waiting out the
       * production one.
       */
      secretInputBindingBudgetMilliseconds?: number;
    },
    handlers: CoreTransportHandlers,
  ): Promise<CoreTransport> {
    // The native pipe connect runs on its own thread with a fixed internal wait, so it
    // cannot be interrupted once it has started. It can still be bounded: a caller that
    // is spending a larger budget passes what is left of it here, a late success is
    // drained and closed rather than activated, and a connect that never returns stops
    // holding its caller. `while` outside a timer is not a bound — this is.
    const connect = async (): Promise<CoreTransport> => {
      const pipe = await connectMainPipe(options);
      return new CoreTransport(
        pipe,
        { channelId: options.channelId, connectionEpoch: options.connectionEpoch },
        handlers,
        options.onLifecycle,
        {
          ...(options.physicalSendBudgetMilliseconds === undefined
            ? {}
            : { physicalSendBudgetMilliseconds: options.physicalSendBudgetMilliseconds }),
          ...(options.secretInputBindingBudgetMilliseconds === undefined
            ? {}
            : {
                secretInputBindingBudgetMilliseconds: options.secretInputBindingBudgetMilliseconds,
              }),
        },
      );
    };
    const budget = options.connectTimeoutMilliseconds;
    if (budget === undefined) return connect();
    const outcome = await settleWithin(connect(), Math.max(0, budget), undefined, (late) => {
      late.close();
    });
    if (outcome.kind === 'value') return outcome.value;
    if (outcome.kind === 'error') throw outcome.error;
    throw Object.assign(
      new Error(`Aura Main Pipe connect did not complete within ${Math.max(0, budget)}ms`),
      { code: 'ETIMEDOUT' },
    );
  }

  get peerIdentity(): MainPipeConnectionInfo {
    return this.#pipe.info();
  }

  async beginHandshake(
    request: RuntimeHandshakeRequest,
    budgetMilliseconds?: number,
  ): Promise<RuntimeHandshakeResponse> {
    return (await withDeadline(
      this.#requestInternal('runtime.handshake.begin', request, 0, 10_000),
      budgetMilliseconds ?? HANDSHAKE_REQUEST_TIMEOUT_MS,
    )) as RuntimeHandshakeResponse;
  }

  async finishHandshake(
    request: RuntimeHandshakeFinish,
    budgetMilliseconds?: number,
  ): Promise<RuntimeHandshakeComplete> {
    const completed = (await withDeadline(
      this.#requestInternal('runtime.handshake.finish', request, 0, 10_000),
      budgetMilliseconds ?? HANDSHAKE_REQUEST_TIMEOUT_MS,
    )) as RuntimeHandshakeComplete;
    if (
      completed.channel_id !== this.#binding.channelId ||
      BigInt(completed.connection_epoch) !== this.#binding.connectionEpoch ||
      completed.handshake_kind !== request.handshake_kind ||
      completed.transcript_hash.algorithm !== request.transcript_hash.algorithm ||
      completed.transcript_hash.hex !== request.transcript_hash.hex
    ) {
      throw new Error('Core returned a handshake completion for a different channel');
    }
    this.#authenticated = true;
    return completed;
  }

  async request(
    method: string,
    params: unknown,
    priority: QueuePriority = 1,
    timeoutMilliseconds = 15_000,
    onInvocation?: (id: CommandInvocationId) => void,
    onRequestId?: (id: CorrelationId) => void,
    onQueuedOpening?: (cancelIfUnsent: () => boolean) => void,
  ): Promise<unknown> {
    if (!this.#authenticated) {
      throw new Error('Core transport is not authenticated');
    }
    if (onQueuedOpening && method !== 'terminal.user.open')
      throw new Error('Only a terminal opening owns pre-send cancellation');
    const invocation = isTransientInput(method)
      ? undefined
      : (createAuraId('civ') as CommandInvocationId);
    if (invocation !== undefined) onInvocation?.(invocation);
    return this.#requestInternal(
      method,
      params,
      priority,
      timeoutMilliseconds,
      invocation,
      undefined,
      'ordinary',
      onRequestId,
      onQueuedOpening,
    );
  }

  async requestWithSecretInput(
    method: string,
    params: unknown,
    operation: SecretInputOperation,
    secret: Buffer,
    timeoutMilliseconds = 45_000,
    onInvocation?: (id: CommandInvocationId) => void,
  ): Promise<unknown> {
    if (!this.#authenticated) {
      secret.fill(0);
      throw new Error('Core transport is not authenticated');
    }
    const invocation = createAuraId('civ') as CommandInvocationId;
    onInvocation?.(invocation);
    return this.#requestInternal(method, params, 1, timeoutMilliseconds, invocation, {
      operation,
      secret,
    });
  }

  /**
   * Send one bulk stream as a sequence of COMPLETE frames.
   *
   * This used to wrap every chunk in ONE outbound lock, which is the second reproduced deviation: a
   * P0 control frame could not be considered until the whole stream had been written. The stream is
   * now owned by `OutboundSendOwner`, which builds the next chunk's frame only after the previous one
   * has been written - so a control frame queued in between is simply the next frame chosen, and no
   * frame being written is ever interrupted.
   *
   * A failure is still honest about what it means: the send owner sees the writer error (possibly
   * half a frame on the wire), retires THIS connection exactly once through the send owner's own
   * verdict, and only then does the caller see the error.
   */
  async sendBinaryStream(streamId: string, bytes: Buffer): Promise<void> {
    if (!this.#authenticated) throw new Error('Core transport is not authenticated');
    if (this.#closed) throw new Error('Core transport is closed');
    // A bulk stream is a send task like any other, so it is registered the same way: the send owner
    // reports how it ended, and the transport decides here whether that means THIS connection can no
    // longer be proven able to continue. `sendBinaryStream` itself only reports the failure to the
    // caller that awaits it.
    const key = createTaskKey(`binary:${streamId}`);
    this.#taskRequests.set(key, {
      commandInvocationId: undefined,
      id: streamId,
      method: 'stream.send',
    });
    try {
      await this.#send.sendBinaryStream(streamId, bytes, key);
    } finally {
      this.#taskRequests.delete(key);
      this.#tasks.delete(streamId);
      this.#writing.delete(streamId);
    }
  }

  /**
   * The authoritative outcome of one CommandInvocationId.
   *
   * Same internal control route as `cancel`, for the same reason: the recovery entry point must not be
   * refused by the business capacity whose unresolved work it exists to resolve. Ordinary callers
   * cannot reach this pool - only the two routes above can.
   */
  async queryCommandOutcome(commandInvocationId: CommandInvocationId): Promise<CommandOutcome> {
    return (await this.#controlRequest('commandOutcome', {
      command_invocation_id: commandInvocationId,
    })) as CommandOutcome;
  }

  /**
   * Run one internal control route with its own bounded admission and its existing P0 writer priority.
   *
   * The method, the budget, the frame shape and the admission class all come from
   * `INTERNAL_CONTROL_ROUTES`, which is private to this file: this is the only path that can ask the
   * registry for the control pool, and nothing a caller supplies selects it.
   */
  async #controlRequest(route: InternalControlRoute, params: unknown): Promise<unknown> {
    if (!this.#authenticated) throw new Error('Core transport is not authenticated');
    const spec = INTERNAL_CONTROL_ROUTES[route];
    return this.#requestInternal(
      spec.method,
      params,
      spec.priority,
      spec.timeoutMilliseconds,
      spec.commandInvocation ? (createAuraId('civ') as CommandInvocationId) : undefined,
      undefined,
      'control',
    );
  }

  async #requestInternal(
    method: string,
    params: unknown,
    priority: QueuePriority,
    timeoutMilliseconds: number,
    commandInvocationId?: CommandInvocationId,
    secretInput?: { operation: SecretInputOperation; secret: Buffer },
    /**
     * Which admission pool this request is measured against. Only `#controlRequest` (fed by the
     * internal route table) passes `control`; every other caller - including the public `request()`
     * with any method string and any priority - takes the `ordinary` default and cannot spend the
     * reserved control capacity.
     */
    requestClass: RequestClass = 'ordinary',
    onRequestId?: (id: CorrelationId) => void,
    onQueuedOpening?: (cancelIfUnsent: () => boolean) => void,
  ): Promise<unknown> {
    if (this.#closed) {
      secretInput?.secret.fill(0);
      throw new CoreRpcError(closedTransportEnvelope(method));
    }
    const id = createAuraId('cor');
    const request: JsonRpcRequest = {
      id,
      jsonrpc: '2.0',
      method,
      params,
    };
    if (commandInvocationId !== undefined) {
      request.command_invocation_id = commandInvocationId;
    }
    // SecretInput bytes have exactly ONE owner at a time: the caller until the send boundary takes
    // them over into its own bounded copy, the send owner afterwards. The timeout below therefore
    // only ever clears the caller's buffer, never bytes the sender is still using.
    const secrets =
      secretInput === undefined ? undefined : new SecretInputOwnership(secretInput.secret);
    const response = new Promise<unknown>((resolve, reject) => {
      const timeout = setTimeout(() => {
        // A timeout settles THIS request and nothing else. It used to retire the whole channel, which
        // made one slow request every other request's business. The id is retained afterwards so the
        // real answer, if it arrives, is recognised as a legal late terminal and dropped rather than
        // read as an unknown request.
        //
        // The terminal is `cancelled`, not `failed`: `failed` maps to the internal category and would
        // read as "Core is broken" when the Core is healthy and only this caller stopped waiting. The
        // code stays `ipc.request_timeout` so existing diagnostics keep matching.
        //
        // Whether the command may already have run is read from the send stage rather than assumed:
        // a request whose frame never reached the writer is reported as `not_sent`, so no caller is
        // sent to query an outcome that cannot exist.
        secretInput?.secret.fill(0);
        const task = this.#tasks.get(id);
        if (task !== undefined && !task.outcome().wroteAnything) {
          // The frame is still the channel's to send: take it back BEFORE the waiter is settled, so the
          // bytes never go out for a caller that is no longer listening. From here on the frame cannot
          // be dispatched at all - the send owner refuses it at the scheduling boundary.
          task.withdraw();
        }
        const settled = this.#settle(id, 'timeout');
        if (settled === undefined) return;
        settled.pending.reject(
          new CoreRpcError(
            requestRecoveryEnvelope(
              method,
              settled.pending.commandInvocationId,
              errorEnvelope(
                'request_timeout',
                `Core request timed out: ${method}`,
                'cancelled',
                id,
              ),
              settled.provablyUnsent
                ? OUTCOME_RECOVERY_NOT_SENT
                : OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
            ),
          ),
        );
      }, timeoutMilliseconds);
      try {
        this.#requests.begin(id, {
          commandInvocationId,
          // Retained so a legal late terminal can be reported as a record about THIS request; see
          // `TransportLifecycleSeam.lateTerminalConsumed`. It is the method NAME, never its params.
          method,
          payload: { commandInvocationId, reject, resolve, timeout },
          requestClass,
          timeoutMilliseconds,
        });
        // Emission point 1: the waiter exists and nothing has reached the writer yet.
        this.#lifecycle.emit(method, id, commandInvocationId, 'pending-write');
      } catch (error) {
        clearTimeout(timeout);
        secrets?.release();
        if (error instanceof RequestAdmissionError) {
          // Bounded admission refused this request locally, from its own class's pool. Nothing was
          // written, NO send task was registered, and the channel is deliberately untouched: refusing
          // is what keeps a legitimate first late answer from being evicted out of the identity set
          // by a burst of unrelated traffic. `error.refusal` names the pool in `not_sent_reason`, so a
          // refused CONTROL request is visibly a control-capacity refusal rather than a hidden
          // business one.
          reject(
            new CoreRpcError(notSentEnvelope('local_overload', error.message, error.refusal, id)),
          );
          return;
        }
        reject(error);
        return;
      }
      // The write is deliberately NOT awaited before returning, and it is no longer serialised behind
      // an outbound barrier: each frame is its own queue entry in the channel's send owner, so a
      // request with a 5 ms budget cannot sit behind an unrelated slow write, and a P0 control frame
      // is simply the next frame chosen once the current one has been written.
      const key = createTaskKey(method);
      try {
        this.#taskRequests.set(key, { commandInvocationId, id, method, onRequestId });
        const task = this.#send.register(
          {
            commandInvocationId,
            correlationId: id,
            // Serialised ONCE, here, so the send owner can declare the frame's exact byte size BEFORE
            // the frame exists: queue capacity, the P0 limit and the byte bound are decided from that
            // number, and the bytes themselves are produced at the scheduling boundary, which is what
            // commits the sequence number in true send order.
            framePayload: Buffer.from(JSON.stringify(request), 'utf8'),
            method,
            priority,
            taskLabel: key.label,
            ...(secrets === undefined || secretInput === undefined
              ? {}
              : { secret: { operation: secretInput.operation, ownership: secrets } }),
          },
          key,
        );
        if (!this.#taskRequests.has(key)) {
          // The task ended the moment it was admitted - a local refusal, or a frame that was simply
          // written. `#onTaskSettled` has already taken this request's one settlement decision, so
          // there is nothing left here and nothing to keep in `#tasks`: an entry kept for a task that
          // has already ended is a reference nobody would ever release.
          return;
        }
        this.#tasks.set(id, task);
        onQueuedOpening?.(
          createOpeningWithdrawal(
            id,
            commandInvocationId,
            () => this.#tasks.get(id),
            () => this.#writing.has(id),
            (envelope) => this.#settle(id, 'cancel')?.pending.reject(new CoreRpcError(envelope)),
          ),
        );
        // Which frames may still be dispatched at the SCHEDULING BOUNDARY:
        //
        //  - An ordinary request is the channel's to send only while its waiter is still live. Once
        //    the caller gave up, its frame is refused while nothing of it exists - no sequence number,
        //    no bytes - so `not_sent` is true of the wire as well as of the caller. `setWanted`
        //    is read live: the scheduler asks THIS record at the moment it would really build the
        //    frame, so a predicate installed after registration still decides.
        //  - An atomic SecretInput binding is NOT gated: once its command frame is committed the Core
        //    may already hold half the operation, so the binding is owed. Either it completes, or the
        //    send owner's own binding budget retires the channel honestly.
        const gated = (): boolean => this.#requests.isActive(id);
        if (secrets === undefined) {
          task.setWanted(gated);
        } else {
          task.setWanted(() => true);
        }
        if (this.#requests.isActive(id)) return;
        // Already settled locally while this frame was being prepared: the frame is WITHDRAWN instead
        // of dispatched. Rejecting a waiter and then sending the request anyway would be an
        // unannounced write, and `not_sent` would be a lie.
        task.withdraw();
      } catch (error) {
        // A refusal that is PROVABLY unsent: an oversized P0 frame, an invalid secret, a missing
        // invocation id, or a task that does not fit the channel's total pending-byte bound. Nothing
        // was registered, nothing was written and the channel stays healthy.
        this.#taskRequests.delete(key);
        clearTimeout(timeout);
        secrets?.release();
        const settled = this.#settle(id, 'send-failure');
        if (settled === undefined) return;
        settled.pending.reject(
          new CoreRpcError(
            requestRecoveryEnvelope(
              method,
              settled.pending.commandInvocationId,
              notSentEnvelope('request_not_sent', String(error), sendFailureReason(error), id),
              OUTCOME_RECOVERY_NOT_SENT,
            ),
          ),
        );
      }
    });
    return response;
  }

  /**
   * The ONE send-stage boundary: a frame of this request is about to be handed to the sink.
   *
   * From here on the client cannot prove the bytes were not written, so the registry moves the record
   * to `submitted` and the lifecycle observer sees emission point 2. Both happen synchronously, so an
   * observed request never gains a microtask turn the unobserved one does not have.
   */
  #onWriteStarted(task: OutboundTaskKey): void {
    const entry = this.#taskRequests.get(task);
    if (entry === undefined) return;
    if (this.#writing.has(entry.id)) return;
    this.#writing.add(entry.id);
    this.#requests.markSubmitted(entry.id);
    entry.onRequestId?.(entry.id);
    this.#lifecycle.emit(entry.method, entry.id, entry.commandInvocationId, 'submitted');
  }

  /** Requests whose frame has already begun writing, so the send stage is reported exactly once. */
  readonly #writing = new Set<CorrelationId>();

  /**
   * The ONE settlement decision for the frame of one request, taken when the send owner reports how a
   * task ended.
   *
   * The transport never sends anything itself and never settles a caller from a send detail: it asks
   * the registry what stage this request really reached. Three cases, deliberately distinct:
   *
   *  1. The task's own payload was invalid AND nothing of it reached the sink. That is request-local:
   *     the caller hears `not_sent`, and the channel stays healthy.
   *  2. The connection can no longer be proven able to continue - a physical send stopped progressing
   *     with a frame possibly half written, or a SecretInput binding could not be completed after its
   *     command frame was committed. THIS connection is retired exactly once, so the waiter keeps the
   *     possibly-executed outcome and is never told `not_sent`.
   *  3. A failure the registry already settled (a timeout, or the channel ending) is not re-reported:
   *     the first settlement stands.
   */
  #onTaskSettled(task: OutboundTaskKey, ended: OutboundTaskEnding): void {
    const entry = this.#taskRequests.get(task);
    if (entry === undefined) return;
    this.#taskRequests.delete(task);
    this.#tasks.delete(entry.id);
    this.#writing.delete(entry.id);
    if (ended.connectionUnrecoverable) {
      this.#failTransport(
        'transport_lost',
        ended.reason ?? new Error('Aura Core send owner retired its connection'),
        'transport_lost',
      );
      return;
    }
    if (ended.reason === undefined) {
      // Nothing failed: the frame was written, or it was taken back before any of it existed. Neither
      // is a channel failure. In particular a frame that goes out after its caller stopped waiting is
      // NOT one: the Core can still answer it, and that first real answer is the request's legal late
      // terminal - the registry consumes exactly one of them. Retiring the channel here would end a
      // connection over a request that is, by the protocol's own rules, already fully accounted for.
      return;
    }
    const settled = this.#settle(entry.id, 'send-failure');
    if (settled === undefined) return;
    settled.pending.reject(
      new CoreRpcError(
        requestRecoveryEnvelope(
          entry.method,
          settled.pending.commandInvocationId,
          settled.provablyUnsent
            ? notSentEnvelope(
                'request_not_sent',
                ended.reason.message,
                sendFailureReason(ended.reason),
                entry.id,
              )
            : errorEnvelope('transport_lost', ended.reason.message, 'transport_lost', entry.id),
          settled.provablyUnsent
            ? OUTCOME_RECOVERY_NOT_SENT
            : OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
        ),
      ),
    );
    if (!settled.provablyUnsent) {
      // The frame had begun writing and this failure is not the sink's own: the byte stream can no
      // longer be shown to be intact, so the channel owner retires it exactly once. The waiter above
      // already holds the queryable-outcome answer, never a `not_sent` claim.
      this.#failTransport('transport_lost', ended.reason, 'transport_lost');
    }
  }

  /**
   * The send-stage facts of one request, keyed by its own send task.
   *
   * The send owner reports how a TASK ended, not how a request ended; this map is what lets the
   * transport take its ONE settlement decision about the right request. It holds identity only - a
   * method name, a correlation id, an invocation id - and an entry is removed the moment its task
   * settles, so a settled request leaves nothing behind here.
   */
  readonly #taskRequests = new Map<
    OutboundTaskKey,
    {
      commandInvocationId: CommandInvocationId | undefined;
      id: CorrelationId;
      method: string;
      onRequestId?: ((id: CorrelationId) => void) | undefined;
    }
  >();

  /** This channel's in-flight send tasks, by the request they carry. */
  readonly #tasks = new Map<CorrelationId, OutboundTask>();

  /**
   * Ask the Core to cancel one correlation id.
   *
   * Goes through the internal control route, so it is admitted from its own bounded pool and reaches
   * the sink even while the business pools are saturated with unresolved identities - which is exactly
   * when cancelling matters. When the CONTROL pool is full it is refused locally as
   * `ipc.local_overload` with `not_sent`: nothing was written, so the caller knows the cancel did not
   * happen, and no identity awaiting its real terminal was evicted to make room.
   */
  async cancel(requestId: CorrelationId): Promise<unknown> {
    return this.#controlRequest('cancel', { request_id: requestId });
  }

  close(): void {
    this.#failTransport(
      'transport_lost',
      new Error('Aura Core transport closed'),
      'transport_lost',
    );
  }

  #handleControl(payload: Buffer): void {
    const message = JSON.parse(payload.toString('utf8')) as unknown;
    if (!isRecord(message) || message.jsonrpc !== '2.0') {
      throw new Error('invalid JSON-RPC control message');
    }
    if (typeof message.method === 'string' && !('id' in message)) {
      if (message.method === 'stream.open') this.#streams.open(message.params);
      if (message.method === 'stream.close' || message.method === 'stream.error') {
        this.#streams.close(message.params);
      }
      this.#handlers.onNotification(message.method, message.params);
      return;
    }
    if (typeof message.id !== 'string') throw new Error('JSON-RPC response has no request id');

    // Shape is validated BEFORE settling. Settling first and then throwing left the caller with a
    // promise that could never settle, because the request was already gone from the pending set.
    const isSuccess = 'result' in message && !('error' in message);
    const isFailure = 'error' in message && !('result' in message);
    if (!isSuccess && !isFailure) {
      throw new Error('JSON-RPC response must contain exactly one terminal');
    }
    if (isSuccess) {
      const success = message as unknown as JsonRpcSuccess;
      if (!isRecord(success.result) || success.result.terminal !== 'succeeded') {
        throw new Error('JSON-RPC success has no unique succeeded terminal');
      }
    } else {
      const failure = message as unknown as JsonRpcFailure;
      if (!isRecord(failure.error) || !isIpcErrorEnvelope(failure.error.data)) {
        throw new Error('JSON-RPC failure has no typed error envelope');
      }
    }

    const settled = this.#settle(message.id, 'response');
    if (settled === undefined) {
      // Not active. Four different things, deliberately not collapsed into one:
      const late = this.#requests.consumeLateTerminal(message.id);
      if (late.verdict === 'legal-late') {
        // The client itself gave up first (timeout/cancel/send failure) on a request it HAD submitted.
        // This is the one legal late terminal and it has just been consumed; the record now says the
        // final response was seen, so the next terminal for this id is a duplicate. The value is NOT
        // delivered: a caller that timed out must never be handed a success afterwards. The
        // authoritative result for anything that may have run is runtime.command_outcome.
        //
        // The consumption is REPORTED as a lifecycle record about this request, after the registry has
        // already consumed it, so a reader can attribute it to one request instead of to the channel's
        // `late` total. Reporting cannot change the outcome above: the record carries identity only and
        // a throwing observer is counted and swallowed.
        this.#lifecycle.lateTerminalConsumed(late.consumed);
        return;
      }
      if (late.verdict === 'duplicate') {
        throw new Error('JSON-RPC response repeated a terminal already seen for this request');
      }
      if (late.verdict === 'unsent') {
        // No byte of this id ever reached the writer, so no legitimate terminal can exist for it.
        throw new Error('JSON-RPC response references a request that was never submitted');
      }
      throw new Error('JSON-RPC response references an unknown request');
    }
    if (isSuccess) {
      settled.pending.resolve((message as unknown as JsonRpcSuccess).result.value);
      return;
    }
    settled.pending.reject(new CoreRpcError((message as unknown as JsonRpcFailure).error.data));
  }

  /**
   * The single settlement path. It hands back the waiter only for the call that actually settled it,
   * which is what makes timeout-vs-response and disconnect-vs-response settle exactly once, together
   * with the send stage that decides how honestly the outcome can be reported.
   */
  #settle(id: CorrelationId, trigger: SettlementTrigger): Settlement | undefined {
    const record = this.#requests.settle(id, trigger);
    if (record === undefined) return undefined;
    clearTimeout(record.payload.timeout);
    return {
      pending: record.payload,
      provablyUnsent: isProvablyUnsent(record.stage, trigger),
    };
  }

  /**
   * Counts for tests and diagnostics. No request content, no secrets.
   *
   * The counts are split by OWNER on purpose, because that is what the send fix is about: the registry
   * owns admission and identity, the send owner owns frames, budgets and queue occupancy, the
   * scheduler owns frames and bytes actually waiting. A reader can therefore see which bound was
   * reached without any of them having to guess about another.
   */
  requestStats(): CoreTransportRequestStats {
    const outboundFrames = this.#scheduler.facts();
    const outbound = this.#send.facts();
    return {
      ...this.#requests.stats(),
      lifecycleFailures: this.#lifecycle.failures,
      lifecycleSeen: this.#lifecycle.seen,
      outbound,
      outboundFrames,
      withdrawn: outboundFrames.withdrawnGroups,
    };
  }

  /**
   * The sequence number the NEXT frame written on this channel will carry.
   *
   * It is exposed for one reason: a reader can compare it with the numbers it really observed at the
   * sink and prove the outbound sequence is gap-free - a frame that was withdrawn or refused must not
   * have spent a number, and a frame that was written must have spent exactly one.
   */
  get nextOutboundSequence(): bigint {
    return this.#scheduler.nextSequence;
  }

  /**
   * The channel identity this transport really authenticated, plus whether an observer is attached.
   *
   * This is the transport's OWN binding, not a guess from a log line: `channel_id` and
   * `connection_epoch` are the pair every inbound frame is checked against. It carries no request
   * content and no credential.
   */
  channelFacts(): CoreTransportChannelFacts {
    return {
      authenticated: this.#authenticated,
      channel_id: this.#binding.channelId,
      closed: this.#closed,
      connection_epoch: Number(this.#binding.connectionEpoch),
      observer_attached: this.#lifecycle.attached,
    };
  }

  /**
   * Attach (or, with `undefined`, detach) the identity-only send-stage observer.
   *
   * Additive and one-directional: the sink receives records and returns nothing, so it cannot answer
   * a request, resolve a waiter, or change a send stage. A throwing sink is counted and swallowed -
   * an acceptance observer must never be able to fail a real request.
   */
  attachLifecycleObserver(
    sink: ((record: CoreTransportLifecycleRecord) => void) | undefined,
  ): void {
    this.#lifecycle.attach(sink);
  }

  /** How the observer seam itself behaved. Counts and one message; never request content. */
  lifecycleFacts(): TransportLifecycleFacts {
    return this.#lifecycle.facts();
  }

  #handleBinary(frame: {
    chunkSequence: bigint;
    data: Buffer;
    endOfMessage: boolean;
    streamId: string;
  }): void {
    this.#streams.chunk(frame);
    this.#handlers.onBinaryChunk(frame);
  }

  #failTransport(code: string, cause: unknown, terminal: IpcRequestTerminal): void {
    if (this.#closed) return;
    this.#closed = true;
    const message = cause instanceof Error ? cause.message : String(cause);
    const binaryResumeCursors = this.#streams.resumeCursors();
    const envelope = {
      ...errorEnvelope(code, message, terminal),
      details: { binary_resume_cursors: binaryResumeCursors },
    };
    // A real disconnect still rejects this channel's in-flight work. It is deliberately scoped to this
    // channel: no other session's requests are touched, and each rejection carries its invocation id
    // plus the recovery hint so the caller queries the authoritative outcome instead of replaying.
    // Ordinary and control requests drain through the same single path; identities still awaiting a
    // real terminal are NOT evicted to tidy up - they stay in the registry, and the channel that could
    // have consumed them is over.
    for (const record of this.#requests.drainActive()) {
      clearTimeout(record.payload.timeout);
      record.payload.reject(
        new CoreRpcError(
          requestRecoveryEnvelope(
            record.method,
            record.commandInvocationId,
            {
              ...envelope,
              correlation_id: record.id,
            },
            isTransientInput(record.method) && isProvablyUnsent(record.stage, 'disconnect')
              ? OUTCOME_RECOVERY_NOT_SENT
              : OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
          ),
        ),
      );
    }
    // A frame that is currently being written is not silently dropped here: the native side is asked
    // to cancel its outstanding I/O, which is what settles a write stuck inside the OS, and only then
    // is the pipe closed. Every send task then settles through its own owner, so no bytes a native
    // write may still be reading are freed underneath it, and `close` cannot wedge Main.
    this.#pipe.cancel();
    this.#pipe.close();
    // The scheduler is closed AFTER the sink stopped: everything it still held is rejected with this
    // same reason, so a queued frame is released and a task waiting for queue space is woken instead
    // of being left on a promise that can never settle.
    this.#scheduler.close(new Error(message));
    for (const task of this.#tasks.values()) task.release();
    // Every reference this transport kept to those send tasks goes with them. A record that can be
    // released now must not wait out a 120 s deadline, and a map entry kept for a task whose channel
    // is already over is a reference nobody would ever clear. The send tasks still settle through
    // their own owner - `#onTaskSettled` finds no entry and takes no second decision.
    this.#tasks.clear();
    this.#taskRequests.clear();
    this.#writing.clear();
    this.#handlers.onClosed(envelope);
  }

  async #readLoop(decoder: FrameDecoder): Promise<void> {
    try {
      while (!this.#closed) {
        const chunk = await this.#pipe.read(1024 * 1024);
        if (this.#closed) return;
        if (chunk.byteLength === 0) {
          throw new Error('Aura Core pipe disconnected');
        }
        decoder.push(chunk);
      }
    } catch (error) {
      if (!this.#closed) this.#failTransport('transport_lost', error, 'transport_lost');
    }
  }
}
