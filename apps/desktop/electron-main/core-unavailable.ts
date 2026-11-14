import { createAuraId } from './ids.js';
import { notSentEnvelope } from './request-failure-envelopes.js';

/**
 * Main's own answers when Core cannot take a request. They reach the user's notifications, so
 * they are written for the user.
 */
export const CORE_NOT_READY_MESSAGE = 'Aura Core 尚未就绪，这次操作没有执行；请等它就绪后重试。';
/** No authenticated channel: the request was refused before anything was sent. */
export const CORE_UNREACHABLE_MESSAGE =
  'Aura Core 暂时连不上（可能正在重启），这次操作没有发出；请稍后重试。';
/** The failure for a request refused because no authenticated channel exists: nothing was sent. */
export const coreUnreachable = (): Error =>
  Object.assign(new Error(CORE_UNREACHABLE_MESSAGE), {
    envelope: notSentEnvelope(
      'request_not_sent',
      CORE_UNREACHABLE_MESSAGE,
      'core_unreachable',
      createAuraId('cor'),
    ),
  });
export const READINESS_UNAVAILABLE_MESSAGE = 'Aura Core 的就绪状态暂时读不到，请稍后重试。';
