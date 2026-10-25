// OS and pipe substitutes shared by supervisor behavior tests; product RPC/handshake code stays real.
import { afterEach, expect, vi } from 'vitest';
import { createHash } from 'node:crypto';
import { EventEmitter } from 'node:events';
import { appendFile } from 'node:fs/promises';
import { spawn, type ChildProcess } from 'node:child_process';
import type {
  CommandOutcome,
  ProjectionChange,
  RuntimeHandshakeRequest,
  RuntimeHandshakeResponse,
} from '@aura/contracts/generated';
import { CoreSupervisor } from '../core-supervisor.js';
import { CoreTransport } from '../core-transport.js';
import type { ReleaseContext } from '../release.js';
import type { SupervisorFixture } from './types.js';
export type Kind = 'core_health' | 'workspace_session' | 'workspace_files';
type Callbacks = Parameters<typeof CoreTransport.connect>[1];
export const pipeClosed: Parameters<Callbacks['onClosed']>[0] = {
  category: 'transport',
  code: 'ERR_REVIEW_DISCONNECT',
  correlation_id: 'review',
  details: {},
  message: 'named pipe closed',
  retryable: true,
  terminal: 'transport_lost',
};
const supervisors: CoreSupervisor[] = [];
afterEach(() => {
  for (const supervisor of supervisors.splice(0)) supervisor.stop();
  vi.clearAllMocks();
});
// Fixture peer uses the real supervisor handshake and public refresh methods.
// Only the OS process/pipe are substituted, so callbacks can interleave with
// pending RPC replies deterministically without private-state access.
function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object')
    return Object.fromEntries(
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([k, v]) => [k, canonical(v)]),
    );
  return value;
}

interface FixtureOptions {
  freshChildren?: boolean;
  configurationRecovery?: boolean;
  /**
   * Runs before a connect attempt resolves. Throwing (or rejecting) keeps the Core
   * unreachable for that attempt; a pending promise models the native pipe wait,
   * which the startup phase cannot cancel once it started.
   */
  connect?: (attempt: number) => void | Promise<void>;
  handshake?: 'ok' | 'hang' | 'foreign-bundle';
  startupTimeouts?: { connectMs?: number; handshakeMs?: number };
}

