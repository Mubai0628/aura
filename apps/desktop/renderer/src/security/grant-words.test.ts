import { expect, test } from 'vitest';

import {
  CAPABILITY_TEXT,
  EXTERNAL_CAPABILITY_TEXT,
  GRANT_STATE_CHANGED,
  grantsNotRead,
  nativeRevokeFailure,
  revokeFailure,
} from './grant-words';

// The grant lists name what a grant allows in words, and a list or a withdrawal that did not
// happen says why and what is in force.

const TIMEOUT = { code: 'ipc.request_timeout', message: 'Core request timed out' };

test('what a grant allows is said in words', () => {
  expect(
    ['read', 'write', 'model_egress'].map((each) => EXTERNAL_CAPABILITY_TEXT[each as never]),
  ).toEqual(['读取', '写入', '发给模型']);
  expect(CAPABILITY_TEXT.workspace_file_write).toBe('修改工作区文件');
  expect(CAPABILITY_TEXT.desktop_input).toBe('操作鼠标键盘');
});

test('a list that was not read says why and to try again', () => {
  expect(grantsNotRead('目录授权', TIMEOUT)).toBe(
    '目录授权没有读到（Aura Core 没有及时回复），请点“重试”。',
  );
  expect(grantsNotRead('Shell 运行批准', { message: 'unavailable' })).toBe(
    'Shell 运行批准没有读到，请点“重试”。',
  );
});

test('a withdrawal that did not happen says why and what is in force', () => {
  expect(
    revokeFailure({ code: 'file.grant_state_changed', message: 'x' }, '文件批准', '它仍然有效'),
  ).toBe(GRANT_STATE_CHANGED);
  expect(GRANT_STATE_CHANGED).toBe(
    '这项批准的状态刚变化（可能已撤销或到期），列表已重新读取，请按最新状态处理。',
  );
  expect(revokeFailure({ code: 'x', message: 'y' }, '目录授权', '这个目录仍可访问')).toBe(
    '目录授权没有撤销，这个目录仍可访问；列表已重新读取，请核对后再撤销。',
  );
  expect(revokeFailure(TIMEOUT, '文件批准', '它仍然有效')).toBe(
    '撤销没有确认完成（Aura Core 没有及时回复），这项文件批准可能仍然有效；列表已重新读取，请核对。',
  );
  expect(
    revokeFailure(
      { code: 'permission.other', message: '这项批准不属于当前会话。' },
      '联网批准',
      '它仍然有效',
    ),
  ).toBe('联网批准没有撤销：这项批准不属于当前会话。');
  expect(revokeFailure(new Error('bridge'), '记忆批准', '它仍然有效')).toBe(
    '撤销没有确认完成（Aura 内部通信失败），这项记忆批准可能仍然有效；列表已重新读取，请核对。',
  );
  expect(nativeRevokeFailure(TIMEOUT)).toBe(
    '撤销没有确认完成（Aura Core 没有及时回复），这项 Shell 运行批准可能仍然有效。可以再点一次撤销，不会重复撤销。',
  );
  expect(nativeRevokeFailure({ code: 'permission.grant_state_changed', message: 'x' })).toBe(
    GRANT_STATE_CHANGED,
  );
});
