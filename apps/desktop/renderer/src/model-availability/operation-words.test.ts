import { expect, test } from 'vitest';

import { availabilityCancelled, discoveryOutcome } from './operation-words';

// A model list request that ended says how: with its counts when part of it came back, and why
// when it failed or was cancelled; an availability test that was cancelled says why.

test('a model list request that ended says how, with its counts or why', () => {
  expect(
    discoveryOutcome('completed', {
      outcome: 'partial',
      returned_count: 12,
      created_count: 3,
      not_returned_count: 2,
    } as never),
  ).toBe('部分完成：返回 12 个，新增 3 个，2 个这次没有返回');
  expect(
    discoveryOutcome('completed', {
      outcome: 'partial',
      returned_count: 4,
      created_count: 0,
      not_returned_count: 0,
    } as never),
  ).toBe('部分完成：返回 4 个，新增 0 个');
  expect(
    discoveryOutcome('cancelled', { cancellation_reason: 'configuration_changed' } as never),
  ).toBe('已取消：连接设置已改变，请重新获取');
  expect(
    discoveryOutcome('cancelled', { cancellation_reason: 'application_restarted' } as never),
  ).toBe('已取消：Aura 重启过，请重新获取');
  expect(discoveryOutcome('cancelled', { cancellation_reason: 'target_deleted' } as never)).toBe(
    '已取消：这个连接已删除',
  );
  expect(discoveryOutcome('cancelled', { cancellation_reason: 'user_requested' } as never)).toBe(
    '已取消',
  );
  expect(discoveryOutcome('completed', { outcome: 'failed', provider_error: null } as never)).toBe(
    '失败：没有收到服务商的有效回复，请检查网络、代理和 Base URL 后重试',
  );
  expect(
    discoveryOutcome('completed', {
      outcome: 'failed',
      provider_error: { issue_code: 'provider.server_error' },
    } as never),
  ).toBe('失败：服务商返回了错误，请检查连接后重试');
  expect(discoveryOutcome('completed', { outcome: 'succeeded' } as never)).toBe('已完成');
  expect(discoveryOutcome('running', undefined)).toBe('请求中');
  expect(discoveryOutcome('queued', undefined)).toBe('排队中');
});

test('a cancelled availability test says why it was cancelled and what to do', () => {
  expect(availabilityCancelled('GPT', 'configuration_changed')).toBe(
    'GPT 的可用性测试已取消：连接设置已改变。保存后重新测试。',
  );
  expect(availabilityCancelled('GPT', 'application_restarted')).toBe(
    'GPT 的可用性测试已取消：Aura 重启过，测试没有做完。需要时重新测试。',
  );
  expect(availabilityCancelled('GPT', 'target_deleted')).toBe(
    'GPT 的可用性测试已取消：这个模型或它的连接已删除。',
  );
  expect(availabilityCancelled('GPT', 'user_requested')).toBe('GPT 的可用性测试已取消。');
  expect(availabilityCancelled('GPT', null)).toBe('GPT 的可用性测试已取消。');
});
