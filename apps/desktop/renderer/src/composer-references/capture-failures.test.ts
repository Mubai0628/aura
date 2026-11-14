import { expect, test } from 'vitest';
import { notHandedOver, notReadPage, referenceUnreadable } from './capture-failures';

// A reference that could not be handed to the model or read as the message goes says what failed,
// why when known, that the message was not sent and is still in the box, and what to do.

test('a terminal, page or Chrome tab that was not handed over keeps the message and says why', () => {
  expect(notHandedOver('终端 1')).toBe(
    '「终端 1」没能交给模型（它可能已关闭，或服务没有回应），消息没有发出，内容仍在输入框里。请移除这个引用或稍后再发。',
  );
  expect(
    notHandedOver('终端 1', { code: 'ipc.request_timeout', message: 'Core request timed out' }),
  ).toBe(
    '「终端 1」没能交给模型（Aura Core 没有及时回复），消息没有发出，内容仍在输入框里。请移除这个引用或稍后再发。',
  );
  expect(
    notHandedOver('终端 1', { code: 'terminal.ended', message: '终端已结束，请新建终端。' }),
  ).toBe(
    '「终端 1」没能交给模型：终端已结束，请新建终端。消息没有发出，内容仍在输入框里。请移除这个引用或稍后再发。',
  );
  expect(notHandedOver('Example', '这个页面已关闭')).toBe(
    '「Example」没能交给模型：这个页面已关闭。消息没有发出，内容仍在输入框里。请移除这个引用或稍后再发。',
  );
  // English text from below is never shown.
  expect(notHandedOver('Example', { code: 'x', message: 'tab gone' })).toBe(
    '「Example」没能交给模型（它可能已关闭，或服务没有回应），消息没有发出，内容仍在输入框里。请移除这个引用或稍后再发。',
  );
});

test('a page whose text cannot be read keeps the message and says to wait for it to load', () => {
  expect(notReadPage('Example')).toBe(
    '读不到「Example」的内容（页面可能还在加载或已关闭），消息没有发出，内容仍在输入框里。等页面加载完再发，或移除这个引用。',
  );
  expect(notReadPage('Example', '这个页面已关闭')).toBe(
    '读不到「Example」的内容：这个页面已关闭。消息没有发出，内容仍在输入框里；请移除这个引用或稍后再发。',
  );
});

test('a reference that failed some other way still says the message was not sent', () => {
  expect(referenceUnreadable(new Error('boom'))).toBe(
    '有一个引用的内容读不出来（服务没有回应或拒绝了读取），消息没有发出，内容仍在输入框里。请移除它或稍后再发。',
  );
  expect(referenceUnreadable(new Error('Transport is closed'))).toBe(
    '有一个引用的内容读不出来（与 Aura Core 的连接断开了），消息没有发出，内容仍在输入框里。请移除它或稍后再发。',
  );
});
