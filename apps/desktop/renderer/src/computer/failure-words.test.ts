import { expect, test } from 'vitest';
import type { ComputerControlledV1 } from '@aura/contracts/generated';

import { giveBackFailure, stopFailure, toldModelFailure } from './failure-words';

// A stop or a give-back that did not take effect says why, what it means for the desktop, and what
// to do; a desktop given back whose message to the model was not sent says the model will not go
// on by itself.

const NOTEPAD: ComputerControlledV1 = { app_id: 'notepad', name: '记事本', foreground: true };
const PAINT: ComputerControlledV1 = {
  app_id: 'paint',
  name: '画图',
  foreground: false,
  mode: 'child_session',
};
const TIMEOUT = { code: 'ipc.request_timeout', message: 'Core request timed out' };

test('a stop that did not take effect says the model may still control the program', () => {
  expect(stopFailure(TIMEOUT, [NOTEPAD], 'notepad')).toBe(
    '没有停止（Aura Core 没有及时回复），模型可能仍在操控「记事本」；请再点一次“停止”，需要时按 Esc 接管。',
  );
  expect(
    stopFailure(
      { code: 'computer.unavailable', message: 'computer.unavailable: 电脑操控不可用' },
      [NOTEPAD],
      'notepad',
    ),
  ).toBe(
    '没有停止：电脑操控不可用。模型可能仍在操控「记事本」；请再点一次“停止”，需要时按 Esc 接管。',
  );
  // A program on the child desktop is not taken over with Esc.
  expect(stopFailure(new Error('boom'), [PAINT], 'paint')).toBe(
    '没有停止，模型可能仍在操控「画图」；请再点一次“停止”。',
  );
  expect(stopFailure(undefined, [NOTEPAD, PAINT])).toBe(
    '没有停止，模型可能仍在操控这些程序；请再点一次“全部停止”，需要时按 Esc 接管。',
  );
});

test('a give-back that did not take effect says the desktop is still the user’s', () => {
  expect(giveBackFailure(TIMEOUT, NOTEPAD)).toBe(
    '没有交还（Aura Core 没有及时回复），「记事本」仍由你操作。请再点一次“交还”。',
  );
  expect(giveBackFailure(undefined, NOTEPAD)).toBe(
    '没有交还，「记事本」仍由你操作。请再点一次“交还”。',
  );
});

test('a desktop given back whose message was not sent says to send 继续', () => {
  expect(toldModelFailure({ code: 'ipc.request_not_sent', message: 'not sent' })).toBe(
    '操作权已交还，但告诉模型继续的消息没有发出（请求没有发出）。请在输入框发送“继续”，模型才会接着做。',
  );
  expect(
    toldModelFailure({ code: 'agent.unavailable', message: 'agent.unavailable: 模型不可用' }),
  ).toBe(
    '操作权已交还，但告诉模型继续的消息没有发出：模型不可用。请在输入框发送“继续”，模型才会接着做。',
  );
  expect(
    toldModelFailure({
      ...TIMEOUT,
      details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
    }),
  ).toBe(
    '操作权已交还，但告诉模型继续的消息是否发出没有确认（Aura Core 没有及时回复）；请看对话里有没有这条消息，没有就在输入框发送“继续”。',
  );
  expect(toldModelFailure(new Error('ipc closed'), true)).toBe(
    '操作权已交还，但告诉模型继续的消息是否发出没有确认；请看对话里有没有这条消息，没有就在输入框发送“继续”。',
  );
});
