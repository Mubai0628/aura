import type {
  ErrorCategory,
  IpcErrorEnvelope,
  IpcRequestTerminal,
} from '@aura/contracts/generated';

export type CoreStartupFailureStage =
  'transport_connect' | 'runtime_handshake' | 'initial_projection_sync';

export type CoreStartupFailureKind =
  | 'transport_unavailable'
  | 'transport_denied'
  | 'request_timeout'
  | 'protocol_violation'
  | 'remote_rejected'
  | 'unexpected';

export interface CoreStartupFailureDiagnostic {
  category?: ErrorCategory;
  correlationId?: string;
  failureCode: string;
  failureKind: CoreStartupFailureKind;
  retryable: boolean;
  stage: CoreStartupFailureStage;
  statusCode: string;
  statusMessage: string;
  terminal?: IpcRequestTerminal;
}

const ERROR_CATEGORIES = new Set<ErrorCategory>([
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

const REQUEST_TERMINALS = new Set<IpcRequestTerminal>([
  'succeeded',
  'failed',
  'cancelled',
  'transport_lost',
  'protocol_violation',
  'core_restarted',
]);

const STAGE_STATUS: Record<CoreStartupFailureStage, { statusCode: string; statusMessage: string }> =
  {
    transport_connect: {
      statusCode: 'core.transport_connect_failed',
      statusMessage: '无法连接 Aura Core，正在安全重启…',
    },
    runtime_handshake: {
      statusCode: 'core.handshake_failed',
      statusMessage: 'Aura Core 握手失败，正在安全重启…',
    },
    initial_projection_sync: {
      statusCode: 'core.initial_projection_sync_failed',
      statusMessage: 'Aura Core 初始化同步失败，正在安全重启…',
    },
  };

/**
 * Reduces an arbitrary connection or RPC failure to fields that are safe to
 * persist. In particular, raw messages and envelope details are never copied:
 * either can contain a Named Pipe path, bootstrap nonce, or peer-supplied text.
 */
export const describeCoreStartupFailure = (
  stage: CoreStartupFailureStage,
  error: unknown,
): CoreStartupFailureDiagnostic => {
  const envelope = readIpcEnvelope(error);
  const status = STAGE_STATUS[stage];
  if (envelope !== undefined) {
    const failureCode = safeIpcCode(envelope.code);
    const diagnostic: CoreStartupFailureDiagnostic = {
      failureCode,
      failureKind: classifyEnvelope(envelope),
      retryable: envelope.retryable,
      stage,
      statusCode: status.statusCode,
      statusMessage: status.statusMessage,
    };
    if (ERROR_CATEGORIES.has(envelope.category)) diagnostic.category = envelope.category;
    if (REQUEST_TERMINALS.has(envelope.terminal)) diagnostic.terminal = envelope.terminal;
    if (isCorrelationId(envelope.correlation_id)) {
      diagnostic.correlationId = envelope.correlation_id;
    }
    return diagnostic;
  }

  const systemCode = readSystemErrorCode(error);
  return {
    failureCode: systemCode,
    failureKind: classifySystemCode(systemCode),
    retryable: stage !== 'initial_projection_sync' || systemCode === 'ETIMEDOUT',
    stage,
    statusCode: status.statusCode,
    statusMessage: status.statusMessage,
  };
};

const readIpcEnvelope = (error: unknown): IpcErrorEnvelope | undefined => {
  if (!isRecord(error) || !isRecord(error.envelope)) return undefined;
  const envelope = error.envelope;
  if (
    typeof envelope.code !== 'string' ||
    typeof envelope.category !== 'string' ||
    typeof envelope.correlation_id !== 'string' ||
    typeof envelope.retryable !== 'boolean' ||
    typeof envelope.terminal !== 'string'
  ) {
    return undefined;
  }
  return envelope as unknown as IpcErrorEnvelope;
};

const safeIpcCode = (code: string): string =>
  /^ipc\.[a-z0-9][a-z0-9_.-]{0,63}$/.test(code) ? code : 'ipc.unknown';

const readSystemErrorCode = (error: unknown): string => {
  if (!isRecord(error) || typeof error.code !== 'string') return 'system.unknown';
  return /^E[A-Z0-9_]{1,31}$/.test(error.code) ? error.code : 'system.unknown';
};

const classifyEnvelope = (envelope: IpcErrorEnvelope): CoreStartupFailureKind => {
  if (envelope.terminal === 'protocol_violation' || envelope.category === 'protocol') {
    return 'protocol_violation';
  }
  if (envelope.code === 'ipc.request_timeout') return 'request_timeout';
  if (
    envelope.terminal === 'transport_lost' ||
    envelope.terminal === 'core_restarted' ||
    envelope.category === 'transport'
  ) {
    return 'transport_unavailable';
  }
  return 'remote_rejected';
};

const classifySystemCode = (code: string): CoreStartupFailureKind => {
  // Aura startup budgets report their own codes: a budget that expired while the
  // Core was still starting is a timeout, an early exit is an unavailable Core.
  if (code === 'ERR_AURA_CORE_START_TIMEOUT') return 'request_timeout';
  if (code === 'ERR_AURA_CORE_EXITED_EARLY') return 'transport_unavailable';
  if (code === 'EACCES' || code === 'EPERM') return 'transport_denied';
  if (code === 'ETIMEDOUT') return 'request_timeout';
  if (code === 'ENOENT' || code === 'ECONNREFUSED' || code === 'ECONNRESET' || code === 'EPIPE') {
    return 'transport_unavailable';
  }
  return 'unexpected';
};

const isCorrelationId = (value: string): boolean => /^cor_[0-9A-Z]{26}$/.test(value);

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
