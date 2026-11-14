import { connectionCause } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/**
 * What the composer says when a reference could not be handed to the model or read as the message
 * goes: what failed, why when known, that the message was not sent and is still in the box, and
 * what to do.
 */

const KEPT = '消息没有发出，内容仍在输入框里';

/** The sentence ended as one, to go before the next. */
const ended = (sentence: string): string =>
  /[。！？.!?]$/u.test(sentence) ? sentence : `${sentence}。`;

/** Core's or Main's own sentence for the failure, when it has one and the connection is not why. */
const saidFor = (error: unknown): string =>
  error === undefined || connectionCause(error) !== undefined ? '' : userMessage(error, '');

/** A terminal, page or Chrome tab that could not be handed to the model. */
export function notHandedOver(title: string, error?: unknown): string {
  const said = saidFor(error);
  if (said) return `「${title}」没能交给模型：${ended(said)}${KEPT}。请移除这个引用或稍后再发。`;
  const cause = error === undefined ? undefined : connectionCause(error);
  return `「${title}」没能交给模型（${cause ?? '它可能已关闭，或服务没有回应'}），${KEPT}。请移除这个引用或稍后再发。`;
}

/** A page of the built-in browser whose text could not be read. */
export function notReadPage(title: string, said?: string): string {
  const sentence = said === undefined ? '' : saidFor(said);
  return sentence
    ? `读不到「${title}」的内容：${ended(sentence)}${KEPT}；请移除这个引用或稍后再发。`
    : `读不到「${title}」的内容（页面可能还在加载或已关闭），${KEPT}。等页面加载完再发，或移除这个引用。`;
}

/** A reference that failed in a way none of the above names. */
export const referenceUnreadable = (error: unknown): string =>
  `有一个引用的内容读不出来（${connectionCause(error) ?? '服务没有回应或拒绝了读取'}），${KEPT}。请移除它或稍后再发。`;
