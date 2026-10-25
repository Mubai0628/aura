// Typed send-stage recovery, independent of connection lifecycle ownership.
import type {
  CommandInvocationId,
  IpcErrorEnvelope,
  IpcRequestTerminal,
} from '@aura/contracts/generated';
import { createAuraId } from './ids.js';
import { coreRoute } from '../shared/generated/routes.js';
import {
  OUTCOME_RECOVERY_NOT_SENT,
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  OUTCOME_RECOVERY_TERMINAL_INPUT_NO_REPLAY,
} from '../shared/ipc-request-failure.js';

export const isTransientInput = (method: string | undefined): boolean =>
  typeof method === 'string' && coreRoute(method)?.idempotency === 'transient_input';

export const errorEnvelope = (
  code: string,
  message: string,
  terminal: IpcRequestTerminal,
  correlationId = createAuraId('cor'),
): IpcErrorEnvelope => ({
  category:
    terminal === 'protocol_violation'
      ? 'protocol'
      : terminal === 'cancelled'
        ? 'cancelled'
        : terminal === 'failed'
          ? 'internal'
          : 'transport',
  code: `ipc.${code}`,
  correlation_id: correlationId,
  details: {},
  message,
  retryable: terminal === 'transport_lost' || terminal === 'core_restarted',
  terminal,
});

export const notSentEnvelope = (
  code: string,
  message: string,
  reason: string,
  correlationId: string,
): IpcErrorEnvelope => ({
  ...errorEnvelope(code, message, 'failed', correlationId),
  details: { not_sent_reason: reason, outcome_recovery: OUTCOME_RECOVERY_NOT_SENT },
});

export function closedTransportEnvelope(method: string): IpcErrorEnvelope {
  const envelope = errorEnvelope('transport_lost', 'Transport is closed', 'transport_lost');
  return isTransientInput(method)
    ? requestRecoveryEnvelope(method, undefined, envelope, OUTCOME_RECOVERY_NOT_SENT)
    : envelope;
}

export function requestRecoveryEnvelope(
  method: string | undefined,
  commandInvocationId: CommandInvocationId | undefined,
  envelope: IpcErrorEnvelope,
  recovery: string = OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
): IpcErrorEnvelope {
  if (isTransientInput(method)) {
    const details = { ...envelope.details };
    delete details.command_invocation_id;
    return {
      ...envelope,
      retryable: false,
      message:
        recovery === OUTCOME_RECOVERY_NOT_SENT
          ? envelope.message
          : '终端输入结果未确认，请查看终端状态；不要自动重发输入。',
      details: {
        ...details,
        outcome_recovery:
          recovery === OUTCOME_RECOVERY_NOT_SENT
            ? OUTCOME_RECOVERY_NOT_SENT
            : OUTCOME_RECOVERY_TERMINAL_INPUT_NO_REPLAY,
      },
    };
  }
  return commandInvocationId === undefined
    ? envelope
    : {
        ...envelope,
        details: {
          ...envelope.details,
          command_invocation_id: commandInvocationId,
          outcome_recovery: recovery,
        },
      };
}
