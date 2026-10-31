import type {
  ToolCredentialMainCommand,
  ToolCredentialMainParams,
  ToolCredentialMainResult,
} from '../shared/tool-credential-validation.js';
import { requestAgentSecret, requestToolCredential } from './secret-requests.js';
import { auxiliaryRuntimeNotification } from './runtime-notifications.js';
import { replyBudget, requestAgentCommand } from './agent-requests.js';
import { CoreDomainRequests } from './core-domain-requests.js';
import {
  isRecoveryParams,
  parseRecoveryResult,
  type ConfigRecoveryMainCommand,
  type ConfigRecoveryMainParams,
  type ConfigRecoveryMainResult,
} from '../shared/recovery-validation.js';
import { parseRuntimeReadiness } from '../shared/readiness-validation.js';
import { recoveryProjectionPlan } from './recovery-projection-plan.js';
import { CoreExitRecovery, type CoreRestartCause } from './core-exit-recovery.js';
import { HeartbeatWatch, processActivityReader } from './core-supervisor/heartbeat-watch.js';
import { appendMainLog } from './core-supervisor/main-log.js';
import { ProjectionCompatibility } from './projection-compatibility.js';
import type { ProjectionCompatibilitySnapshotV1 } from '@aura/contracts/generated';
import type {
  IpcChannelId,
  CommandInvocationId,
  CommandOutcome,
  CorrelationId,
  ProjectionSnapshotRequest,
  ProjectionSubscription,
  ProjectionUnsubscribeRequest,
  RuntimeHandshake,
  RuntimeHandshakeFinish,
  RuntimeHandshakeKind,
  RuntimeHandshakeRequest,
  RuntimeHandshakeResponse,
  SecurityProjectionChange,
  ToolActivityPageRequestV1,
  ToolActivityPageV1,
  ToolCredentialCandidatesProjectionV1,
} from '@aura/contracts/generated';
import { createHmac, randomBytes } from 'node:crypto';
import { spawn, type ChildProcess } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { CoreTransport, CoreRpcError, errorEnvelope } from './core-transport.js';
import type { CoreTransportChannelFacts, CoreTransportLifecycleRecord } from './core-transport.js';

import { StartupAttempt, settleWithin, startupStillOwns, withDeadline } from './startup-attempt.js';

import { waitForChildExit } from './child-exit.js';
import { createAuraId } from './ids.js';
import { describeCoreStartupFailure } from '../shared/core-startup-diagnostics.js';
import { isRequestLocalTimeout } from '../shared/ipc-request-failure.js';
import type { P03CoreCommand, P03CoreParams, P03CoreResult } from '../shared/p03-validation.js';
import type {
  ModelProjectionChange,
  P05MainParams,
  P05MainResult,
  P05NonSecretMainCommand,
  P05SecretMainCommand,
} from '../shared/p05-validation.js';
import type {
  P07CoreParams,
  P07CoreResult,
  P07MainCommand,
  P07ReadParams,
} from '../shared/p07-validation.js';
import type { P04CoreCommand, P04CoreParams, P04CoreResult } from '../shared/p04-validation.js';
import {
  isToolProjectionKind,
  modelProjectionRouteKey,
  toolProjectionRouteKey,
  type ModelProjectionRouteRequest,
  type ToolProjectionChange,
  type ToolProjectionRouteRequest,
} from '../shared/projection-validation.js';
import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/agent-validation.js';
import {
  CORE_RESPONSIVENESS,
  CORE_STARTUP_PROGRESS_INTERVAL_MS,
  type CoreHealthProjectionChange,
  type CoreSupervisorOptions,
  HANDSHAKE_TIMEOUT_MS,
  LATE_RECONNECT_CLOSE_BUDGET_MS,
  READINESS_TIMEOUT_MS,
  RESUME_GRACE_MS,
  STARTUP_CONNECT_TIMEOUT_MS,
  type WorkspaceFilesProjectionChange,
  type WorkspaceSessionProjectionChange,
  sanitizeCoreStderr,
} from './core-supervisor/config.js';
import {
  CompatibilityFailure,
  CoreStartupStageFailure,
  type CoreStartupTimings,
  PairingInvalidated,
  ReconnectBudgetExhausted,
  ReconnectDeadlineExpired,
  coreExitBeforeReadyError,
  readErrorCode,
  readFailureEnvelope,
  startupAttemptEndedError,
  startupBudgetExceededError,
  startupFailureIsAbandoned,
  startupFailureDetail,
} from './core-supervisor/errors.js';
import {
  delay,
  frozenCoreHandshake,
  isNativeProcessIdentityFailure,
  parseHeartbeat,
  parseStartupProgress,
  sameRuntimeHandshake,
} from './core-supervisor/handshake.js';
import {
  isModelProjectionChange,
  localProjection,
  localWorkspaceFilesProjection,
  localWorkspaceSessionProjection,
  nextLocalProjection,
  p05SecretOperation,
  sameCoreHealthPayload,
  sameWorkspaceFilesPayload,
  sameWorkspaceSessionPayload,
  nextWithCoreWaiting,
  withCoreWaiting,
} from './core-supervisor/projections.js';
export { STARTUP_CONNECT_TIMEOUT_MS, HANDSHAKE_TIMEOUT_MS } from './core-supervisor/config.js';
export type { CoreSupervisorOptions } from './core-supervisor/config.js';
import { CORE_UNREACHABLE_MESSAGE } from './core-unavailable.js';
import { RevisionedProjection } from './core-supervisor/revisioned-projection.js';
import { ConfirmedCommands } from './core-supervisor/confirmed-commands.js';
import {
  mainRuntimeHandshake,
  validateCoreHandshake,
} from './core-supervisor/handshake-validation.js';
export type { CoreStartupTimings } from './core-supervisor/errors.js';

