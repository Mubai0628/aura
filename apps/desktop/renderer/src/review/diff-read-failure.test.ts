import { expect, test } from 'vitest';

import { diffReadFailure } from './review-problem';

// A file's diff that could not be read says why when known and which button reads it again; one
// whose newer diff could not be read says the one shown is the last one read.

test("a file's diff that could not be read says why and how to read it again", () => {
  expect(
    diffReadFailure({ code: 'ipc.request_timeout', message: 'Core request timed out' }, false),
  ).toBe('这个文件的差异没读出来（Aura Core 没有及时回复）；点“重试读取”再读一次。');
  expect(diffReadFailure({ code: 'git.read_failed', message: 'git exited 128' }, false)).toBe(
    '这个文件的差异没读出来（Git 读取失败或服务没有回应）；点“重试读取”再读一次。',
  );
  expect(diffReadFailure({ code: 'x', message: 'y' }, true)).toBe(
    '最新的差异没读出来（Git 读取失败或服务没有回应），显示的是上次读到的内容；点“刷新”可以再读一次。',
  );
});
