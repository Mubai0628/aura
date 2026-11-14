import {
  isUnconfirmedCommandOutcome,
  type IpcFailureLike,
} from '../../../shared/ipc-request-failure';
import { connectionCause, isConflict } from './failure-text';
import { userMessage } from './user-message';

/**
 * A step before a new conversation's first message that failed with a known outcome: nothing was
 * sent and nothing needs checking, so the send asks for no review.
 */
export class NotSent extends Error {}

const NOT_SENT = '消息没有发出，草稿已保留';

const field = (value: unknown, name: 'code' | 'message'): string | undefined => {
  if (typeof value !== 'object' || value === null) return undefined;
  const found = (value as Record<string, unknown>)[name];
  return typeof found === 'string' ? found : undefined;
};

/**
 * What a step before the first message says when it failed: `what` did not happen, why, that the
 * message was not sent and the draft is kept, and `next`. Every step is checked again before the
 * next send, so an answer that did not come needs no review. A reason Aura or Core has a sentence
 * for is given as that sentence, with its own next step.
 */
export function preparationFailure(
  error: unknown,
  what: string,
  next = '请再点发送。',
  options: { cause?: string } = {},
): string {
  const cause =
    options.cause ?? (isConflict(error) ? '这个会话的设置刚刚在别处改过' : connectionCause(error));
  if (cause) return `${what}（${cause}），${NOT_SENT}；${next}`;
  const said = userMessage(error, '');
  if (said) return `${what}：${said.replace(/[。.]+$/u, '')}。${NOT_SENT}。`;
  return `${what}，${NOT_SENT}；${next}`;
}

/** What the first message says when its conversation could not be made. */
export function sessionCreateFailure(error: unknown): string {
  const code = field(error, 'code') ?? '';
  if (
    typeof error === 'object' &&
    error !== null &&
    isUnconfirmedCommandOutcome({ terminal: '', ...(error as Partial<IpcFailureLike>), code })
  )
    return `新会话是否建好没有确认（Aura Core 没有及时答复），${NOT_SENT}；点“已核对，返回编辑”后可以再发送，侧栏里如果多出一个空会话可以删掉。`;
  if (code === 'persistence.busy')
    return preparationFailure(error, '没能新建会话', '请稍后再点发送。', {
      cause: 'Aura 的存储正忙',
    });
  if ((field(error, 'message') ?? '').startsWith('workspace was not found'))
    return preparationFailure(error, '没能新建会话', '请换一个工作区再发送。', {
      cause: '这个工作区已不存在',
    });
  return preparationFailure(error, '没能新建会话', '请稍后再点发送。');
}
