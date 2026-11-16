import { expect, test } from 'vitest';
import { TURN_FALLBACK, turnFailureMessage } from './turn-failure';
import { userMessage } from './user-message';

// A turn Aura ended for its own reason read "任务暂时中断"; a user stop looked like a fault.
test('a turn Aura ended for its own reason says why and what to do next', () => {
  expect(turnFailureMessage('agent.cancelled')).toContain('这一轮已停止');
  expect(turnFailureMessage('agent.store_unknown: the store did not answer')).toContain('数据库');
  expect(turnFailureMessage('agent.goal_budget_exhausted')).toContain('预算');
  expect(turnFailureMessage('model.credential_changed')).toContain('新设置');
  expect(turnFailureMessage('agent.history_group_missing')).toBe(TURN_FALLBACK);
  // The same code outside a turn keeps the caller's own words.
  expect(userMessage('读取失败（agent.store_unknown），请重试。', '消息未发送。')).toBe(
    '消息未发送。',
  );
});

test('applicable model and settlement failures explain how to recover', () => {
  expect(turnFailureMessage('model.profile_disabled')).toContain('已停用');
  expect(turnFailureMessage('model profile is disabled: mdl_example')).toContain('已停用');
  expect(turnFailureMessage('agent.model_binding_required')).toContain('在输入框发“继续”');
  expect(turnFailureMessage('agent.failure_settlement_pending')).toContain('停止当前任务');
});

// Every kind of model failure without a sentence of its own fell back to the generic line.
test('a model failure is told by its kind when its code has no sentence of its own', () => {
  const said = (code: string, kind: string, status = 'Some(400)') =>
    turnFailureMessage(`agent.provider_failed: ${code} (${kind}, HTTP ${status})`);
  expect(said('rate_limit_error', 'RateLimit', 'Some(429)')).toContain('限流');
  expect(said('too_many_requests', 'Protocol', 'None')).toContain('限流');
  expect(said('bad_gateway', 'Transport', 'Some(502)')).toContain('服务端出错');
  expect(said('content_policy', 'ContentFilter')).toContain('内容审核');
  expect(said('model_not_found', 'UnsupportedModel', 'Some(404)')).toContain('没有这个模型');
  expect(said('provider_output_empty', 'EmptyOutput', 'None')).toContain('没有给出任何内容');
  expect(said('invalid_value', 'InvalidRequest')).toContain('不被接受');
  for (const sentence of [said('x', 'Unknown', 'None'), said('y', 'Refusal', 'None')]) {
    expect(sentence).not.toBe(TURN_FALLBACK);
    expect(sentence).not.toMatch(/[a-z]_[a-z]/u);
  }
});
