import type {
  CommandInvocationId,
  CorrelationId,
  IpcErrorEnvelope,
} from '@aura/contracts/generated';
import { SendBudgetExceededError } from '../outbound-send-owner.js';

/** JSON-RPC shapes, pending requests and settlements, and the transport's error checks. */
export interface JsonRpcRequest {
  command_invocation_id?: CommandInvocationId;
  id: CorrelationId;
  jsonrpc: '2.0';
  method: string;
  params: unknown;
}

export interface JsonRpcSuccess {
  id: CorrelationId;
  jsonrpc: '2.0';
  result: { terminal: 'succeeded'; value: unknown };
}

export interface JsonRpcFailure {
  error: { code: number; data: IpcErrorEnvelope; message: string };
  id: CorrelationId;
  jsonrpc: '2.0';
}

export interface PendingRequest {
  commandInvocationId: CommandInvocationId | undefined;
  reject: (error: CoreRpcError) => void;
  resolve: (value: unknown) => void;
  timeout: NodeJS.Timeout;
}

/** What the one settlement path hands back: the caller payload plus how the request ended. */
export interface Settlement {
  pending: PendingRequest;
  /**
   * The client can prove no byte of this request reached the writer, so the failure may be reported
   * as `not_sent` instead of "possibly executed, query the outcome".
   */
  provablyUnsent: boolean;
}

/**
 * Ownership of ONE SecretInput buffer now lives with the send owner
 * (`outbound-send-owner.ts::SecretInputOwnership`): the bytes cross the same boundary as the frame
 * that carries them, so the holder and the sender are the same owner.
 */

export class CoreRpcError extends Error {
  readonly envelope: IpcErrorEnvelope;

  constructor(envelope: IpcErrorEnvelope) {
    super(envelope.message);
    this.name = 'CoreRpcError';
    this.envelope = envelope;
  }
}

/**
 * Why one send task could not be dispatched, as the caller's envelope reports it.
 *
 * A local refusal that is not "the channel is out of room" keeps its historical `send_failed` reason;
 * a task refused because the channel is already holding its whole pending-byte bound says so, so a
 * reader can tell a bound that was really reached apart from a payload that was invalid.
 */
export const sendFailureReason = (reason: unknown): string =>
  reason instanceof SendBudgetExceededError ? 'send_budget_exceeded' : 'send_failed';

export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

export const isIpcErrorEnvelope = (value: unknown): value is IpcErrorEnvelope =>
  isRecord(value) &&
  typeof value.code === 'string' &&
  typeof value.message === 'string' &&
  typeof value.correlation_id === 'string' &&
  typeof value.terminal === 'string';
