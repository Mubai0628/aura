/**
 * What a secret typed into Aura reads when it did not reach Core: nothing was sent or saved, so
 * typing it again is safe. A failure Core answered keeps its own envelope.
 */
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { OUTCOME_RECOVERY_NOT_SENT } from '../shared/ipc-request-failure.js';
import { envelopeOf, errorEnvelope } from './request-failure-envelopes.js';

export const SECRET_TEXT = {
  terminalNotSent:
    'Aura Core 正在重启或暂时连不上，这次输入没有发出，终端没有收到任何内容；等 Core 就绪后重新输入即可。',
  notSent:
    'Aura Core 正在重启或暂时连不上，这次输入没有发出，什么都没有保存；等 Core 就绪后重新输入即可。',
  notReady: 'Aura Core 尚未就绪，这次输入没有发出；请等它就绪后再试。',
  modelKeyNotSent:
    'Aura Core 正在重启或暂时连不上，密钥没有发出，也没有保存；等 Core 就绪后再保存一次。',
  modelKeyUnconfirmed:
    '密钥是否已保存没有确认（Aura 内部出错）；请刷新后看是否已保存，再决定是否重新保存。',
} as const;

/**
 * A secret the connection to Core never carried, said for its surface; none for any other
 * failure, including one refused locally for too many requests, which keeps its own words.
 */
export const secretNotSent = (
  error: unknown,
  code: string,
  message: string,
): IpcErrorEnvelope | undefined => {
  const envelope = envelopeOf(error);
  return envelope?.code === 'ipc.request_not_sent' &&
    envelope.details.outcome_recovery === OUTCOME_RECOVERY_NOT_SENT
    ? {
        ...errorEnvelope(code, message, 'failed'),
        details: { outcome_recovery: OUTCOME_RECOVERY_NOT_SENT },
      }
    : undefined;
};

/** Before Core is ready nothing is sent. */
export const secretNotReady = (): IpcErrorEnvelope =>
  errorEnvelope('core_not_ready', SECRET_TEXT.notReady, 'failed');

/** A model key that failed: not sent, Core's own failure, or an outcome nobody confirmed. */
export const modelKeyFailure = (error: unknown): IpcErrorEnvelope =>
  secretNotSent(error, 'model_credential_not_sent', SECRET_TEXT.modelKeyNotSent) ??
  envelopeOf(error) ??
  errorEnvelope('model_credential_request_failed', SECRET_TEXT.modelKeyUnconfirmed, 'failed');
