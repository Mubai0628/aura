import type { ProjectionChange } from '@aura/contracts/generated';
import { CoreTransport } from '../core-transport.js';
import type { ReleaseContext } from '../release.js';

/** The supervisor's budgets and timings, its options and the projection shapes it serves. */
export const CORE_STDERR_TAIL_LIMIT = 2_048;

/**
 * Sanitizes a Core stderr chunk for the diagnostic log: absolute Windows paths
 * (the namespace root, the user profile) are reduced to their base name, so a
 * failure reason such as an unwritable Store can be read without persisting
 * private locations.
 */
export const sanitizeCoreStderr = (text: string): string =>
  text
    .replaceAll(/\b[A-Za-z]:\\[^\s"']*/gu, (match) => {
      const parts = match.split('\\').filter((part) => part !== '');
      return `<path>\\${parts.at(-1) ?? ''}`;
    })
    .slice(-CORE_STDERR_TAIL_LIMIT);

export const CORE_STARTUP_PROGRESS_INTERVAL_MS = 3_000;

export const PROTOCOL_VERSION = { major: 1, minor: 7 } as const;

// A cold Core opens its real Store before it publishes its pipe: the wait for the
// process to become reachable is a startup budget of its own and is never shared
// with the proof-of-secret handshake that follows on an already reachable process.
// Two minutes. Opening a large Store checks it first, which on a cold disk took the
// owner's development Core 22-35 s; at 30 s Main killed it mid-check, and the restart had to
// check again from the start.
export const STARTUP_CONNECT_TIMEOUT_MS = 120_000;

export const HANDSHAKE_TIMEOUT_MS = 10_000;

export const READINESS_TIMEOUT_MS = 120_000;

export const HEARTBEAT_MISS_TIMEOUT_MS = 6_000;

export const RESUME_GRACE_MS = 15_000;

/**
 * How long the exhausted-recovery path waits for a native connect that was already
 * running to be closed.
 *
 * Short on purpose: the connect cannot be cancelled, so this only stops "recovery is
 * over" from being reported while a channel object is still arriving. A connect that
 * never lands must not hold the controlled exit open.
 */
export const LATE_RECONNECT_CLOSE_BUDGET_MS = 2_000;

export type CoreHealthProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'core_health' }
>;

export type WorkspaceSessionProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'workspace_session' }
>;

export type WorkspaceFilesProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'workspace_files' }
>;

export interface CoreSupervisorOptions {
  additionalCoreArguments?: readonly string[];
  appIsPackaged: boolean;
  localAppData: string;
  onEscalated: () => void;
  parentRuntimeInstanceId?: string;
  release: ReleaseContext;
  safeMode: boolean;
  /**
   * Startup budgets in milliseconds. `connectMs` bounds the wait for a freshly
   * spawned Core to publish its pipe (default `STARTUP_CONNECT_TIMEOUT_MS`);
   * `handshakeMs` bounds the proof-of-secret and identity exchange on that
   * reachable process (default `HANDSHAKE_TIMEOUT_MS`). Only tests inject
   * shorter budgets so the failure paths stay fast.
   */
  startupTimeouts?: { connectMs?: number; handshakeMs?: number };
}

export interface BuildExtensionInvocationIdentityV1 {
  coreRuntimeInstanceId: string;
  mainRuntimeInstanceId: string;
}

/**
 * The identity one operation's outcome confirmation belongs to.
 *
 * `writeTransport` is the exact transport that carried the write; `coreGeneration` and
 * `coreRuntimeInstanceId` name the Core PROCESS that owns the outcome ledger the query reads. The
 * channel and the process are deliberately separate: a reconnect to the same process is the same
 * authority and may answer, a new process is not.
 */
export interface CoreConfirmationOwner {
  readonly coreGeneration: number;
  readonly coreRuntimeInstanceId: string;
  readonly writeTransport: CoreTransport;
}
