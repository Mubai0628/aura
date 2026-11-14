import { expect, test } from 'vitest';

import {
  closeFailure,
  inputFailure,
  listFailure,
  OUTPUT_OUT_OF_STEP,
  processListFailure,
  processStopFailure,
  resizeFailure,
  SENSITIVE_UNCONFIRMED,
  sensitiveFailure,
  subscribeFailure,
  TERMINAL_GONE,
} from './failure-words';

// A terminal or a process that failed says why when known, what it means for the terminal or the
// program running in it, and what to do; nothing asks to retry what the user never started.

const TIMEOUT = { code: 'ipc.request_timeout', message: 'Core request timed out' };

test('typed input that was lost says the terminal may be missing it; unconfirmed input says so', () => {
  expect(inputFailure(TIMEOUT)).toBe(
    '这次输入没有送到终端（Aura Core 没有及时回复），终端里可能缺了这段输入。请看终端当前内容再决定是否重新输入。',
  );
  expect(
    inputFailure({
      ...TIMEOUT,
      details: { outcome_recovery: 'terminal_input_unconfirmed_do_not_replay' },
    }),
  ).toBe(
    '这次输入是否送到终端没有确认（Aura Core 没有及时回复）；请看终端当前内容再决定是否重新输入。',
  );
});

test('a size the terminal did not take, and output it could not follow, say what to do', () => {
  expect(resizeFailure({ code: 'x', message: 'resize failed' })).toBe(
    '终端没能跟着面板调整大小，显示可能错位。再拖动一次面板宽度会重新调整。',
  );
  expect(subscribeFailure(TIMEOUT)).toBe(
    '没有接上这个终端的输出（Aura Core 没有及时回复），屏幕可能不再更新。请关闭这个页签后重新打开终端。',
  );
  expect(OUTPUT_OUT_OF_STEP).toBe(
    '终端输出没有对上，屏幕已停止更新。请关闭这个页签后重新打开终端。',
  );
  expect(TERMINAL_GONE).toBe(
    '这个终端已经结束（例如 Aura Core 重启过），无法再连上。请关闭这个页签，需要时新开一个终端。',
  );
});

test('a list or a close that failed says what is still there', () => {
  expect(listFailure(TIMEOUT)).toBe(
    '终端列表读不出来（Aura Core 没有及时回复），之前打开的终端暂时没有列出。切换到别的会话再切回来会重新读取。',
  );
  expect(closeFailure({ code: 'x', message: 'close failed' })).toBe(
    '终端没有关闭，它仍在运行，页签已保留；请再关一次。',
  );
});

test('secure input whose request has ended says nothing was typed; one not confirmed says to look first', () => {
  const ended =
    '这次安全输入请求已经结束（超时或已经用过），没有输入任何内容。请查看终端，需要时让模型再请求一次。';
  expect(
    sensitiveFailure({
      code: 'terminal.sensitive.expired_or_consumed',
      message: 'terminal.sensitive.expired_or_consumed',
    }),
  ).toBe(ended);
  expect(
    sensitiveFailure({ code: 'agent.command_failed', message: 'terminal.sensitive.not_pending' }),
  ).toBe(ended);
  expect(sensitiveFailure({ code: 'terminal.sensitive.consumed', message: 'x' })).toBe(ended);
  expect(sensitiveFailure({ code: 'x', message: 'y' })).toBe(
    '安全输入没有完成，没有输入任何内容；请重新输入后再提交，或取消。',
  );
  expect(SENSITIVE_UNCONFIRMED).toBe(
    '没能确认这次输入是否送到终端。请先看终端当前状态，确认没收到再输入一次，避免重复输入。',
  );
});

test('a process list or a stop that failed says whether the program may still run', () => {
  expect(processListFailure(TIMEOUT)).toBe(
    '进程列表读不出来（Aura Core 没有及时回复），点“刷新”再试。',
  );
  expect(processStopFailure({ code: 'x', message: 'y' })).toBe(
    '停止没有完成，进程可能仍在运行；点“刷新”查看，仍在运行就再停一次。',
  );
  expect(processStopFailure(TIMEOUT)).toBe(
    '停止没有完成（Aura Core 没有及时回复），进程可能仍在运行；点“刷新”查看，仍在运行就再停一次。',
  );
});
