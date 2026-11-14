import { expect, test } from 'vitest';

import { packageFailure, packageNotSent } from './package-words';

// A tool-package action that did not happen names the action, why, and what stays as it was; one
// Core did not answer says it may have happened and to check the refreshed list.

const TIMEOUT = { code: 'ipc.request_timeout', message: 'Core request timed out' };
const NOT_READY = {
  code: 'ipc.core_not_ready',
  message: 'Aura Core 尚未就绪，这次操作没有执行；请等它就绪后重试。',
};

test('each package action names itself and what stays as it was', () => {
  expect(packageFailure('packages.import_directory', {}, NOT_READY)).toBe(
    '工具包没有导入（Aura Core 还没有就绪），列表不变；请重试。',
  );
  expect(
    packageFailure('packages.install', {} as never, {
      code: 'x',
      message: 'x.y: 这个版本还没有通过验证。',
    }),
  ).toBe('工具包没有安装：这个版本还没有通过验证。');
  expect(
    packageFailure('packages.set_enabled', { enabled: false } as never, {
      code: 'x',
      message: 'y',
    }),
  ).toBe('工具包没有停用，它仍在启用中；请重试。');
  expect(
    packageFailure('packages.set_enabled', { enabled: true } as never, { code: 'x', message: 'y' }),
  ).toBe('工具包没有启用，它仍是停用的；请重试。');
  expect(packageFailure('packages.uninstall', {} as never, { code: 'x', message: 'y' })).toBe(
    '工具包没有卸载，它仍已安装；请重试。',
  );
  expect(
    packageFailure('packages.credential.clear', {} as never, { code: 'x', message: 'y' }),
  ).toBe('凭据绑定没有解除，工具仍使用这个凭据；请重试。');
});

test('an action Core did not answer may have happened; a read says it was not read', () => {
  expect(packageFailure('packages.uninstall', {} as never, TIMEOUT)).toBe(
    '没有等到 Aura Core 的答复（Aura Core 没有及时回复），可能已经卸载；看刷新后的列表核对，再决定是否重做。',
  );
  expect(packageFailure('packages.list', {}, TIMEOUT)).toBe(
    '工具包列表没有读到（Aura Core 没有及时回复），请重试。',
  );
  expect(packageNotSent('packages.revoke_trust', {} as never)).toBe(
    '工具包的信任没有撤销（Aura 内部通信失败），它仍可运行；请重试。',
  );
});
