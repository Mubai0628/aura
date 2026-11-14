import { expect, test } from 'vitest';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../../shared/ipc-request-failure';
import { NotSent, preparationFailure, sessionCreateFailure } from './first-send-failure';

// A new conversation's first message needs the conversation, its model and its permission mode
// first. A step that failed says what did not happen, why, that the message was not sent and the
// draft is kept, and what to do next.

const timeout = { code: REQUEST_TIMEOUT_CODE, message: 'Core request timed out' };

test('a model not chosen says why, that nothing was sent and what to do', () => {
  expect(preparationFailure(timeout, '模型没有选上')).toBe(
    '模型没有选上（Aura Core 没有及时回复），消息没有发出，草稿已保留；请再点发送。',
  );
  // The choice is read again before the next send, so an answer that did not come needs no review.
  expect(
    preparationFailure(
      { ...timeout, details: { outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY } },
      '模型没有选上',
    ),
  ).toBe('模型没有选上（Aura Core 没有及时回复），消息没有发出，草稿已保留；请再点发送。');
  expect(
    preparationFailure(
      {
        code: 'models.configuration.revision_conflict',
        message: '模型设置已在别处更新，请刷新后再试。',
      },
      '模型没有选上',
    ),
  ).toBe('模型没有选上（这个会话的设置刚刚在别处改过），消息没有发出，草稿已保留；请再点发送。');
  // Core's own sentence is the reason, with its own next step.
  expect(
    preparationFailure(
      {
        code: 'models.binding.profile_disabled',
        message: '这个模型已停用，请在模型设置中启用或选择其他模型。',
      },
      '模型没有选上',
    ),
  ).toBe(
    '模型没有选上：这个模型已被停用，请在模型设置中启用或选择其他模型。消息没有发出，草稿已保留。',
  );
  expect(
    preparationFailure(
      { code: 'broker.dispatch_unavailable', message: 'session model binding was not found' },
      '模型没有选上',
    ),
  ).toBe('模型没有选上，消息没有发出，草稿已保留；请再点发送。');
});

test('a conversation not made says why and what to do; one that may exist says so', () => {
  expect(
    sessionCreateFailure({
      code: 'persistence.busy',
      message: 'persistence writer session queue is at capacity; the request was not admitted',
    }),
  ).toBe('没能新建会话（Aura 的存储正忙），消息没有发出，草稿已保留；请稍后再点发送。');
  expect(
    sessionCreateFailure({
      code: 'broker.dispatch_unavailable',
      message: 'workspace was not found',
    }),
  ).toBe('没能新建会话（这个工作区已不存在），消息没有发出，草稿已保留；请换一个工作区再发送。');
  expect(sessionCreateFailure({ code: 'ipc.core_not_ready', message: 'Core is not ready' })).toBe(
    '没能新建会话（Aura Core 还没有就绪），消息没有发出，草稿已保留；请稍后再点发送。',
  );
  expect(
    sessionCreateFailure({
      ...timeout,
      details: { outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY },
    }),
  ).toBe(
    '新会话是否建好没有确认（Aura Core 没有及时答复），消息没有发出，草稿已保留；点“已核对，返回编辑”后可以再发送，侧栏里如果多出一个空会话可以删掉。',
  );
});

test('a step whose outcome is known is marked, so the send asks for no review', () => {
  const known = new NotSent('没能新建会话，消息没有发出，草稿已保留；请稍后再点发送。');
  expect(known).toBeInstanceOf(Error);
  expect(known.message).toBe('没能新建会话，消息没有发出，草稿已保留；请稍后再点发送。');
});
