import { describe, expect, it } from 'vitest';

import { describeCoreStartupFailure } from '../../shared/core-startup-diagnostics';

const CORRELATION_ID = 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV';

describe('Core startup diagnostics', () => {
  it('classifies a Named Pipe connection failure without retaining its sensitive message', () => {
    const bootstrapNonce = 'A'.repeat(64);
    const pipeName = String.raw`\\.\pipe\Aura\run_example\private-pipe`;
    const diagnostic = describeCoreStartupFailure('transport_connect', {
      code: 'ENOENT',
      message: `connect ENOENT ${pipeName} nonce=${bootstrapNonce}`,
    });

    expect(diagnostic).toMatchObject({
      failureCode: 'ENOENT',
      failureKind: 'transport_unavailable',
      retryable: true,
      stage: 'transport_connect',
      statusCode: 'core.transport_connect_failed',
    });
    expect(JSON.stringify(diagnostic)).not.toContain(pipeName);
    expect(JSON.stringify(diagnostic)).not.toContain(bootstrapNonce);
  });

  it('keeps only typed IPC metadata and drops peer messages and details', () => {
    const bootstrapNonce = 'B'.repeat(64);
    const diagnostic = describeCoreStartupFailure('runtime_handshake', {
      envelope: {
        category: 'transport',
        code: 'ipc.request_timeout',
        correlation_id: CORRELATION_ID,
        details: { bootstrap_nonce: bootstrapNonce },
        message: `handshake timed out for ${bootstrapNonce}`,
        retryable: true,
        terminal: 'transport_lost',
      },
    });

    expect(diagnostic).toEqual({
      category: 'transport',
      correlationId: CORRELATION_ID,
      failureCode: 'ipc.request_timeout',
      failureKind: 'request_timeout',
      retryable: true,
      stage: 'runtime_handshake',
      statusCode: 'core.handshake_failed',
      statusMessage: 'Aura Core 握手失败，正在安全重启…',
      terminal: 'transport_lost',
    });
    expect(JSON.stringify(diagnostic)).not.toContain(bootstrapNonce);
  });

  it('rejects untrusted error codes instead of persisting arbitrary text', () => {
    const bootstrapNonce = 'C'.repeat(64);
    const diagnostic = describeCoreStartupFailure('initial_projection_sync', {
      envelope: {
        category: 'internal',
        code: bootstrapNonce,
        correlation_id: 'not-a-correlation-id',
        details: {},
        message: bootstrapNonce,
        retryable: false,
        terminal: 'failed',
      },
    });

    expect(diagnostic).toMatchObject({
      category: 'internal',
      failureCode: 'ipc.unknown',
      failureKind: 'remote_rejected',
      retryable: false,
      stage: 'initial_projection_sync',
      statusCode: 'core.initial_projection_sync_failed',
      terminal: 'failed',
    });
    expect(diagnostic).not.toHaveProperty('correlationId');
    expect(JSON.stringify(diagnostic)).not.toContain(bootstrapNonce);
  });

  it('distinguishes access denial from an unknown local failure', () => {
    expect(describeCoreStartupFailure('transport_connect', { code: 'EACCES' })).toMatchObject({
      failureCode: 'EACCES',
      failureKind: 'transport_denied',
    });
    expect(
      describeCoreStartupFailure('transport_connect', {
        code: 'secret-or-peer-controlled-value',
      }),
    ).toMatchObject({
      failureCode: 'system.unknown',
      failureKind: 'unexpected',
    });
  });

  it('separates an expired startup budget from an early Core exit', () => {
    expect(
      describeCoreStartupFailure('transport_connect', { code: 'ERR_AURA_CORE_START_TIMEOUT' }),
    ).toMatchObject({
      failureCode: 'ERR_AURA_CORE_START_TIMEOUT',
      failureKind: 'request_timeout',
      retryable: true,
      stage: 'transport_connect',
      statusCode: 'core.transport_connect_failed',
    });
    expect(
      describeCoreStartupFailure('transport_connect', { code: 'ERR_AURA_CORE_EXITED_EARLY' }),
    ).toMatchObject({
      failureCode: 'ERR_AURA_CORE_EXITED_EARLY',
      failureKind: 'transport_unavailable',
      retryable: true,
      stage: 'transport_connect',
    });
    // The handshake keeps its own budget, reported as a handshake failure.
    expect(describeCoreStartupFailure('runtime_handshake', { code: 'ETIMEDOUT' })).toMatchObject({
      failureCode: 'ETIMEDOUT',
      failureKind: 'request_timeout',
      stage: 'runtime_handshake',
      statusCode: 'core.handshake_failed',
    });
  });
});
