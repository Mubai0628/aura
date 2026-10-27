import { expect, test } from 'vitest';
import { userMessage } from './user-message';

// A refused model binding must tell the user what to do; the Core names the real
// reason (`models.binding.*`), and the Renderer must not fall back to the generic
// "operation not completed" copy or leak the English envelope text.
test('binding refusals explain the next action in product copy', () => {
  expect(userMessage('models.binding.credential_required')).toContain('凭据');
  expect(userMessage('models.binding.output_limit_required')).toContain('最大输出');
  expect(userMessage('models.binding.profile_disabled')).toContain('停用');
  expect(userMessage('models.binding.profile_missing')).toContain('重新选择');
  expect(userMessage('models.binding.endpoint_missing')).toContain('连接');
});

test('unknown technical text still falls back instead of reaching the user', () => {
  expect(userMessage('models.configuration.failed')).toBe('操作未完成，请刷新状态后重试。');
});

test('an attachment refusal retains a quoted real filename and Core’s Chinese reason', () => {
  const sentence = '「SaveTik.io_7638307602505000000.mp4」不在这个对话的工作区里，请先放进工作区。';
  expect(userMessage(`agent.attachment_outside_workspace: ${sentence}`, '消息未发送。')).toBe(
    sentence,
  );
  expect(userMessage('读取失败（agent.store_unknown），请重试。', '消息未发送。')).toBe(
    '消息未发送。',
  );
  for (const filename of ['agent.session_missing.mp4', 'model_transport_connect_timeout.mp4']) {
    const reason = `「${filename}」无法读取，请选择其他文件。`;
    expect(userMessage(`agent.attachment_unreadable: ${reason}`)).toBe(reason);
  }
});

test.each(['tool_policy', 'security_authority', 'model_authority'])(
  'a configuration refusal directs recovery without displaying the envelope (%s)',
  (capability) => {
    const text = userMessage(
      `This capability is unavailable: core.${capability}.config_unavailable`,
    );
    expect(text).toContain('恢复');
    expect(text).toContain('设置');
    expect(text).not.toContain('core.');
    expect(text).not.toContain('This capability');
  },
);

// The newer Core areas refuse as `<code>: <sentence>`. The sentence is written for the user;
// the code is not. Before, any dotted identifier sent the whole text to the generic fallback.
test('a refusal written as code and sentence shows the sentence only', () => {
  expect(userMessage('git.remote_required: 请指定已配置的远端，或先为本地分支设置上游。')).toBe(
    '请指定已配置的远端，或先为本地分支设置上游。',
  );
  expect(userMessage(new Error('plugins.invalid: 插件清单缺少 name 字段。'))).toBe(
    '插件清单缺少 name 字段。',
  );
  expect(userMessage('fs.write 需要你的确认。')).toBe('fs.write 需要你的确认。');
  expect(userMessage('terminal.ended: Core stopped; open a new terminal', '终端已结束。')).toBe(
    '终端已结束。',
  );
  expect(userMessage('请稍后再试（agent.store_unknown）', '未完成。')).toBe('未完成。');
});

test('an error envelope is read by its code first, then by its own sentence', () => {
  expect(
    userMessage({
      code: 'models.binding.credential_required',
      message: 'This Endpoint has no usable credential, so the model cannot be bound.',
    }),
  ).toContain('凭据');
  expect(
    userMessage({ code: 'git.status_failed', message: 'git.status_failed: 读取状态失败。' }),
  ).toBe('读取状态失败。');
  expect(userMessage({ code: 'models.unknown', message: 'Something broke.' }, '没有保存。')).toBe(
    '没有保存。',
  );
});

test('a Hook refusal of a message shows the Hook and its reason, not the code', () => {
  expect(
    userMessage('hooks.message_blocked: Hook policy 没有接收这条消息：请先写测试 (see rule 2.1)'),
  ).toBe('Hook policy 没有接收这条消息：请先写测试 (see rule 2.1)');
  expect(userMessage('hooks.turn_blocked')).toContain('设置 → Hook');
  expect(userMessage('hooks.step_blocked')).toContain('Hook');
  expect(userMessage('hooks.completion_blocked')).toContain('连续 3 次');
});

// A step that ended on an error Core did not expect, or a compaction that did not succeed, says
// that the task paused and how to go on, instead of only "任务暂时中断".
test('an unexpected step failure and a failed compaction say the task paused and how to go on', () => {
  const fallback = '任务暂时中断。过程输出仍可阅读，请在输入框说明下一步。';
  const unexpected = userMessage('agent.execution_interrupted', fallback);
  expect(unexpected).not.toBe(fallback);
  expect(unexpected).toContain('继续');
  const compaction = userMessage('context.compaction_failed: Failed { .. }', fallback);
  expect(compaction).toContain('压缩');
  expect(compaction).toContain('继续');
  const looped = userMessage(
    'agent.provider_failed: model_reasoning_loop (StreamInterrupted, HTTP None)',
    fallback,
  );
  expect(looped).toContain('同一段');
  expect(looped).toContain('继续');
});

// Two tasks stopped with only "任务暂时中断"; Core had stored why.
test('a model service that could not be reached says so and what to do next', () => {
  const fallback = '任务暂时中断。过程输出仍可阅读，请在输入框说明下一步。';
  const stored = 'agent.provider_failed: model_transport_connect_timeout (Transport, HTTP None)';
  const said = userMessage(stored, fallback);
  expect(said).not.toBe(fallback);
  expect(said).toContain('连接模型服务超时');
  expect(said).toContain('继续');
  const late = 'agent.provider_failed: model_transport_firstevent_timeout (Transport, HTTP None)';
  expect(userMessage(late, fallback)).toContain('First Event Timeout');
  const failed = 'agent.provider_failed: model_transport_failed (Transport, HTTP None)';
  expect(userMessage(failed, fallback)).toContain('连接模型服务失败');
});

// Every failure Core stored for them says what happened and what to do next.
test('a model service that answered badly or failed midway says so and what to do next', () => {
  const fallback = '任务暂时中断。过程输出仍可阅读，请在输入框说明下一步。';
  const said = (code: string, kind = 'Protocol') =>
    userMessage(`agent.provider_failed: ${code} (${kind}, HTTP None)`, fallback);
  for (const code of [
    'provider_sse_invalid',
    'agent.transport_interrupted',
    'request_timeout',
    'internal_server_error',
    'rate_limit_exceeded',
  ]) {
    expect(said(code), code).not.toBe(fallback);
    expect(said(code), code).toContain('继续');
  }
  expect(said('provider_sse_invalid')).toContain('数据');
  expect(said('agent.transport_interrupted')).toContain('没有自动重发');
  expect(said('internal_server_error', 'Transport')).toContain('服务端');
  expect(said('context_length_exceeded')).toContain('上下文');
});

// An overloaded service says so, and that it was asked again.
test('an overloaded model service says so and that it was retried', () => {
  const fallback = '任务暂时中断。过程输出仍可阅读，请在输入框说明下一步。';
  for (const code of [
    'server_is_overloaded',
    'overloaded_error',
    'service_unavailable',
    'request_timeout',
  ]) {
    const said = userMessage(`agent.provider_failed: ${code} (Protocol, HTTP None)`, fallback);
    expect(said, code).not.toBe(fallback);
    expect(said, code).toContain('自动重试');
    expect(said, code).toContain('继续');
  }
});