export class CoreSupervisor {
  readonly domains = new CoreDomainRequests((options) => this.#commands.run(options));
  /**
   * Identity-only client-lifecycle observers. A PRODUCT RUN REGISTERS NONE, and the fan-out below is
   * then never handed to a transport, so no record is even built: the seam is inert unless the
   * acceptance layer explicitly attaches to it.
   */
  readonly #lifecycleObservers = new Set<(record: CoreTransportLifecycleRecord) => void>();
  /**
   * Stable fan-out. The transport owns the failure accounting: a sink that throws is counted there and
   * cannot fail a request, so nothing is swallowed twice.
   */
  readonly #lifecycleFanout = (record: CoreTransportLifecycleRecord): void => {
    for (const sink of this.#lifecycleObservers) sink(record);
  };
  /** Every normal command and its bounded outcome confirmation (core-supervisor/confirmed-commands.ts). */
  readonly #commands = new ConfirmedCommands({
    transport: () => this.#transport,
    coreGeneration: () => this.#coreGeneration,
    coreRuntimeInstanceId: () => this.#coreRuntimeInstanceId,
    reconnecting: () => this.#reconnectPromise,
    log: (event, fields) => this.#log(event, fields),
    queryCommandOutcome: (commandInvocationId) => this.queryCommandOutcome(commandInvocationId),
  });
  readonly #securityListeners = new Set<(change: SecurityProjectionChange) => void>();
  readonly #modelListeners = new Set<(change: ModelProjectionChange) => void>();
  readonly #modelScopedSubscriptions = new Map<string, ModelProjectionRouteRequest>();
  readonly #toolListeners = new Set<(change: ToolProjectionChange) => void>();
  readonly #toolScopedSubscriptions = new Map<string, ToolProjectionRouteRequest>();
  readonly #mainRuntimeInstanceId = createAuraId('run');
  readonly #options: CoreSupervisorOptions;
  #child: ChildProcess | undefined;
  #stoppingChild: ChildProcess | undefined;
  /** Exit observed while the startup budgets are still running, if any. */
  #childExit: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  #startupStartedAt = 0;
  /** Ends the whole startup phase on stop, a new generation, or a child exit. */
  #startupAbort: AbortController | undefined;
  /** The attempt that owns the current generation's connect, handshake, and pre-activation transport. */
  #startupAttempt: StartupAttempt | undefined;
  /** The same-process reconnect that has not activated its transport yet. */
  #reconnectAttempt: StartupAttempt | undefined;
  /** Bounded Core stderr tail; the reason a Core refuses to start is not visible anywhere else. */
  #coreStderr = '';
  #activeConnectionGeneration = 0;
  #connectionGeneration = 0;
  #connectionEpoch = 0;
  #coreGeneration = 0;
  #coreHandshake: RuntimeHandshake | undefined;
  #corePeerIdentity:
    | {
        serverCreationTime: string;
        serverPid: number;
        serverWindowsSessionId: number;
      }
    | undefined;
  #corePid: number | undefined;
  #coreRuntimeInstanceId = createAuraId('run');
  readonly #exitRecovery = new CoreExitRecovery();
  readonly #projectionCompatibility = new ProjectionCompatibility(
    (code, key) =>
      void this.#log('core_projection_rejected', { code, ...(key === undefined ? {} : { key }) }),
  );
  /** Core's heartbeats and what their silence leads to (core-supervisor/heartbeat-watch.ts). */
  readonly #heartbeats: HeartbeatWatch;
  /** Why Main itself is ending the current Core, until its exit is handled. */
  #ending: CoreRestartCause | undefined;
  #lastHeartbeatSequence: number | undefined;
  #lastStartupSequence: number | undefined;
  #pipeNonce: string | undefined;
  #readyDeadline: NodeJS.Timeout | undefined;
  #reconnectPromise: Promise<void> | undefined;
  #resumeGraceUntil = 0;
  #securitySupported = false;
  #modelSupported = false;
  #toolSupported = false;
  #toolCredentialsSupported = false;
  #stopping = false;
  #suspended = false;
  #transport: CoreTransport | undefined;
  /**
   * The three projections Main relays from Core and republishes (core-supervisor/revisioned-projection.ts):
   * Core health, Workspace/Session and Workspace Files.
   */
  readonly #coreProjection: RevisionedProjection<CoreHealthProjectionChange>;
  readonly #workspaceSessionProjection: RevisionedProjection<WorkspaceSessionProjectionChange>;
  readonly #workspaceFilesProjection: RevisionedProjection<WorkspaceFilesProjectionChange>;
  #workspaceSessionSupported = false;
  #workspaceFilesSupported = false;

  constructor(options: CoreSupervisorOptions) {
    this.#options = options;
    const host = <Change>(refresh: () => Promise<Change>) => ({
      coreRuntimeInstanceId: () => this.#coreRuntimeInstanceId,
      log: (event: string, fields: Record<string, unknown>) => this.#log(event, fields),
      refresh,
      logRefreshFailure: (projection: string, error: unknown) =>
        this.#logProjectionRefreshFailure(projection, error),
    });
    this.#coreProjection = new RevisionedProjection(
      {
        integerLabel: 'Core projection',
        snapshotLabel: 'Core projection',
        gapEvent: 'core_projection_gap_detected',
        refreshName: 'core_health',
        samePayload: sameCoreHealthPayload,
      },
      {
        ...host(() => this.refreshSnapshot()),
        decorate: (change) => withCoreWaiting(change, this.#heartbeats.waitingSince),
      },
      localProjection(
        this.#coreRuntimeInstanceId,
        'not_started',
        'core.not_started',
        'Aura Core 尚未启动。',
        true,
        1,
        options.safeMode,
        0,
      ),
    );
    this.#workspaceSessionProjection = new RevisionedProjection(
      {
        integerLabel: 'Workspace/Session projection',
        snapshotLabel: 'Workspace/Session',
        gapEvent: 'workspace_session_projection_gap_detected',
        refreshName: 'workspace_session',
        samePayload: sameWorkspaceSessionPayload,
      },
      host(() => this.refreshWorkspaceSessionSnapshot()),
      localWorkspaceSessionProjection(this.#coreRuntimeInstanceId, 1),
    );
    this.#workspaceFilesProjection = new RevisionedProjection(
      {
        integerLabel: 'Workspace Files projection',
        snapshotLabel: 'Workspace Files',
        gapEvent: 'workspace_files_projection_gap_detected',
        refreshName: 'workspace_files',
        samePayload: sameWorkspaceFilesPayload,
      },
      host(() => this.refreshWorkspaceFilesSnapshot()),
      localWorkspaceFilesProjection(this.#coreRuntimeInstanceId, 1),
    );
    this.#projectionCompatibility.reset(this.#coreRuntimeInstanceId);
    const readActivity = processActivityReader(options);
    this.#heartbeats = new HeartbeatWatch(
      {
        paused: () =>
          this.#stopping ||
          this.#suspended ||
          Date.now() < this.#resumeGraceUntil ||
          this.#child?.exitCode !== null,
        log: (event, fields) => this.#log(event, fields),
        waiting: (since) =>
          this.#coreProjection.emit(nextWithCoreWaiting(this.#coreProjection.current, since)),
        deadlocked: () => this.#endCore('unresponsive'),
      },
      { ...CORE_RESPONSIVENESS, ...options.responsiveness },
      () => {
        const peer = this.#corePeerIdentity;
        return peer && readActivity(peer.serverPid, peer.serverCreationTime);
      },
    );
  }

  get snapshot(): CoreHealthProjectionChange {
    return this.#coreProjection.snapshot;
  }

  get workspaceSessionSnapshot(): WorkspaceSessionProjectionChange {
    return this.#workspaceSessionProjection.snapshot;
  }

  get workspaceFilesSnapshot(): WorkspaceFilesProjectionChange {
    return this.#workspaceFilesProjection.snapshot;
  }

  get projectionCompatibilitySnapshot(): ProjectionCompatibilitySnapshotV1 {
    return this.#projectionCompatibility.snapshot();
  }

  subscribeProjectionCompatibility(
    listener: (snapshot: ProjectionCompatibilitySnapshotV1) => void,
  ): () => void {
    return this.#projectionCompatibility.subscribe(listener);
  }

  subscribe(listener: (change: CoreHealthProjectionChange) => void): () => void {
    return this.#coreProjection.subscribe(listener);
  }

  subscribeWorkspaceSessions(
    listener: (change: WorkspaceSessionProjectionChange) => void,
  ): () => void {
    return this.#workspaceSessionProjection.subscribe(listener);
  }

  subscribeWorkspaceFiles(listener: (change: WorkspaceFilesProjectionChange) => void): () => void {
    return this.#workspaceFilesProjection.subscribe(listener);
  }

  subscribeSecurity(listener: (change: SecurityProjectionChange) => void): () => void {
    this.#securityListeners.add(listener);
    return () => this.#securityListeners.delete(listener);
  }

  subscribeModels(listener: (change: ModelProjectionChange) => void): () => void {
    this.#modelListeners.add(listener);
    return () => this.#modelListeners.delete(listener);
  }

  subscribeTools(listener: (change: ToolProjectionChange) => void): () => void {
    this.#toolListeners.add(listener);
    return () => this.#toolListeners.delete(listener);
  }

  async requestToolCredential<C extends ToolCredentialMainCommand>(
    command: C,
    params: ToolCredentialMainParams[C],
    secret?: Buffer,
  ): Promise<ToolCredentialMainResult[C]> {
    return requestToolCredential(
      command,
      params,
      secret,
      this.#toolCredentialsSupported,
      (options) => this.#commands.run(options),
    );
  }
  async requestAgentSecret<C extends Parameters<typeof requestAgentSecret>[0]>(
    command: C,
    params: AgentMainParams[C],
    secret: Buffer,
  ): Promise<AgentMainResult[C]> {
    return requestAgentSecret(command, params, secret, (o) => this.#commands.run(o));
  }

  async requestConfigRecovery<Command extends ConfigRecoveryMainCommand>(
    command: Command,
    params: ConfigRecoveryMainParams[Command],
  ): Promise<ConfigRecoveryMainResult[Command]> {
    let invocation: CommandInvocationId | undefined;
    if (!isRecoveryParams(command, params))
      throw new Error('Invalid configuration recovery parameters');
    const result = await this.#commands.run({
      command,
      accept: (value) => {
        try {
          return parseRecoveryResult(command, value);
        } catch {
          if (command !== 'config.recovery.apply')
            throw new Error('Invalid configuration recovery response.');
          throw new CoreRpcError({
            ...errorEnvelope(
              'command.outcome_unknown',
              'Recovery response could not be confirmed.',
              'failed',
            ),
            details: {
              outcome_recovery: 'query_runtime.command_outcome_without_replay',
              ...(invocation ? { command_invocation_id: invocation } : {}),
            },
          });
        }
      },
      send: (transport) =>
        transport.request(command, params, 1, 15_000, (id) => {
          invocation = id;
        }),
    });
    if (command === 'config.recovery.apply' && this.#transport !== undefined) {
      // Recovery may have supplied previously absent services. Subscribe their real projections
      // now, without replaying the already-settled mutation if a read-only refresh fails.
      await this.#synchronizeProjections(this.#transport).catch(() =>
        this.#log('recovery_projection_refresh_failed', { code: 'projection.refresh_unavailable' }),
      );
    }
    return result;
  }

  async requestP03<Command extends P03CoreCommand>(
    command: Command,
    params: P03CoreParams[Command],
  ): Promise<P03CoreResult[Command]> {
    this.#require(this.#workspaceFilesSupported, 'P03 Workspace Files');
    return this.#commands.run({
      accept: (value) => value as P03CoreResult[Command],
      command,
      send: (transport) => transport.request(command, params, 1, replyBudget(command)),
    });
  }

  async requestP04<Command extends P04CoreCommand>(
    command: Command,
    params: P04CoreParams[Command],
  ): Promise<P04CoreResult[Command]> {
    this.#require(this.#securitySupported, 'P04 Security Control Plane');
    return this.#commands.run({
      accept: (value) => value as P04CoreResult[Command],
      command,
      send: (transport) => transport.request(command, params, 1, replyBudget(command)),
    });
  }

  async requestP05<Command extends P05NonSecretMainCommand>(
    command: Command,
    params: P05MainParams[Command],
  ): Promise<P05MainResult[Command]> {
    this.#require(this.#modelSupported, 'P05 Model Control Plane');
    return this.#commands.run({
      accept: (value) => value as P05MainResult[Command],
      command,
      send: (transport) => transport.request(command, params),
    });
  }

  async requestP05WithSecretInput<Command extends P05SecretMainCommand>(
    command: Command,
    params: P05MainParams[Command],
    secret: Buffer,
  ): Promise<P05MainResult[Command]> {
    if (!this.#modelSupported) {
      secret.fill(0);
      throw new Error('Core does not support the P05 Model Control Plane interface');
    }
    if (this.#transport === undefined) {
      secret.fill(0);
      throw new Error(CORE_UNREACHABLE_MESSAGE);
    }
    // The credential bytes are handed to the transport ONCE, here, and the confirmation that may
    // follow never receives them: it is keyed by the invocation id alone, so no plaintext is
    // retained anywhere in order to learn the result and there is nothing to retry with.
    return this.#commands.run({
      accept: (value) => value as P05MainResult[Command],
      command,
      send: (transport) =>
        transport.requestWithSecretInput(command, params, p05SecretOperation(command), secret),
    });
  }

  async requestP07<Command extends P07MainCommand>(
    command: Command,
    params: P07CoreParams[Command],
  ): Promise<P07CoreResult[Command]> {
    this.#require(this.#toolSupported, 'P07 Tool Control Plane');
    return this.#commands.run({
      accept: (value) => value as P07CoreResult[Command],
      command,
      send: (transport) => transport.request(command, params),
    });
  }

  async requestAgent<Command extends AgentMainCommand>(
    command: Command,
    params: AgentMainParams[Command],
    onRequestId?: (id: CorrelationId) => void,
    onQueuedOpening?: (cancelIfUnsent: () => boolean) => void,
  ): Promise<AgentMainResult[Command]> {
    return requestAgentCommand(
      command,
      params,
      (options) => this.#commands.run(options),
      onRequestId,
      onQueuedOpening,
    );
  }

  /**
   * The longest caller budget `requestWithBudget` accepts.
   *
   * Deliberately the same number as `CoreTransport.request`'s own 15 s default and never larger: a
   * caller-budget seam that could outlive the production default would be a second, silent default.
   */
  static readonly MAX_CALLER_BUDGET_MS = 15_000;

  /**
   * One authenticated request with an explicit caller budget.
   *
   * Additive and narrow on purpose: it is the ONLY way the acceptance layer can shorten a single
   * designated request's wait. It goes through `CoreTransport.request` - the same authentication,
   * ledger, claim, router, reply and outcome-confirmation path as every product request. Only the
   * original reply budget changes; confirmation retains its own existing bounded policy. There is
   * no priority change, retry, replay or new wire method. The production default stays 15 s.
   */
  async requestWithBudget(
    method: string,
    params: unknown,
    timeoutMilliseconds: number,
  ): Promise<unknown> {
    if (!Number.isSafeInteger(timeoutMilliseconds) || timeoutMilliseconds < 1) {
      throw new Error('a caller budget must be a positive safe integer of milliseconds');
    }
    if (timeoutMilliseconds > CoreSupervisor.MAX_CALLER_BUDGET_MS) {
      throw new Error(
        `a caller budget must not exceed the production default of ` +
          `${CoreSupervisor.MAX_CALLER_BUDGET_MS}ms`,
      );
    }
    return this.#commands.run({
      accept: (value) => value,
      command: method,
      send: (transport) => transport.request(method, params, 1, timeoutMilliseconds),
    });
  }

  /**
   * Send the existing `aura.cancel` for one real correlation id.
   *
   * The transport has always been able to send this; nothing reached it, so the runtime's own
   * "cancellation accepted" answer was unreachable from Main. This delegates to that one method
   * instead of inventing a second cancellation path.
   */
  /** The authenticated channel; without one a request is refused before anything is sent. */
  #activeTransport(): CoreTransport {
    const transport = this.#transport;
    if (transport === undefined) throw new Error(CORE_UNREACHABLE_MESSAGE);
    return transport;
  }

  async cancel(requestId: CorrelationId): Promise<unknown> {
    const transport = this.#activeTransport();
    return transport.cancel(requestId);
  }

  /**
   * Ask the Core for the authoritative outcome of one CommandInvocationId.
   *
   * Same delegation: `CoreTransport.queryCommandOutcome` is the product's only implementation of
   * `runtime.command_outcome`, and until now nothing called it, so the recovery instruction the
   * transport itself carries ("query runtime.command_outcome_without_replay") was unreachable.
   */
  async queryCommandOutcome(commandInvocationId: CommandInvocationId): Promise<CommandOutcome> {
    const transport = this.#activeTransport();
    return transport.queryCommandOutcome(commandInvocationId);
  }

  /**
   * Attach an identity-only client-lifecycle observer; the returned function detaches it.
   *
   * WHAT AN OBSERVER SEES: the correlation id, the CommandInvocationId, the method, the send stage,
   * and this channel's id/epoch - all copied from the real transport, and nothing else (the transport
   * builds the record from a type that has no member for params, a secret or a nonce).
   *
   * WHAT IT CANNOT DO: it never answers, settles or reorders a request; the transport only counts a
   * throwing sink. A product run never calls this, so no record is built and the send path is
   * unchanged.
   */
  observeClientLifecycle(sink: (record: CoreTransportLifecycleRecord) => void): () => void {
    if (typeof sink !== 'function')
      throw new Error('a client lifecycle observer must be a function');
    this.#lifecycleObservers.add(sink);
    this.#transport?.attachLifecycleObserver(this.#lifecycleFanout);
    let detached = false;
    return () => {
      if (detached) return;
      detached = true;
      this.#lifecycleObservers.delete(sink);
      if (this.#lifecycleObservers.size === 0) {
        this.#transport?.attachLifecycleObserver(undefined);
      }
    };
  }

  /**
   * The authenticated channel/epoch this supervisor's active transport really holds, or `null` when
   * there is no active connection. Read-only identity; no request content.
   */
  channelFacts(): CoreTransportChannelFacts | null {
    return this.#transport?.channelFacts() ?? null;
  }

  /**
   * The active transport's own request-registry counters, or `null` when there is no active
   * connection.
   *
   * COUNTS ONLY - how many requests are active, how many legal late terminals the transport really
   * consumed, how many ids still await their real terminal, how many admissions were refused. There is
   * no request id, no method, no params and no credential in it, and it answers no request.
   *
   * It exists because the acceptance layer must be able to read the TRANSPORT'S OWN account of "the
   * designated request's terminal arrived and was consumed" instead of inferring it from a released
   * barrier or from another request that happened to succeed.
   */
  requestStats(): ReturnType<CoreTransport['requestStats']> | null {
    return this.#transport?.requestStats() ?? null;
  }

  /**
   * Sever ONLY the connection this Main process owns.
   *
   * It closes the active transport - which is what the production transport-loss path already acts on
   * - and stops there: the Core process, the pipe nonce, the peer identity and the frozen handshake
   * are deliberately kept, so the existing same-process re-authentication runs and the Core keeps
   * serving. It is NOT a Core restart, and it kills nothing.
   */
  closeActiveConnection(): void {
    const transport = this.#transport;
    if (transport === undefined) {
      throw new Error('there is no active Core connection to close');
    }
    transport.close();
  }

  async subscribeModelProjection(route: ModelProjectionRouteRequest): Promise<void> {
    this.#require(this.#modelSupported, 'P05 Model Control Plane');
    if (route.projection_scope.type === 'global') return;
    const key = modelProjectionRouteKey(route);
    if (this.#modelScopedSubscriptions.has(key)) return;
    const transport = this.#activeTransport();
    const subscription: ProjectionSubscription = {
      ...route,
      after_revision: null,
      known_runtime_instance_id: this.#coreRuntimeInstanceId,
    } as ProjectionSubscription;
    await transport.request('projection.subscribe', subscription);
    this.#modelScopedSubscriptions.set(key, structuredClone(route));
  }

  async unsubscribeModelProjection(route: ModelProjectionRouteRequest): Promise<void> {
    if (route.projection_scope.type === 'global') return;
    const key = modelProjectionRouteKey(route);
    if (!this.#modelScopedSubscriptions.has(key)) return;
    this.#modelScopedSubscriptions.delete(key);
    const transport = this.#transport;
    if (transport === undefined) return;
    await transport.request('projection.unsubscribe', route as ProjectionUnsubscribeRequest);
  }

  async subscribeToolProjection(route: ToolProjectionRouteRequest): Promise<void> {
    this.#require(this.#toolSupported, 'P07 Tool Control Plane');
    const key = toolProjectionRouteKey(route);
    if (this.#toolScopedSubscriptions.has(key)) return;
    const transport = this.#activeTransport();
    const subscription: ProjectionSubscription = {
      ...route,
      after_revision: null,
      known_runtime_instance_id: this.#coreRuntimeInstanceId,
    } as ProjectionSubscription;
    this.#toolScopedSubscriptions.set(key, structuredClone(route));
    try {
      await transport.request('projection.subscribe', subscription);
    } catch (error) {
      this.#toolScopedSubscriptions.delete(key);
      throw error;
    }
  }

  async unsubscribeToolProjection(route: ToolProjectionRouteRequest): Promise<void> {
    const key = toolProjectionRouteKey(route);
    if (!this.#toolScopedSubscriptions.has(key)) return;
    this.#toolScopedSubscriptions.delete(key);
    const transport = this.#transport;
    if (transport === undefined) return;
    await transport.request('projection.unsubscribe', route as ProjectionUnsubscribeRequest);
  }

  async refreshToolProjection(route: ToolProjectionRouteRequest): Promise<ToolProjectionChange> {
    this.#require(this.#toolSupported, 'P07 Tool Control Plane');
    const transport = this.#activeTransport();
    const request: ProjectionSnapshotRequest = {
      ...route,
      known_runtime_instance_id: this.#coreRuntimeInstanceId,
    } as ProjectionSnapshotRequest;
    const value = await transport.request('projection.snapshot', request);
    const projection = this.#projectionCompatibility.read(
      value,
      this.#coreRuntimeInstanceId,
      route,
    );
    if (
      projection === undefined ||
      !isToolProjectionKind(projection.projection_kind) ||
      toolProjectionRouteKey({
        projection_kind: projection.projection_kind,
        projection_scope: projection.projection_scope,
      } as ToolProjectionRouteRequest) !== toolProjectionRouteKey(route)
    ) {
      throw new Error('Core returned an invalid Tool Projection snapshot');
    }
    return projection as ToolProjectionChange;
  }

  async readRuntimeReadiness() {
    const transport = this.#activeTransport();
    return parseRuntimeReadiness(
      await transport.request('runtime.readiness', {}, 1, READINESS_TIMEOUT_MS),
    );
  }

  async readToolActivityPage(request: ToolActivityPageRequestV1): Promise<ToolActivityPageV1> {
    this.#require(this.#toolSupported, 'P07 Tool Control Plane');
    const transport = this.#activeTransport();
    return (await transport.request('tools.activity.page', request)) as ToolActivityPageV1;
  }

  async readToolCredentialCandidates(
    request: P07ReadParams['tools.credential.candidates'],
  ): Promise<ToolCredentialCandidatesProjectionV1> {
    this.#require(this.#toolSupported, 'P07 Tool Control Plane');
    const transport = this.#activeTransport();
    return (await transport.request(
      'tools.credential.candidates',
      request,
    )) as ToolCredentialCandidatesProjectionV1;
  }

  async start(): Promise<void> {
    if (this.#stoppingChild !== undefined) await waitForChildExit(this.#stoppingChild);
    this.#stoppingChild = undefined;
    this.#stopping = false;
    await this.#spawnCore(0);
  }

  stop(): void {
    this.#stopping = true;
    this.#startupAttempt?.abandon();
    this.#reconnectAttempt?.abandon();
    this.#startupAbort?.abort();
    this.#clearTimers();
    this.#connectionGeneration += 1;
    this.#activeConnectionGeneration = 0;
    this.#transport?.close();
    this.#transport = undefined;
    this.#stoppingChild = this.#child ?? this.#stoppingChild;
    this.#child?.kill();
    this.#child = undefined;
  }

  async stopAndWait(): Promise<void> {
    const child = this.#child ?? this.#stoppingChild;
    // Register first: a fast exit must not race the observer. Preserve the
    // existing synchronous stop entry for desktop shutdown callers.
    const stopped = child === undefined ? Promise.resolve() : waitForChildExit(child);
    this.stop();
    await stopped;
  }

  /**
   * The user's 立即重启 while the window shows Main waiting for Core: ends that Core, and the exit
   * path starts a new one at once. A Core that answers again is left alone.
   */
  restartNow(): boolean {
    if (this.#heartbeats.waitingSince === undefined) return false;
    void this.#log('core_restart_requested', { silenceMs: this.#heartbeats.silenceMs() });
    return this.#endCore('user_restart');
  }

  /** Main ends the Core it owns; the window says so until the exit arrives. */
  #endCore(cause: 'unresponsive' | 'user_restart'): boolean {
    const child = this.#child;
    if (this.#stopping || child === undefined || child.exitCode !== null) return false;
    this.#ending = cause;
    this.#heartbeats.stop();
    this.#coreProjection.emit(
      nextLocalProjection(
        this.#coreProjection.current,
        'restarting',
        cause === 'user_restart' ? 'core.restart_requested' : 'core.unresponsive_restart',
        cause === 'user_restart'
          ? '正在重启 Aura Core…'
          : 'Aura Core 已 1 分钟没有回应，进程也没有在运行，正在重启…',
        true,
      ),
    );
    child.kill();
    return true;
  }

  onSuspend(): void {
    this.#suspended = true;
    if (this.#readyDeadline !== undefined) clearTimeout(this.#readyDeadline);
    this.#readyDeadline = undefined;
  }

  onResume(): void {
    this.#suspended = false;
    this.#resumeGraceUntil = Date.now() + RESUME_GRACE_MS;
    this.#heartbeats.reset();
    if (
      this.#coreProjection.current.payload.state !== 'ready' &&
      this.#coreProjection.current.payload.state !== 'recovery_required'
    ) {
      this.#resetReadyDeadline(RESUME_GRACE_MS);
    }
  }

  async refreshSnapshot(): Promise<CoreHealthProjectionChange> {
    return this.#refreshProjection(this.#coreProjection, 'core_health');
  }

  async refreshWorkspaceSessionSnapshot(): Promise<WorkspaceSessionProjectionChange> {
    return this.#refreshProjection(this.#workspaceSessionProjection, 'workspace_session');
  }

  async refreshWorkspaceFilesSnapshot(): Promise<WorkspaceFilesProjectionChange> {
    return this.#refreshProjection(this.#workspaceFilesProjection, 'workspace_files');
  }

  /**
   * Reads Core's snapshot of one relayed projection and applies it, unless the channel it was read
   * on is no longer the active one. The revision known before the read is what a reply older than a
   * newer increment is measured against.
   */
  async #refreshProjection<
    Change extends
      | CoreHealthProjectionChange
      | WorkspaceSessionProjectionChange
      | WorkspaceFilesProjectionChange,
  >(projection: RevisionedProjection<Change>, kind: Change['projection_kind']): Promise<Change> {
    const transport = this.#transport;
    if (transport === undefined) return projection.snapshot;
    const revisionAtRequest = projection.sourceRevision;
    const value = await transport.request('projection.snapshot', {
      known_runtime_instance_id: projection.current.runtime_instance_id,
      projection_kind: kind,
      projection_scope: { type: 'global' },
    });
    const snapshot = this.#projectionCompatibility.read(value, this.#coreRuntimeInstanceId, {
      projection_kind: kind,
      projection_scope: { type: 'global' },
    });
    if (snapshot?.projection_kind !== kind) return projection.snapshot;
    if (this.#stopping || this.#transport !== transport) return projection.snapshot;
    projection.applySnapshot(snapshot as Change, revisionAtRequest);
    return projection.snapshot;
  }

  async #spawnCore(restartAttempt: number): Promise<void> {
    if (this.#stopping) return;
    this.#coreGeneration += 1;
    const generation = this.#coreGeneration;
    this.#connectionGeneration += 1;
    this.#activeConnectionGeneration = 0;
    this.#connectionEpoch = 0;
    this.#coreHandshake = undefined;
    this.#corePeerIdentity = undefined;
    this.#corePid = undefined;
    this.#pipeNonce = undefined;
    this.#coreRuntimeInstanceId = createAuraId('run');
    this.#projectionCompatibility.reset(this.#coreRuntimeInstanceId);
    this.#coreProjection.resetSourceRevision();
    this.#workspaceSessionProjection.resetSourceRevision();
    this.#workspaceSessionSupported = false;
    this.#workspaceFilesProjection.resetSourceRevision();
    this.#workspaceFilesSupported = false;
    this.#securitySupported = false;
    this.#modelSupported = false;
    this.#toolSupported = false;
    this.#toolCredentialsSupported = false;
    this.#lastHeartbeatSequence = undefined;
    this.#lastStartupSequence = undefined;
    this.#coreProjection.emit(
      localProjection(
        this.#coreRuntimeInstanceId,
        'starting',
        'core.starting',
        '正在启动 Aura Core…',
        true,
        1,
        this.#options.safeMode,
        restartAttempt,
      ),
    );
    this.#workspaceSessionProjection.emit(
      localWorkspaceSessionProjection(this.#coreRuntimeInstanceId, 1),
    );
    this.#workspaceFilesProjection.emit(
      localWorkspaceFilesProjection(this.#coreRuntimeInstanceId, 1),
    );
    const pipeNonce = randomBytes(16).toString('hex');
    this.#pipeNonce = pipeNonce;
    const pipeName = `\\\\.\\pipe\\Aura\\${this.#options.release.namespaceObjectHash}\\${this.#coreRuntimeInstanceId}\\${pipeNonce}`;
    const bootstrapSecret = randomBytes(32);
    const executable =
      process.env.AURA_E2E === '1' && process.env.AURA_E2E_CORE_MODE === 'missing'
        ? resolve(this.#options.release.versionDirectory, 'missing-aura-core.exe')
        : this.#options.release.coreExecutable;
    const coreArguments = [
      `--pipe-name=${pipeName}`,
      '--bootstrap-stdin',
      `--parent-pid=${process.pid}`,
      `--runtime-instance-id=${this.#coreRuntimeInstanceId}`,
      `--component-manifest=${this.#options.release.manifestPath}`,
    ];
    if (this.#options.release.allowDevMismatch)
      coreArguments.push('--allow-dev-component-mismatch');
    if (this.#options.safeMode) coreArguments.push('--safe-mode');
    const testMode = process.env.AURA_E2E === '1' ? process.env.AURA_E2E_CORE_MODE : undefined;
    if (
      testMode !== undefined &&
      testMode !== 'normal' &&
      testMode !== 'missing' &&
      (testMode !== 'crash-after-handshake' || restartAttempt === 0)
    ) {
      coreArguments.push(`--test-mode=${testMode}`);
    }
    coreArguments.push(...this.#exitRecovery.spawning());

    this.#childExit = undefined;
    // Monotonic: the startup budgets are measured on a clock that cannot step.
    // A new Core generation also retires a reconnect that has not activated.
    this.#reconnectAttempt?.abandon();
    this.#startupAttempt?.abandon();
    this.#startupAttempt = new StartupAttempt(generation);
    this.#startupStartedAt = this.#startupAttempt.startedAt;
    this.#startupAbort?.abort();
    this.#startupAbort = this.#startupAttempt.abort;
    this.#coreStderr = '';
    const child = spawn(executable, coreArguments, {
      cwd: dirname(executable),
      detached: false,
      shell: false,
      // Both pipes are drained: stderr feeds the bounded startup diagnostic.
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
      // A development run launches a Core that may itself be a published one, and
      // the execution installation publishes execution objects only. The Release
      // Bundle's own components (Sandbox Host, Egress Proxy, Elevated Helper) are
      // then not siblings of that Core, so the directory that holds them is named
      // explicitly. Only ever set for a development run: a packaged Core must not be
      // pointable at another directory by an environment variable. Core re-verifies
      // every component's hash and protocol against the manifest, so this chooses a
      // location, never a content.
      env: this.#options.release.componentRoot
        ? { ...process.env, AURA_DEV_COMPONENT_ROOT: this.#options.release.componentRoot }
        : process.env,
    });
    this.#child = child;
    child.stdin?.on('error', () => {
      // Process startup and handshake paths report the authoritative failure.
    });
    child.stdin?.end(bootstrapSecret);
    child.stdout?.on('data', () => {
      // The Core protocol runs on the pipe; a readable stdout is drained only to
      // keep the child from blocking on a full pipe.
    });
    child.stderr?.on('data', (chunk: Buffer | string) => {
      this.#coreStderr = sanitizeCoreStderr(`${this.#coreStderr}${String(chunk)}`);
    });
    child.once('error', (error) => {
      if (generation !== this.#coreGeneration || this.#stopping) return;
      void this.#log('core_spawn_failed', { message: error.message });
      this.#coreProjection.emit(
        localProjection(
          this.#coreRuntimeInstanceId,
          'unavailable',
          'core.executable_missing',
          '未找到或无法启动 aura-core.exe。',
          true,
          2,
          this.#options.safeMode,
          restartAttempt,
        ),
      );
    });
    child.once('exit', (code, signal) => {
      // A process that exits while the startup budgets run is reported as an
      // exit reason instead of waiting the remaining budget out.
      this.#childExit = { code, signal };
      this.#startupAttempt?.abandon();
      this.#reconnectAttempt?.abandon();
      this.#startupAbort?.abort();
      void this.#handleCoreExit(generation, code, signal);
    });
    void this.#log('core_spawned', { generation, restartAttempt });

    try {
      if (child.pid === undefined) {
        await this.#log('core_spawn_failed', {
          message: 'Aura Core process did not expose a process identifier',
        });
        this.#coreProjection.emit(
          localProjection(
            this.#coreRuntimeInstanceId,
            'unavailable',
            'core.executable_missing',
            '未找到或无法启动 aura-core.exe。',
            true,
            2,
            this.#options.safeMode,
            restartAttempt,
          ),
        );
        return;
      }
      this.#corePid = child.pid;
      // The image this generation actually loaded, recorded with the bound Core
      // binding so a reader can compare "which file was selected" with "which file
      // is running" without inferring either from the other.
      void this.#log('core_image', {
        generation,
        restartAttempt,
        pid: child.pid,
        executable,
        binding_state: this.#options.release.coreBinding.state,
        binding_source: this.#options.release.coreBinding.source,
        binding_refusal: this.#options.release.coreBinding.refusal,
        binding_mismatched_components: this.#options.release.coreBinding.mismatchedComponents,
        binding_unknown_components: this.#options.release.coreBinding.unknownComponents ?? [],
        installation_id: this.#options.release.coreBinding.installationId,
      });
      const timings = await this.#connectAndHandshake(
        generation,
        pipeNonce,
        bootstrapSecret,
        child.pid,
      );
      void this.#log('core_startup_timings', { generation, restartAttempt, ...timings });
    } catch (error) {
      if (generation !== this.#coreGeneration || this.#stopping) return;
      if (this.#childExit !== undefined && startupFailureIsAbandoned(error)) return;
      if (error instanceof CompatibilityFailure) {
        this.#connectionGeneration += 1;
        this.#transport?.close();
        this.#transport = undefined;
        this.#coreProjection.emit(
          localProjection(
            this.#coreRuntimeInstanceId,
            'incompatible',
            'core.incompatible',
            error.message,
            true,
            2,
            this.#options.safeMode,
            restartAttempt,
          ),
        );
        child.kill();
        return;
      }
      const diagnostic =
        error instanceof CoreStartupStageFailure
          ? describeCoreStartupFailure(error.stage, error.originalError)
          : describeCoreStartupFailure('runtime_handshake', error);
      await this.#log('core_startup_failed', {
        ...diagnostic,
        elapsedMilliseconds: this.#startupAttempt?.elapsedMilliseconds() ?? 0,
        ...(this.#coreStderr === '' ? {} : { coreStderrTail: this.#coreStderr }),
        ...startupFailureDetail(error),
      });
      this.#coreProjection.emit(
        nextLocalProjection(
          this.#coreProjection.current,
          'restarting',
          diagnostic.statusCode,
          diagnostic.statusMessage,
          true,
        ),
      );
      if (child.exitCode === null && !child.killed) child.kill();
    } finally {
      bootstrapSecret.fill(0);
    }
  }

  /** Three separate budgets: reachability, authenticated handshake, then the
   *  ready deadline that `#resetReadyDeadline` keeps for initialization. */
  async #connectAndHandshake(
    generation: number,
    pipeNonce: string,
    bootstrapSecret: Buffer,
    expectedServerPid: number,
  ): Promise<CoreStartupTimings> {
    const budget = this.#startupTimeouts();
    const connectStartedAt = performance.now();
    // Monotonic for the whole phase: a native connect attempt cannot be cancelled
    // once started, so the phase must abandon one that can no longer fit and must
    // never activate a connection that lands after the phase ended.
    const deadline = connectStartedAt + budget.connectMs;
    let connection:
      | {
          channelId: IpcChannelId;
          connectionGeneration: number;
          transport: CoreTransport;
        }
      | undefined;
    let lastError: unknown;
    let lastErrorCode: string | undefined;
    let attempts = 0;
    const abort = this.#startupAbort;
    const aborted = () =>
      this.#stopping ||
      generation !== this.#coreGeneration ||
      this.#childExit !== undefined ||
      (abort?.signal.aborted ?? false);
    // Progress reporting runs on its own timer, not after a failed attempt: one
    // native attempt can occupy the whole budget without ever returning.
    const progress = setInterval(() => {
      void this.#log('core_startup_progress', {
        phase: 'connect',
        elapsedMilliseconds: Math.round(performance.now() - this.#startupStartedAt),
        attempts,
        ...(lastErrorCode === undefined ? {} : { lastErrorCode }),
      });
    }, CORE_STARTUP_PROGRESS_INTERVAL_MS);
    try {
      while (!aborted()) {
        const remaining = deadline - performance.now();
        if (remaining <= 0) break;
        const channelId = createAuraId('chn') as IpcChannelId;
        attempts += 1;
        const outcome = await settleWithin(
          this.#createTransport(pipeNonce, expectedServerPid, generation, channelId, 1),
          remaining,
          abort?.signal,
          // A late success is drained and closed; it is never handed to the
          // handshake, because the phase that owned it has already ended.
          (late) => late.transport.close(),
        );
        if (outcome.kind === 'value') {
          if (aborted() || performance.now() >= deadline) {
            outcome.value.transport.close();
            break;
          }
          connection = { ...outcome.value, channelId };
          break;
        }
        if (outcome.kind === 'error') {
          lastError = outcome.error;
          lastErrorCode = readErrorCode(outcome.error);
          const pause = Math.min(75, Math.max(0, deadline - performance.now()));
          if (pause > 0) await delay(pause);
          continue;
        }
        break;
      }
    } finally {
      clearInterval(progress);
    }
    if (connection === undefined) {
      const exited = this.#childExit;
      // An early exit is its own reason: waiting out the rest of the startup
      // budget would report a timeout for a process that is already gone.
      if (exited !== undefined) {
        throw new CoreStartupStageFailure('transport_connect', coreExitBeforeReadyError(exited));
      }
      throw new CoreStartupStageFailure(
        'transport_connect',
        startupBudgetExceededError('connect', budget.connectMs, lastError),
      );
    }
    const connectMilliseconds = Math.round(performance.now() - connectStartedAt);
    const { channelId, connectionGeneration, transport } = connection;
    this.#startupAttempt?.holdTransport(transport);
    if (!this.#startupOwns(generation)) {
      transport.close();
      throw new CoreStartupStageFailure('transport_connect', startupAttemptEndedError());
    }
    const handshakeStartedAt = performance.now();
    try {
      let response: RuntimeHandshakeResponse;
      try {
        // Reachability is not identity: the secret proof and the release
        // identity exchange keep their own, shorter budget.
        response = await withDeadline(
          this.#authenticateTransport(transport, 'initial', channelId, 1, bootstrapSecret, () =>
            this.#startupOwns(generation),
          ),
          budget.handshakeMs,
        );
      } finally {
        bootstrapSecret.fill(0);
      }
      if (!this.#startupOwns(generation) || this.#startupAttempt?.releaseForActivation() !== true) {
        transport.close();
        throw new CoreStartupStageFailure('runtime_handshake', startupAttemptEndedError());
      }
      this.#corePeerIdentity = transport.peerIdentity;
      this.#coreHandshake = frozenCoreHandshake(response.handshake, this.#options.release);
      this.#activateTransport(transport, connectionGeneration, 1);
    } catch (error) {
      transport.close();
      if (error instanceof CompatibilityFailure) throw error;
      throw new CoreStartupStageFailure('runtime_handshake', error);
    }
    const handshakeMilliseconds = Math.round(performance.now() - handshakeStartedAt);
    const syncStartedAt = performance.now();
    try {
      await this.#synchronizeProjections(transport, () => this.#startupOwns(generation));
      if (!this.#startupOwns(generation)) {
        throw new CoreStartupStageFailure('initial_projection_sync', startupAttemptEndedError());
      }
      this.#resetReadyDeadline(this.#readyTimeoutMilliseconds());
    } catch (error) {
      if (error instanceof CoreStartupStageFailure) throw error;
      throw new CoreStartupStageFailure('initial_projection_sync', error);
    }
    const projectionSyncMilliseconds = Math.round(performance.now() - syncStartedAt);
    return {
      connectMilliseconds,
      handshakeMilliseconds,
      projectionSyncMilliseconds,
      totalMilliseconds: Math.round(performance.now() - this.#startupStartedAt),
    };
  }

  #startupOwns(generation: number): boolean {
    const attempt = this.#startupAttempt;
    return (
      attempt !== undefined &&
      startupStillOwns(attempt, generation, this.#stopping, this.#childExit !== undefined)
    );
  }

  #startupTimeouts(): { connectMs: number; handshakeMs: number } {
    return {
      connectMs: this.#options.startupTimeouts?.connectMs ?? STARTUP_CONNECT_TIMEOUT_MS,
      handshakeMs: this.#options.startupTimeouts?.handshakeMs ?? HANDSHAKE_TIMEOUT_MS,
    };
  }

  /** Subscribes one relayed global projection after the revision Main already holds. */
  async #subscribeRelayed(
    transport: CoreTransport,
    kind: 'core_health' | 'workspace_session' | 'workspace_files',
    projection: { readonly sourceRevision: number | undefined },
    deadline?: number,
  ): Promise<void> {
    const revision = projection.sourceRevision;
    await this.#projectionStageWithin(
      transport.request('projection.subscribe', {
        after_revision: revision === undefined ? null : String(revision),
        known_runtime_instance_id: this.#coreRuntimeInstanceId,
        projection_kind: kind,
        projection_scope: { type: 'global' },
      }),
      deadline,
    );
  }

  async #subscribeModelGlobals(
    transport: CoreTransport,
    owns: () => boolean,
    deadline?: number,
  ): Promise<void> {
    for (const projectionKind of ['model_settings', 'provider_catalog'] as const) {
      if (!owns()) return;
      await this.#projectionStageWithin(
        transport.request('projection.subscribe', {
          after_revision: null,
          known_runtime_instance_id: this.#coreRuntimeInstanceId,
          projection_kind: projectionKind,
          projection_scope: { type: 'global' },
        }),
        deadline,
      );
    }
  }

  /** Subscribes again the scoped Model or Tool projections a window asked for. */
  async #subscribeScoped(
    transport: CoreTransport,
    routes: Iterable<ModelProjectionRouteRequest | ToolProjectionRouteRequest>,
    owns: () => boolean,
    deadline?: number,
  ): Promise<void> {
    for (const route of routes) {
      if (!owns()) return;
      const subscription: ProjectionSubscription = {
        ...route,
        after_revision: null,
        known_runtime_instance_id: this.#coreRuntimeInstanceId,
      } as ProjectionSubscription;
      await this.#projectionStageWithin(
        transport.request('projection.subscribe', subscription),
        deadline,
      );
    }
  }

  /**
   * Runs one projection-sync stage against the recovery's absolute deadline.
   * The stage RPC cannot be cancelled once it has been sent, so the stage is
   * raced against the time the recovery has left; on timeout the classified
   * deadline error is thrown and the caller closes the channel and cancels the
   * recovery owner, leaving the late reply on a channel that is already closed.
   */
  async #projectionStageWithin<Value>(stage: Promise<Value>, deadline?: number): Promise<Value> {
    if (deadline === undefined) return stage;
    const remaining = Math.max(0, deadline - performance.now());
    if (remaining <= 0) throw this.#recoveryExpired();
    let timer: NodeJS.Timeout | undefined;
    try {
      return await Promise.race([
        stage,
        new Promise<never>((_resolve, reject) => {
          timer = setTimeout(() => reject(this.#recoveryExpired()), remaining);
        }),
      ]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
    }
  }

  async #synchronizeProjections(
    transport: CoreTransport,
    owns: () => boolean = () => true,
    deadline?: number,
  ): Promise<void> {
    if (!owns()) return;
    await this.#subscribeRelayed(transport, 'core_health', this.#coreProjection, deadline);
    if (!owns()) return;
    await this.#projectionStageWithin(this.refreshSnapshot(), deadline);
    if (!owns()) return;
    const notice =
      (this.#coreHandshake?.supported_features.includes('aura.config.recovery.v1') === true ||
        this.#coreProjection.current.payload.status_code === 'core.ready_degraded') &&
      this.#coreProjection.current.payload.state !== 'recovery_required'
        ? parseRecoveryResult(
            'config.recovery.notice',
            await this.#projectionStageWithin(
              transport.request('config.recovery.notice', {}),
              deadline,
            ),
          )
        : undefined;
    const available = recoveryProjectionPlan(notice);
    if (!owns()) return;
    if (
      this.#workspaceSessionSupported &&
      this.#coreProjection.current.payload.state !== 'recovery_required'
    ) {
      await this.#subscribeRelayed(
        transport,
        'workspace_session',
        this.#workspaceSessionProjection,
        deadline,
      );
      if (!owns()) return;
      await this.#projectionStageWithin(this.refreshWorkspaceSessionSnapshot(), deadline);
      if (!owns()) return;
    }
    if (
      available.workspaceFiles &&
      this.#workspaceFilesSupported &&
      this.#coreProjection.current.payload.state !== 'recovery_required'
    ) {
      await this.#subscribeRelayed(
        transport,
        'workspace_files',
        this.#workspaceFilesProjection,
        deadline,
      );
      if (!owns()) return;
      await this.#projectionStageWithin(this.refreshWorkspaceFilesSnapshot(), deadline);
      if (!owns()) return;
    }
    if (
      available.models &&
      this.#modelSupported &&
      this.#coreProjection.current.payload.state !== 'recovery_required'
    ) {
      await this.#subscribeModelGlobals(transport, owns, deadline);
      if (!owns()) return;
      await this.#subscribeScoped(
        transport,
        this.#modelScopedSubscriptions.values(),
        owns,
        deadline,
      );
      if (!owns()) return;
    }
    if (
      available.tools &&
      this.#toolSupported &&
      this.#coreProjection.current.payload.state !== 'recovery_required'
    ) {
      await this.#subscribeScoped(
        transport,
        this.#toolScopedSubscriptions.values(),
        owns,
        deadline,
      );
    }
  }

  async #createTransport(
    pipeNonce: string,
    expectedServerPid: number,
    generation: number,
    channelId: IpcChannelId,
    connectionEpoch: number,
    expectedPeer?: {
      serverCreationTime: string;
      serverWindowsSessionId: number;
    },
    connectBudgetMilliseconds?: number,
  ): Promise<{ connectionGeneration: number; transport: CoreTransport }> {
    const connectionGeneration = ++this.#connectionGeneration;
    const connectOptions: Parameters<typeof CoreTransport.connect>[0] = {
      addonPath: this.#options.release.mainPipeBridgePath,
      channelId,
      connectionEpoch: BigInt(connectionEpoch),
      expectedServerPid,
      namespaceObjectHash: this.#options.release.namespaceObjectHash,
      pipeNonce,
      runtimeInstanceId: this.#coreRuntimeInstanceId,
    };
    // The native connect runs on its own thread with a fixed internal wait, so it
    // cannot be cancelled once it has started: it is bounded, not interruptible. The
    // budget handed here is what is left of the whole recovery, so one attempt can
    // never outlive the recovery it belongs to.
    if (connectBudgetMilliseconds !== undefined) {
      connectOptions.connectTimeoutMilliseconds = Math.max(
        0,
        Math.round(connectBudgetMilliseconds),
      );
    }
    if (expectedPeer !== undefined) {
      connectOptions.expectedServerCreationTime = expectedPeer.serverCreationTime;
      connectOptions.expectedServerWindowsSessionId = expectedPeer.serverWindowsSessionId;
    }
    // Only when the acceptance layer has really attached: with no observer the transport keeps its
    // sink undefined and the emission points cost one comparison and allocate nothing.
    if (this.#lifecycleObservers.size > 0) {
      connectOptions.onLifecycle = this.#lifecycleFanout;
    }
    const transport = await CoreTransport.connect(connectOptions, {
      onBinaryChunk: () => {
        // Stream routing is registered lazily by the MessagePort bridge.
      },
      onClosed: (error) => {
        if (
          this.#stopping ||
          generation !== this.#coreGeneration ||
          connectionGeneration !== this.#activeConnectionGeneration
        ) {
          return;
        }
        this.#activeConnectionGeneration = 0;
        void this.#handleTransportClosed(generation, error);
      },
      onNotification: (method, params) => {
        if (
          generation !== this.#coreGeneration ||
          connectionGeneration !== this.#connectionGeneration
        ) {
          return;
        }
        try {
          this.#handleNotification(method, params);
        } catch (error) {
          void this.#log('core_protocol_violation', { message: String(error) });
          transport.close();
        }
      },
    });
    return { connectionGeneration, transport };
  }

  async #authenticateTransport(
    transport: CoreTransport,
    handshakeKind: RuntimeHandshakeKind,
    channelId: IpcChannelId,
    connectionEpoch: number,
    bootstrapSecret?: Buffer,
    owns: () => boolean = () => true,
    deadline?: number,
  ): Promise<RuntimeHandshakeResponse> {
    if (
      (handshakeKind === 'initial' && bootstrapSecret === undefined) ||
      (handshakeKind === 'reconnect' && bootstrapSecret !== undefined)
    ) {
      throw new Error('runtime handshake secret policy was violated');
    }
    const request: RuntimeHandshakeRequest = {
      channel_id: channelId,
      client_nonce: randomBytes(32).toString('hex'),
      connection_epoch: connectionEpoch,
      handshake: this.#mainHandshake(),
      handshake_kind: handshakeKind,
    };
    // One absolute deadline, re-measured for each half. The second half gets whatever
    // the first half left, so two halves that each fit inside their own window can no
    // longer add up to more than the recovery had.
    const remaining = (): number | undefined =>
      deadline === undefined ? undefined : Math.max(0, deadline - performance.now());
    const beginBudget = remaining();
    if (beginBudget !== undefined && beginBudget <= 0) throw this.#recoveryExpired();
    const response = await transport.beginHandshake(request, beginBudget);
    if (!owns()) throw startupAttemptEndedError();
    const transcriptHash = this.#validateHandshake(response, request);
    const finish: RuntimeHandshakeFinish = {
      channel_id: channelId,
      connection_epoch: connectionEpoch,
      handshake_kind: handshakeKind,
      transcript_hash: transcriptHash,
    };
    if (bootstrapSecret !== undefined) {
      finish.bootstrap_proof = createHmac('sha256', bootstrapSecret)
        .update(Buffer.from(transcriptHash.hex, 'hex'))
        .digest('hex');
    }
    const finishBudget = remaining();
    if (finishBudget !== undefined && finishBudget <= 0) throw this.#recoveryExpired();
    await transport.finishHandshake(finish, finishBudget);
    if (!owns()) throw startupAttemptEndedError();
    return response;
  }

  /** The error a recovery that has spent its deadline ends on. */
  #recoveryExpired(): Error {
    return new ReconnectDeadlineExpired();
  }

  #activateTransport(
    transport: CoreTransport,
    connectionGeneration: number,
    connectionEpoch: number,
  ): void {
    const previous = this.#transport;
    this.#transport = transport;
    this.#activeConnectionGeneration = connectionGeneration;
    this.#connectionEpoch = connectionEpoch;
    this.#heartbeats.start();
    if (previous !== undefined && previous !== transport) previous.close();
  }

  async #handleTransportClosed(
    generation: number,
    error: {
      code: string;
      terminal: string;
    },
  ): Promise<void> {
    if (this.#stopping || generation !== this.#coreGeneration) return;
    this.#clearTimers();
    await this.#log('core_transport_closed', {
      code: error.code,
      recovery: 'same_process_reauthentication',
      terminal: error.terminal,
    });
    if (this.#child?.exitCode !== null || this.#child.killed) return;
    this.#coreProjection.emit(
      nextLocalProjection(
        this.#coreProjection.current,
        'restarting',
        'core.transport_reconnecting',
        'Core 传输已断开，正在验证同一进程并恢复连接…',
        true,
      ),
    );
    if (this.#reconnectPromise !== undefined) return;
    const reconnect = this.#reconnectSameProcess(generation);
    this.#reconnectPromise = reconnect;
    void reconnect
      .catch(async (reconnectError: unknown) => {
        if (this.#stopping || generation !== this.#coreGeneration) return;
        if (reconnectError instanceof ReconnectBudgetExhausted) {
          // Recovery is over. The existing controlled exit takes it from here: the
          // supervisor ends the Core it owns and the next startup attempt is a fresh
          // generation, so no user request, Shell invocation or write is re-sent and no
          // new restart loop is introduced.
          await this.#log('core_reconnect_budget_exhausted', {
            attempts: reconnectError.attempts,
            budgetMilliseconds: reconnectError.budgetMilliseconds,
            lastError:
              reconnectError.lastError === undefined ? undefined : String(reconnectError.lastError),
            recovery: 'controlled_core_restart',
          });
          this.#coreProjection.emit(
            nextLocalProjection(
              this.#coreProjection.current,
              'restarting',
              'core.reconnect_budget_exhausted',
              'Core 连接在预算内未能恢复；正在按现有受控流程结束并重启 Core。',
              false,
            ),
          );
          this.#ending = 'transport_lost';
          if (this.#child?.exitCode === null && !this.#child.killed) this.#child.kill();
          return;
        }
        await this.#log('core_reconnect_pairing_invalidated', {
          message: String(reconnectError),
          recovery: 'new_process_and_initial_bootstrap',
        });
        this.#ending = 'transport_lost';
        if (this.#child?.exitCode === null && !this.#child.killed) this.#child.kill();
      })
      .finally(() => {
        if (this.#reconnectPromise === reconnect) this.#reconnectPromise = undefined;
      });
  }

  async #reconnectSameProcess(generation: number): Promise<void> {
    const pipeNonce = this.#pipeNonce;
    const corePid = this.#corePid;
    const expectedPeer = this.#corePeerIdentity;
    const frozenHandshake = this.#coreHandshake;
    if (
      pipeNonce === undefined ||
      corePid === undefined ||
      expectedPeer === undefined ||
      frozenHandshake === undefined
    ) {
      throw new PairingInvalidated('initial authenticated peer identity is unavailable');
    }
    // A monotonic total budget for the whole recovery, measured on a clock that cannot
    // step backwards. A per-attempt timeout alone is not a bound: the loop could keep
    // starting attempts forever while the window still showed "recovering". The budget
    // is what turns "one attempt is bounded" into "recovery is bounded".
    const budget = this.#startupTimeouts();
    const reconnectStartedAt = performance.now();
    const deadline = reconnectStartedAt + budget.handshakeMs;
    let attempts = 0;
    let lastError: unknown;
    // Attempts whose connect was abandoned. Each entry resolves when that attempt is
    // certain to have been closed, so the exit path can wait a bounded moment for the
    // native connect that has not returned yet instead of reporting "recovery is over"
    // while a channel object is still open.
    const abandoned: Promise<void>[] = [];
    while (
      !this.#stopping &&
      generation === this.#coreGeneration &&
      this.#childExit === undefined &&
      this.#child?.exitCode === null &&
      !this.#child.killed
    ) {
      const remaining = deadline - performance.now();
      if (remaining <= 0) {
        await this.#log('core_reconnect_budget_exhausted', {
          attempts,
          budgetMilliseconds: budget.handshakeMs,
          lastError: lastError === undefined ? undefined : String(lastError),
          recovery: 'controlled_core_restart',
          abandonedConnects: abandoned.length,
        });
        await this.#closeAbandonedConnects(abandoned);
        throw new ReconnectBudgetExhausted(budget.handshakeMs, attempts, lastError);
      }
      const nextEpoch = this.#connectionEpoch + 1;
      if (!Number.isSafeInteger(nextEpoch)) {
        throw new PairingInvalidated('connection epoch exceeded the safe integer range');
      }
      const channelId = createAuraId('chn') as IpcChannelId;
      this.#reconnectAttempt?.abandon();
      const attempt = new StartupAttempt(generation);
      this.#reconnectAttempt = attempt;
      attempts += 1;
      let created: { connectionGeneration: number; transport: CoreTransport };
      try {
        // The whole remaining recovery is handed to the attempt, and the attempt is
        // also raced against it: the native connect cannot be cancelled once it has
        // started, so waiting on it without a race would let one attempt outlive the
        // budget the loop is checking.
        //
        // A connect that lands after the race is a live channel object. Its drain
        // closes it, and `abandoned` resolves when that has certainly happened, so the
        // exit path never reports "recovery is over" while a connection is still open.
        let settledLate = false;
        let noteLate: (() => void) | undefined;
        const lateSettled = new Promise<void>((resolve) => {
          noteLate = resolve;
        });
        const outcome = await settleWithin(
          this.#createTransport(
            pipeNonce,
            corePid,
            generation,
            channelId,
            nextEpoch,
            expectedPeer,
            remaining,
          ),
          remaining,
          undefined,
          (late) => {
            settledLate = true;
            late.transport.close();
            noteLate?.();
          },
        );
        if (outcome.kind === 'ended') {
          if (!settledLate) abandoned.push(lateSettled);
          if (performance.now() >= deadline) {
            await this.#log('core_reconnect_budget_exhausted', {
              attempts,
              budgetMilliseconds: budget.handshakeMs,
              lastError: lastError === undefined ? undefined : String(lastError),
              recovery: 'controlled_core_restart',
              abandonedConnects: abandoned.length,
            });
            await this.#closeAbandonedConnects(abandoned);
            throw new ReconnectBudgetExhausted(budget.handshakeMs, attempts, lastError);
          }
          continue;
        }
        if (outcome.kind === 'error') throw outcome.error;
        created = outcome.value;
      } catch (error) {
        if (error instanceof ReconnectBudgetExhausted) throw error;
        lastError = error;
        if (isNativeProcessIdentityFailure(error)) {
          throw new PairingInvalidated('Aura Core process object changed during reconnect');
        }
        if (!this.#reconnectStillCurrent(attempt, generation)) return;
        const pause = Math.min(100, Math.max(0, deadline - performance.now()));
        if (pause > 0) await delay(pause);
        continue;
      }
      // A connect that returns after stop, exit, a newer generation, or the recovery
      // budget is closed here and never authenticated. Checking the deadline after the
      // await is what keeps a late success from activating a connection the recovery no
      // longer owns.
      attempt.holdTransport(created.transport);
      const owns = () => this.#reconnectOwns(attempt, generation, created.connectionGeneration);
      if (!owns() || performance.now() >= deadline) {
        created.transport.close();
        if (performance.now() >= deadline) {
          await this.#log('core_reconnect_late_connect_closed', { attempts });
          throw new ReconnectBudgetExhausted(budget.handshakeMs, attempts, lastError);
        }
        return;
      }
      try {
        // The absolute deadline goes in, not a snapshot of what is left: each half of the
        // handshake re-measures against it, so two halves that each fit inside their own
        // window cannot add up to more than the recovery had.
        if (performance.now() >= deadline) {
          created.transport.close();
          throw new ReconnectBudgetExhausted(budget.handshakeMs, attempts, lastError);
        }
        const response = await this.#authenticateTransport(
          created.transport,
          'reconnect',
          channelId,
          nextEpoch,
          undefined,
          owns,
          deadline,
        );
        if (!sameRuntimeHandshake(frozenHandshake, response.handshake)) {
          throw new PairingInvalidated(
            'Aura Core RuntimeInstanceId or ReleaseDescriptor changed during reconnect',
          );
        }
        const actualPeer = created.transport.peerIdentity;
        if (
          actualPeer.serverPid !== expectedPeer.serverPid ||
          actualPeer.serverCreationTime !== expectedPeer.serverCreationTime ||
          actualPeer.serverWindowsSessionId !== expectedPeer.serverWindowsSessionId
        ) {
          throw new PairingInvalidated('Aura Core process identity changed during reconnect');
        }
        // Activation is the last place the deadline is checked, because everything before
        // it can consume time: a handshake that finished exactly at the deadline must not
        // activate a channel the recovery no longer owns.
        if (!owns() || performance.now() >= deadline) {
          created.transport.close();
          if (performance.now() >= deadline) {
            throw new ReconnectBudgetExhausted(budget.handshakeMs, attempts, lastError);
          }
          return;
        }
        if (attempt.releaseForActivation() !== true) {
          created.transport.close();
          return;
        }
        this.#activateTransport(created.transport, created.connectionGeneration, nextEpoch);
      } catch (error) {
        created.transport.close();
        if (!owns()) return;
        if (error instanceof ReconnectBudgetExhausted) throw error;
        if (error instanceof PairingInvalidated || error instanceof CompatibilityFailure) {
          throw error;
        }
        throw new PairingInvalidated(
          `authenticated reconnect outcome was ambiguous: ${String(error)}`,
        );
      }
      try {
        // The same absolute deadline bounds every pending sync stage: a subscription
        // or snapshot that has not replied holds at most the recovery's remaining
        // budget, never the per-RPC default.
        await this.#synchronizeProjections(created.transport, owns, deadline);
      } catch (error) {
        if (error instanceof ReconnectDeadlineExpired) {
          // The budget is spent: close the channel and cancel the recovery owner so a
          // late reply cannot touch the generation that comes next, then end on the
          // existing controlled exit for an exhausted budget.
          attempt.abandon();
          created.transport.close();
          throw new ReconnectBudgetExhausted(budget.handshakeMs, attempts, error);
        }
        if (!owns()) return;
        await this.#log('core_reconnect_projection_sync_failed', {
          message: String(error),
          recovery: 'new_authenticated_channel',
        });
        created.transport.close();
        const pause = Math.min(100, Math.max(0, deadline - performance.now()));
        if (pause > 0) await delay(pause);
        continue;
      }
      // The projection sync is part of the recovery, not a step after it: a sync that
      // finished past the deadline must not leave a channel activated on a recovery that
      // had already ended.
      if (!owns() || performance.now() >= deadline) {
        created.transport.close();
        if (performance.now() >= deadline) {
          throw new ReconnectBudgetExhausted(budget.handshakeMs, attempts, lastError);
        }
        return;
      }
      await this.#log('core_transport_reconnected', {
        connectionEpoch: nextEpoch,
        runtimeInstanceId: this.#coreRuntimeInstanceId,
      });
      return;
    }
  }

  #reconnectStillCurrent(attempt: StartupAttempt, generation: number): boolean {
    return (
      this.#reconnectAttempt === attempt &&
      startupStillOwns(attempt, generation, this.#stopping, this.#childExit !== undefined)
    );
  }

  /**
   * A bounded moment for abandoned connects to be closed.
   *
   * A native connect that was already running when the recovery budget ran out lands
   * afterwards and is closed by its own drain. Waiting for that is bounded and short:
   * a channel object that never arrives must not hold the controlled exit open, and
   * one that arrives is closed by the drain either way.
   */
  async #closeAbandonedConnects(abandoned: Promise<void>[]): Promise<void> {
    if (abandoned.length === 0) return;
    await Promise.race([Promise.all(abandoned), delay(LATE_RECONNECT_CLOSE_BUDGET_MS)]);
  }

  #reconnectOwns(
    attempt: StartupAttempt,
    generation: number,
    connectionGeneration: number,
  ): boolean {
    return (
      this.#reconnectStillCurrent(attempt, generation) &&
      connectionGeneration === this.#connectionGeneration
    );
  }

  #mainHandshake(): RuntimeHandshake {
    return mainRuntimeHandshake(this.#options, this.#mainRuntimeInstanceId);
  }

  /** Checks Core's handshake (core-supervisor/handshake-validation.ts) and records its features. */
  #validateHandshake(
    response: RuntimeHandshakeResponse,
    request: RuntimeHandshakeRequest,
  ): RuntimeHandshakeResponse['transcript_hash'] {
    const { transcriptHash, features } = validateCoreHandshake(response, request, {
      coreRuntimeInstanceId: this.#coreRuntimeInstanceId,
      mainRuntimeInstanceId: this.#mainRuntimeInstanceId,
      release: this.#options.release,
      frozenCore: this.#coreHandshake,
    });
    this.#workspaceSessionSupported = features.workspaceSession;
    this.#workspaceFilesSupported = features.workspaceFiles;
    this.#securitySupported = features.security;
    this.#modelSupported = features.model;
    this.#toolSupported = features.tool;
    this.#toolCredentialsSupported = features.toolCredentials;
    return transcriptHash;
  }

  #handleNotification(method: string, params: unknown): void {
    const auxiliaryHandled = auxiliaryRuntimeNotification({
      owner: this,
      method,
      params,
      runtimeId: this.#coreRuntimeInstanceId,
      securitySupported: this.#securitySupported,
      securityListeners: this.#securityListeners,
      compatibility: this.#projectionCompatibility,
    });
    if (auxiliaryHandled) return;
    if (method === 'runtime.startup_progress') {
      const progress = parseStartupProgress(params);
      if (progress.runtime_instance_id !== this.#coreRuntimeInstanceId) {
        throw new Error('startup progress runtime instance changed');
      }
      const expected = this.#lastStartupSequence === undefined ? 0 : this.#lastStartupSequence + 1;
      if (progress.sequence !== expected)
        throw new Error('startup progress sequence was not monotonic');
      this.#lastStartupSequence = progress.sequence;
      if (progress.phase === 'ready' || progress.phase === 'recovery_required') {
        if (progress.phase === 'ready') this.#exitRecovery.ready();
        if (this.#readyDeadline !== undefined) clearTimeout(this.#readyDeadline);
        this.#readyDeadline = undefined;
        // Nobody awaits this refresh - it is triggered by a notification, not a caller. An unowned
        // rejection here escapes as an unhandled rejection in Main, which is the same defect the gap
        // refreshes above had. The refresh timing is deliberately unchanged; only the failure is owned.
        void this.refreshSnapshot().catch((error: unknown) => {
          void this.#logProjectionRefreshFailure('startup_progress', error);
        });
      } else {
        this.#resetReadyDeadline(this.#readyTimeoutMilliseconds());
      }
      return;
    }
    if (method === 'runtime.heartbeat') {
      const heartbeat = parseHeartbeat(params);
      if (heartbeat.runtime_instance_id !== this.#coreRuntimeInstanceId) {
        throw new Error('heartbeat runtime instance changed');
      }
      if (
        this.#lastHeartbeatSequence !== undefined &&
        heartbeat.sequence <= this.#lastHeartbeatSequence
      ) {
        throw new Error('heartbeat sequence was duplicate or reversed');
      }
      this.#lastHeartbeatSequence = heartbeat.sequence;
      this.#heartbeats.heartbeat();
      return;
    }
    if (method === 'projection.change') {
      const change = this.#projectionCompatibility.read(
        params,
        this.#coreRuntimeInstanceId,
        undefined,
        (projection) =>
          isModelProjectionChange(projection)
            ? this.#modelSupported
            : isToolProjectionKind(projection.projection_kind)
              ? this.#toolSupported
              : true,
      );
      if (change === undefined) return;
      if (change.projection_kind === 'core_health') this.#coreProjection.applyIncrement(change);
      else if (change.projection_kind === 'workspace_session') {
        this.#workspaceSessionProjection.applyIncrement(change);
      } else if (change.projection_kind === 'workspace_files') {
        this.#workspaceFilesProjection.applyIncrement(change);
      } else if (isModelProjectionChange(change)) {
        for (const listener of this.#modelListeners) listener(structuredClone(change));
      } else if (isToolProjectionKind(change.projection_kind)) {
        for (const listener of this.#toolListeners) {
          listener(structuredClone(change as ToolProjectionChange));
        }
      } else {
        throw new Error('Core sent an unsupported Projection kind');
      }
      return;
    }
    if (!method.startsWith('stream.')) throw new Error(`unknown Core notification: ${method}`);
  }

  /**
   * Gives a failed gap refresh an owner.
   *
   * A gap refresh is ONE read that no caller awaits. Before this, a failing read rejected a
   * promise nobody held — and since one request can now time out while the channel stays open,
   * that read would have reached the process-level rejection path. Nothing about the failure
   * itself changes Core health here: the projection keeps whatever Core last published and the
   * next notification or refresh reconciles the gap. Only the transport's own typed fields are
   * logged, never a message, which can carry a pipe path or a bootstrap nonce.
   */
  async #logProjectionRefreshFailure(projection: string, error: unknown): Promise<void> {
    const envelope = readFailureEnvelope(error);
    await this.#log('core_projection_refresh_failed', {
      projection,
      requestLocal: envelope !== undefined && isRequestLocalTimeout(envelope),
    });
  }

  #resetReadyDeadline(milliseconds: number): void {
    if (this.#readyDeadline !== undefined) clearTimeout(this.#readyDeadline);
    this.#readyDeadline = setTimeout(() => {
      if (
        this.#suspended ||
        this.#coreProjection.current.payload.state === 'ready' ||
        this.#coreProjection.current.payload.state === 'recovery_required'
      ) {
        return;
      }
      void this.#log('core_ready_progress_timeout', { timeoutMilliseconds: milliseconds });
      this.#child?.kill();
    }, milliseconds);
  }

  #readyTimeoutMilliseconds(): number {
    if (process.env.AURA_E2E === '1') return 2_000;
    return this.#options.appIsPackaged ? 30_000 : 10 * 60_000;
  }

  async #handleCoreExit(
    generation: number,
    exitCode: number | null,
    signal: NodeJS.Signals | null,
  ): Promise<void> {
    if (generation !== this.#coreGeneration || this.#stopping) return;
    this.#clearTimers();
    this.#connectionGeneration += 1;
    this.#transport?.close();
    this.#transport = undefined;
    const cause = this.#ending ?? 'crash';
    this.#ending = undefined;
    const state = this.#coreProjection.current.payload.state;
    const recovery = this.#exitRecovery.afterExit(cause, state, Date.now());
    if (recovery.kind === 'stopped') return;
    const { attempt } = recovery;
    await this.#log('core_exited', { attempt, exitCode, signal, cause });
    this.#coreProjection.emit(
      nextLocalProjection(
        this.#coreProjection.current,
        recovery.state,
        recovery.statusCode,
        recovery.statusMessage,
        true,
      ),
    );
    if (recovery.kind === 'escalate') this.#options.onEscalated();
    else {
      await delay(recovery.delayMilliseconds);
      if (generation === this.#coreGeneration && !this.#stopping) await this.#spawnCore(attempt);
    }
  }

  #clearTimers(): void {
    this.#heartbeats.stop();
    if (this.#readyDeadline !== undefined) clearTimeout(this.#readyDeadline);
    this.#readyDeadline = undefined;
  }

  /** Refuses a request for an interface the connected Core did not offer in its handshake. */
  #require(supported: boolean, name: string): void {
    if (!supported) throw new Error(`Core does not support the ${name} interface`);
  }

  #log(event: string, fields: Record<string, unknown>): Promise<void> {
    const { localAppData, release } = this.#options;
    return appendMainLog(localAppData, release.namespaceObjectHash, event, fields);
  }
}
