import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { isRecord } from '../shared/projection-validation/common.js';

export const isIpcErrorEnvelope = (value: unknown): value is IpcErrorEnvelope =>
  isRecord(value) &&
  typeof value.code === 'string' &&
  typeof value.category === 'string' &&
  typeof value.message === 'string' &&
  typeof value.correlation_id === 'string' &&
  typeof value.terminal === 'string';

export const preloadError = (code: string, message: string): IpcErrorEnvelope => ({
  category: 'protocol',
  code: `preload.${code}`,
  correlation_id: 'cor_00000000000000000000000000',
  details: {},
  message,
  retryable: false,
  terminal: 'protocol_violation',
});