export async function createFixture(options: FixtureOptions = {}): Promise<SupervisorFixture> {
  const makeChild = (pid: number) =>
    Object.assign(new EventEmitter(), {
      killed: false,
      pid,
      exitCode: null as number | null,
      stdin: Object.assign(new EventEmitter(), { end: vi.fn() }),
      stdout: new EventEmitter(),
      stderr: new EventEmitter(),
      kill: vi.fn(() => true),
    });
  const child = makeChild(100);
  const children = [child];
  let activeChild = child;
  let spawns = 0;
  vi.mocked(spawn).mockImplementation(() => {
    if (options.freshChildren && spawns > 0) {
      activeChild = makeChild(100 + spawns);
      children.push(activeChild);
    }
    spawns += 1;
    return activeChild as unknown as ChildProcess;
  });
  let runtime = '';
  let callbacks: Callbacks | undefined;
  let revision = 1;
  let read: ((kind: Kind) => Promise<ProjectionChange>) | undefined;
  const snapshot = (kind: Kind, at = revision): ProjectionChange =>
    ({
      projection_kind: kind,
      projection_scope: { type: 'global' },
      runtime_instance_id: runtime,
      revision: String(at),
      payload:
        kind === 'core_health'
          ? {
              state: 'ready',
              read_only: false,
              safe_mode: false,
              restart_attempt: 0,
              status_code: 'core.ready',
              status_message: `Ready ${at}`,
            }
          : kind === 'workspace_session'
            ? {
                workspaces: [],
                sessions: [
                  {
                    session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
                    workspace_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV',
                    title: `Title ${at}`,
                    state: 'idle',
                  },
                ],
              }
            : { workspaces: [], lifecycle_operations: [] },
    }) as ProjectionChange;
  const transport = {
    get peerIdentity() {
      return { serverPid: activeChild.pid, serverCreationTime: '1', serverWindowsSessionId: 1 };
    },
    // The two additive seams the acceptance layer reaches through the supervisor. The fixture states
    // them so a case can assert the delegation instead of inferring it from the production source.
    attachLifecycleObserver: vi.fn(),
    cancel: vi.fn(async (requestId: string) => ({ request_id: requestId })),
    channelFacts: vi.fn(() => ({
      authenticated: true,
      channel_id: 'chn_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      closed: false,
      connection_epoch: 7,
      observer_attached: true,
    })),
    queryCommandOutcome: vi.fn<(commandInvocationId: string) => Promise<CommandOutcome>>(
      async (commandInvocationId: string) => ({
        command_invocation_id: commandInvocationId,
        outcome: 'unknown',
        reason: 'not_recorded_or_incomplete',
      }),
    ),
    close: vi.fn(),
    finishHandshake: vi.fn(),
    requestWithSecretInput: vi.fn(
      async (
        method: string,
        _params: unknown,
        _operation: unknown,
        secret: Buffer,
      ): Promise<unknown> => {
        // A real send clears the caller's bytes whatever the outcome; the fixture states that so a
        // case can assert the confirmation never needed them again.
        secret.fill(0);
        throw new Error(`Unexpected SecretInput RPC ${method}`);
      },
    ),
    beginHandshake: vi.fn(
      async (request: RuntimeHandshakeRequest): Promise<RuntimeHandshakeResponse> => {
        if (options.handshake === 'hang') return new Promise<RuntimeHandshakeResponse>(() => {});
        const handshake = {
          ...request.handshake,
          supported_features: [
            ...request.handshake.supported_features,
            ...(options.configurationRecovery ? ['aura.config.recovery.v1'] : []),
          ],
          process_kind: 'core' as const,
          runtime_instance_id: runtime,
          parent_runtime_instance_id: request.handshake.runtime_instance_id,
          ...(options.handshake === 'foreign-bundle'
            ? {
                release_descriptor: {
                  ...request.handshake.release_descriptor,
                  identity: {
                    ...request.handshake.release_descriptor.identity,
                    release_bundle_id: 'rel_01ARZ3NDEKTSV4RRFFQ69G5FAW',
                  },
                },
              }
            : {}),
        };
        const response = {
          handshake,
          channel_id: request.channel_id,
          client_nonce: request.client_nonce,
          connection_epoch: request.connection_epoch,
          handshake_kind: request.handshake_kind,
          negotiated_protocol_version: { major: 1, minor: 7 },
          server_nonce: 'a'.repeat(64),
        };
        const transcript = {
          channel_id: request.channel_id,
          client_handshake: request.handshake,
          client_nonce: request.client_nonce,
          connection_epoch: request.connection_epoch,
          handshake_kind: request.handshake_kind,
          negotiated_protocol_version: response.negotiated_protocol_version,
          server_handshake: handshake,
          server_nonce: response.server_nonce,
        };
        return {
          ...response,
          transcript_hash: {
            algorithm: 'sha256',
            hex: createHash('sha256')
              .update(JSON.stringify(canonical(transcript)))
              .digest('hex'),
          },
        };
      },
    ),
    request: vi.fn(
      async (
        method: string,
        params: { projection_kind: Kind },
        priority?: number,
        timeoutMilliseconds?: number,
      ): Promise<unknown> => {
        // The acceptance seam's forwarding is asserted from what the transport really received.
        if (method === 'fixture.echo') return { method, priority, timeoutMilliseconds };
        if (method === 'projection.subscribe') return { subscribed: true };
        if (method !== 'projection.snapshot') throw new Error(`Unexpected RPC ${method}`);
        return read ? read(params.projection_kind) : snapshot(params.projection_kind);
      },
    ),
  };
  let connectAttempts = 0;
  vi.mocked(CoreTransport.connect).mockImplementation(async (connectOptions, listeners) => {
    connectAttempts += 1;
    // Throwing or waiting here is how a Core that has not published its pipe yet
    // behaves; the delay is the native wait the phase has to bound.
    await options.connect?.(connectAttempts);
    runtime = connectOptions.runtimeInstanceId;
    callbacks = listeners;
    return transport as unknown as CoreTransport;
  });
  const hash = { algorithm: 'sha256', hex: 'b'.repeat(64) };
  const component = (kind: string, relativePath: string) => ({
    component_id: `${kind}-fixture`,
    content_hash: hash,
    kind,
    relative_path: relativePath,
    version: '0.1.0',
  });
  const release = {
    allowDevMismatch: true,
    coreComponent: component('core', 'aura-core.exe'),
    coreExecutable: 'fixture.exe',
    versionDirectory: '.',
    desktopComponent: component('desktop', 'aura-desktop.exe'),
    mainPipeBridgeComponent: component('main_pipe_bridge', 'aura-main-pipe.node'),
    manifest: {
      build_commit: 'fixture',
      bundle_namespace: 'aura.fixture',
      components: [component('core', 'aura-core.exe')],
      manifest_version: { major: 1, minor: 0 },
      product_version: '0.0.0-fixture',
      release_bundle_id: 'rel_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      release_channel: 'Nightly',
      target_triple: 'x86_64-pc-windows-msvc',
    },
    manifestPath: 'fixture.json',
    namespaceObjectHash: 'fixture',
    mainPipeBridgePath: 'fixture.node',
    componentManifestHash: hash,
    // The supervisor records which image each generation loaded, beside the binding
    // that chose it, so the fixture states the binding explicitly instead of leaving
    // the log to read an undefined one.
    coreBinding: {
      state: 'published',
      source: 'debug',
      refusal: null,
      mismatchedComponents: [],
      unknownComponents: [],
      installationId: 'fixture-installation',
    },
    componentRoot: null,
    releaseDescriptor: {
      bundle_namespace: 'aura.fixture',
      identity: {
        release_bundle_id: 'rel_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        component_manifest_hash: hash,
      },
    },
  } as ReleaseContext;
  const onEscalated = vi.fn();
  const supervisor = new CoreSupervisor({
    appIsPackaged: false,
    localAppData: '.',
    release,
    safeMode: false,
    onEscalated,
    ...(options.startupTimeouts === undefined ? {} : { startupTimeouts: options.startupTimeouts }),
  });
  supervisors.push(supervisor);
  return {
    supervisor,
    child,
    children,
    onEscalated,
    transport,
    snapshot,
    connectAttempts: () => connectAttempts,
    callbacks: () => {
      if (callbacks === undefined) throw new Error('transport callbacks are missing');
      return callbacks;
    },
    setRead: (handler: typeof read) => {
      read = handler;
    },
    push: (kind: Kind, at: number) => {
      revision = at;
      callbacks!.onNotification('projection.change', snapshot(kind, at));
    },
  };
}

export async function fixture(): Promise<SupervisorFixture> {
  const f = await createFixture();
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  return f;
}

/** Poll until `condition` holds, or fail after `timeoutMilliseconds`. */
export const waitUntil = async (
  condition: () => boolean,
  timeoutMilliseconds = 5_000,
): Promise<void> => {
  const deadline = Date.now() + timeoutMilliseconds;
  for (;;) {
    if (condition()) return;
    if (Date.now() >= deadline) {
      throw new Error(`condition did not hold within ${timeoutMilliseconds}ms`);
    }
    await delay(25);
  }
};

export const delay = (milliseconds: number): Promise<void> =>
  new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });

/** Startup events the supervisor appended to `logs/main.jsonl`, in order. */
export const loggedFields = (event: string): Record<string, unknown>[] =>
  vi
    .mocked(appendFile)
    .mock.calls.map(
      ([, contents]) =>
        JSON.parse(String(contents)) as { event: string; fields: Record<string, unknown> },
    )
    .filter((entry) => entry.event === event)
    .map((entry) => entry.fields);
