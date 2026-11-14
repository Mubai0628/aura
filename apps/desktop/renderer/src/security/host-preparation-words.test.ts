import { expect, test } from 'vitest';

import { hostOperationState, hostRequestFailure } from './host-preparation-words';

// A compatibility environment request that did not start says why and that Windows was not
// changed; an operation under way or ended says where it is in words.

const refused = (code: string) =>
  ({
    category: 'unavailable',
    code,
    correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    details: {},
    message: 'English diagnostic',
    retryable: false,
    terminal: 'failed',
  }) as never;

test('a request that did not start says why and that Windows was not changed', () => {
  expect(
    hostRequestFailure(refused('sandbox.host_preparation_release_trust_failed'), 'prepare'),
  ).toBe(
    '这个 Aura 安装包没有通过签名校验，所以没有请求管理员权限，系统没有被修改。请从官方发布页重新下载安装。',
  );
  expect(hostRequestFailure(refused('sandbox.host_preparation_plan_expired'), 'repair')).toBe(
    '准备方案已过期，系统没有被修改；请重新点“修复兼容环境”。',
  );
  expect(hostRequestFailure(refused('sandbox.host_preparation_busy'), 'prepare')).toBe(
    '另一项兼容环境检查或操作还在进行，这次没有开始；等它结束再试。',
  );
  expect(hostRequestFailure(refused('sandbox.other'), 'remove')).toBe(
    '移除 Aura 配置没有完成，请看“兼容环境”一项的状态，核对后再决定是否重试。',
  );
  expect(
    hostRequestFailure(
      { ...(refused('ipc.request_timeout') as object), message: 'Core request timed out' } as never,
      'prepare',
    ),
  ).toBe(
    '没有等到 Aura Core 的答复（Aura Core 没有及时回复），配置兼容环境可能已经开始；请看“兼容环境”一项的状态，核对后再决定是否重试。',
  );
});

test('an operation under way or ended says where it is in words', () => {
  expect(hostOperationState({ state: 'awaiting_elevation' } as never)).toBe(
    '等待你在系统窗口里确认管理员权限',
  );
  expect(hostOperationState({ state: 'denied' } as never)).toBe(
    '你拒绝了管理员权限，没有做任何更改',
  );
  expect(
    hostOperationState({
      state: 'failed',
      error_code: 'sandbox.host_preparation_capability_conflict',
    } as never),
  ).toBe('失败：系统里现有的权限设置和这份方案冲突，系统没有被修改');
  expect(hostOperationState({ state: 'failed', error_code: null } as never)).toBe('失败：没有完成');
});

test('a request Aura did not send says Windows was not changed', () => {
  expect(
    hostRequestFailure(
      {
        code: 'ipc.core_not_ready',
        message: 'Aura Core 尚未就绪，这次操作没有执行；请等它就绪后重试。',
      },
      'prepare',
    ),
  ).toBe('配置兼容环境没有开始（Aura Core 还没有就绪），系统没有被修改；请稍后再试。');
  expect(
    hostOperationState({ state: 'failed', error_code: 'sandbox.elevated_state_unknown' } as never),
  ).toBe('失败：没有确认是否完成，系统可能已被部分修改；请点“修复兼容环境”重新检查');
});
