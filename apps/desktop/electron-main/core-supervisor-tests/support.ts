import type {
  CommandOutcome,
  IpcErrorEnvelope,
  IpcRequestTerminal,
} from '@aura/contracts/generated';
import { CoreSupervisor } from '../core-supervisor.js';
import { createFixture, loggedFields, type Kind } from './fixture.js';

/** Shared values and readers for the CoreSupervisor tests. */
/**
 * What the transport settles ONE timed-out request with.
 *
 * The terminal is this request's own (`cancelled`), the method's invocation id is attached, and
 * `details` carries the transport's declaration that the authoritative result is
 * `runtime.command_outcome` rather than a replay. The channel, the Core process and every other
 * in-flight request are deliberately not part of this envelope.
 */
// A retained P07 tool settings command, used wherever a sample authenticated write is needed.
export const TOOL_SWITCH = {
  enabled: false,
  expected_entity_revision: '1',
  tool_id: 'tol_01ARZ3NDEKTSV4RRFFQ69G5FAV',
} as const;

export const requestTimeout: IpcErrorEnvelope = {
  category: 'cancelled',
  code: 'ipc.request_timeout',
  correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  details: {
    command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    outcome_recovery: 'query_runtime.command_outcome_without_replay',
  },
  message: 'Core request timed out: models.settings.get_snapshot',
  retryable: false,
  terminal: 'cancelled',
};

/** The rejection Main already knows how to read: an Error carrying the typed envelope. */
export const rejectionWith = (envelope: IpcErrorEnvelope): Error & { envelope: IpcErrorEnvelope } =>
  Object.assign(new Error(envelope.message), { envelope });

/**
 * The budgets one handshake mock was handed, in call order, keeping only calls that were
 * given one.
 *
 * The initial startup handshake happens before the recovery and is given no budget, so
 * the first entry here is the first half of the *reconnect*.
 */
export const budgetsOfCalls = (mock: { mock: { calls: unknown[][] } }): number[] =>
  mock.mock.calls
    .map((call) => call[1])
    .filter((budget): budget is number => typeof budget === 'number');

export const refresh = (s: CoreSupervisor, kind: Kind) => {
  if (kind === 'core_health') return s.refreshSnapshot();
  if (kind === 'workspace_session') return s.refreshWorkspaceSessionSnapshot();
  return s.refreshWorkspaceFilesSnapshot();
};

export const current = (s: CoreSupervisor, kind: Kind) => {
  if (kind === 'core_health') return s.snapshot;
  if (kind === 'workspace_session') return s.workspaceSessionSnapshot;
  return s.workspaceFilesSnapshot;
};

export const listen = (s: CoreSupervisor, kind: Kind, callback: () => void) => {
  if (kind === 'core_health') return s.subscribe(callback);
  if (kind === 'workspace_session') return s.subscribeWorkspaceSessions(callback);
  return s.subscribeWorkspaceFiles(callback);
};

export const projectionCalls = (request: { mock: { calls: unknown[][] } }): number =>
  request.mock.calls.filter((call) => String(call[0]).startsWith('projection.')).length;

/**
 * M2: the automatic bounded outcome confirmation, driven from the NORMAL command entry point.
 *
 * Nothing here calls the confirmation module or a helper directly: every case starts at
 * `requestP03/P04/P05/P07/requestAgent`, which is what Main really serves a Renderer command with.
 * What each case records is the same set the batch asked for - the original write and invocation id,
 * the query count, the budget the confirmation declared, where the answer came from, and the state
 * the caller ends in.
 */
export const M2_SUFFIXES = 'VWXYZ';

/** A distinct, canonical id per operation, so a cross-operation answer is detectable. */
export const civ = (index: number): string =>
  `civ_01ARZ3NDEKTSV4RRFFQ69G5FA${M2_SUFFIXES[index] ?? 'V'}`;

export const cor = (index: number): string =>
  `cor_01ARZ3NDEKTSV4RRFFQ69G5FA${M2_SUFFIXES[index] ?? 'V'}`;

/** The transport's own "this write may already have run" failure for ONE operation. */
export const writeTimedOut = (
  index: number,
  terminal: IpcRequestTerminal = 'cancelled',
): IpcErrorEnvelope => ({
  category: terminal === 'cancelled' ? 'cancelled' : 'transport',
  code: 'ipc.request_timeout',
  correlation_id: cor(index),
  details: {
    command_invocation_id: civ(index),
    outcome_recovery: 'query_runtime.command_outcome_without_replay',
  },
  message: 'Core request timed out',
  retryable: false,
  terminal,
});

/** What Core's ledger stores for a confirmed command: the whole response, for the ORIGINAL request. */
export const confirmedSuccess = (index: number, value: unknown): CommandOutcome => ({
  command_invocation_id: civ(index),
  outcome: 'confirmed',
  response: { id: cor(index), jsonrpc: '2.0', result: { terminal: 'succeeded', value } },
  terminal: 'succeeded',
});

export const confirmedFailure = (index: number, envelope: IpcErrorEnvelope): CommandOutcome => ({
  command_invocation_id: civ(index),
  outcome: 'confirmed',
  response: {
    error: { code: -32_603, data: envelope, message: envelope.message },
    id: cor(index),
    jsonrpc: '2.0',
  },
  terminal: envelope.terminal,
});

export const unknownOutcome = (index: number): CommandOutcome => ({
  command_invocation_id: civ(index),
  outcome: 'unknown',
  reason: 'not_recorded_or_incomplete',
});

/** How many times the real write was handed to the transport: always exactly one. */
export const writeCalls = (f: Awaited<ReturnType<typeof createFixture>>, method: string): number =>
  f.transport.request.mock.calls.filter(([called]) => called === method).length;

export const confirmationRecords = (): Record<string, unknown>[] =>
  loggedFields('command_outcome_confirmation');

export const skipRecords = (): Record<string, unknown>[] =>
  loggedFields('command_outcome_confirmation_skipped');
