import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { expect, test } from 'vitest';
import { toolCallStateLabel } from '../tool-call-state-label';
import { toolCallTone } from './tool-call-state';
import { callUnderWay, stateOf, toneOf } from './tool-call-tone';

const entry = (name: string, status: string, outcome: string | null): AgentToolActivityEntry =>
  ({
    summary: {
      tool_call_id: 'tcl_1',
      tool_batch_id: 'tbt_1',
      ordinal: 1,
      tool_id: 'tol_1',
      tool_version: { definition_revision: '2', semantic_version: null },
      display_name: name,
      status,
      outcome: outcome === null ? null : { type: outcome },
      side_effect_state: null,
      started_at: null,
      completed_at: null,
      can_cancel: false,
    },
    model_step_id: 'step',
    display: {
      title: '向你提问',
      target: null,
      summary: '等待你的回答',
      details: [],
      truncated: false,
      internal: false,
    },
  }) as unknown as AgentToolActivityEntry;

// Core reports a question still waiting for the user as an unfinished call, so the
// conversation reads it again until the question ends. It waits for the user; nothing runs.
test('a question waiting for the user says so and does not look like a running call', () => {
  const waiting = entry('query.user_question', 'running', null);
  const { outcome, text } = stateOf(waiting);
  expect(text).toBe('等待你的回答');
  expect(toneOf(outcome, waiting.summary.status)).toBe('waiting');
  expect(callUnderWay(waiting)).toBe(false);
});

test('an answered question and other running calls keep their usual state', () => {
  const answered = entry('query.user_question', 'finalized', 'succeeded');
  expect(stateOf(answered).text).toBe('成功');
  expect(toneOf(stateOf(answered).outcome, 'finalized')).toBe('success');
  const reading = entry('fs.read', 'running', null);
  expect(stateOf(reading).text).not.toBe('等待你的回答');
  expect(callUnderWay(reading)).toBe(true);
});

// A request whose target refused the connection said "工具暂不可用"; it says what happened.
test('a request whose target could not be reached names the kind, not an unavailable tool', () => {
  const refused = entry('http.request', 'finalized', 'failed');
  Object.assign(refused.summary, {
    error_code: 'tool.network.connection_refused',
    error_class: 'other',
  });
  expect(stateOf(refused).text).toBe('连接被拒绝');
  for (const [code, text] of [
    ['tool.network.host_not_found', '找不到主机'],
    ['tool.network.tls_failed', 'TLS 握手失败'],
    ['tool.network.connection_reset', '连接被断开'],
    ['tool.network.connection_failed', '连接失败'],
  ] as const) {
    Object.assign(refused.summary, { error_code: code });
    expect(stateOf(refused).text).toBe(text);
  }
});

// Two downloads that ran into the 5-minute limit read "工具暂不可用"; a timeout says it is one.
test('a transfer that ran out of time reads as a timeout, a download as 下载超时', () => {
  const download = entry('http.download', 'finalized', 'failed');
  Object.assign(download.summary, { error_code: 'tool.network.timed_out', error_class: 'other' });
  expect(stateOf(download).text).toBe('下载超时');
  const request = entry('http.request', 'finalized', 'failed');
  Object.assign(request.summary, { error_code: 'tool.network.timed_out', error_class: 'other' });
  expect(stateOf(request).text).toBe('超时');
  // Core's own time limit before the request went out, as older records classed it.
  const refused = entry('http.download', 'finalized', 'operation_rejected');
  Object.assign(refused.summary, {
    error_code: 'tool.runtime.timeout',
    error_class: 'unavailable',
  });
  expect(stateOf(refused).text).toBe('下载超时');
});

// An approval nobody answered read "权限拒绝，未执行", though the user never refused.
test('an approval that expired unanswered is not shown as a refusal', () => {
  const expired = entry('fs.write', 'finalized', 'not_executed');
  Object.assign(expired.summary, {
    outcome: { type: 'not_executed', reason: 'permission_denied' },
    error_code: 'tool.approval.expired',
    error_class: 'other',
  });
  expect(stateOf(expired).text).toBe('批准已过期，未执行');
  expect(toolCallStateLabel(expired.summary)).toBe('批准已过期，未执行');
  // A refusal the user gave still says so.
  const denied = entry('fs.write', 'finalized', 'not_executed');
  Object.assign(denied.summary, { outcome: { type: 'not_executed', reason: 'permission_denied' } });
  expect(stateOf(denied).text).toBe('权限拒绝，未执行');
});

// A secure input whose terminal had moved on from the prompt read "工具暂不可用"; nothing was typed.
test('a secure input refused because its prompt had gone says nothing was typed', () => {
  const gone = entry('terminal.request_sensitive_input', 'finalized', 'operation_rejected');
  Object.assign(gone.summary, {
    error_code: 'terminal.sensitive.prompt_gone',
    error_class: 'conflict',
  });
  expect(stateOf(gone).text).toBe('提示已不在，未输入');
  expect(toolCallStateLabel(gone.summary)).toBe('提示已不在，未输入');
});

// A sub-agent call refused for its own input, such as a name over 40 characters, read
// "工具暂不可用"; it is a request the model can correct.
test('a sub-agent call refused for its own input reads as an invalid request', () => {
  const named = entry('agents.spawn', 'finalized', 'operation_rejected');
  Object.assign(named.summary, {
    error_code: 'agents.name_invalid',
    error_class: 'invalid_request',
  });
  expect(stateOf(named).text).toBe('请求无效，未执行');
});

// A target address that did not resolve in time, before anything was sent, read "工具暂不可用".
test('a call stopped by a timeout before it ran says so', () => {
  const unresolved = entry('http.download', 'finalized', 'not_executed');
  Object.assign(unresolved.summary, {
    outcome: { type: 'not_executed', reason: 'runtime_unavailable' },
    error_code: 'tool.runtime.timeout',
    error_class: 'unavailable',
  });
  expect(stateOf(unresolved).text).toBe('下载超时，未执行');
  Object.assign(unresolved.summary, { display_name: 'http.request' });
  expect(stateOf(unresolved).text).toBe('超时，未执行');
  expect(toolCallStateLabel(unresolved.summary)).toBe('超时，未执行');
});

// A call the user refused, one a mode change voided and one a Hook stopped all read "权限拒绝".
test('a call the user refused, a mode change voided or a Hook stopped says which', () => {
  const call = entry('fs.write', 'finalized', 'not_executed');
  for (const [code, text] of [
    ['tool.approval.denied', '你拒绝了，未执行'],
    ['tool.approval.invalidated', '权限模式已变，未执行'],
    ['tool.hook.blocked', '被 Hook 拦下，未执行'],
  ] as const) {
    Object.assign(call.summary, {
      outcome: { type: 'not_executed', reason: 'permission_denied' },
      error_code: code,
      error_class: 'permission',
    });
    expect(stateOf(call).text).toBe(text);
  }
});

// Several not-run reasons read only "未执行", and the shared label showed the raw reason.
test('every reason a call was not run is said in words', () => {
  const call = entry('fs.read', 'finalized', 'not_executed');
  for (const reason of [
    'tool_not_found',
    'version_mismatch',
    'not_loaded',
    'disabled',
    'trust_required',
    'audience_denied',
    'lineage_reconciliation',
    'completion_review',
    'cancelled_before_dispatch',
    'batch_rejected',
  ]) {
    Object.assign(call.summary, { outcome: { type: 'not_executed', reason } });
    const said = stateOf(call).text;
    expect(said, reason).not.toBe('未执行');
    expect(said, reason).not.toMatch(/[a-z_]{4,}/u);
    expect(toolCallStateLabel(call.summary), reason).not.toMatch(/[a-z_]{4,}/u);
  }
});

// Every failure a backend reports while running is stored under one general code in the
// unavailable class, so a package or Git error read "工具暂不可用".
test('a failure a running tool reported is not called an unavailable tool', () => {
  const failed = entry('package.run', 'finalized', 'failed');
  Object.assign(failed.summary, {
    error_code: 'tool.runtime.dispatch_failed',
    error_class: 'unavailable',
  });
  expect(stateOf(failed).text).toBe('执行失败');
  Object.assign(failed.summary, { error_code: 'tool.runtime.unavailable' });
  expect(stateOf(failed).text).toBe('工具暂不可用');
});

// A call whose turn was stopped before anything of it was sent read 需要恢复 or 已中断.
test('a call its stopped turn never started reads as interrupted and not run', () => {
  const before = entry('computer.click', 'finalized', 'not_executed');
  Object.assign(before.summary, {
    outcome: { type: 'not_executed', reason: 'cancelled_before_dispatch' },
  });
  expect(stateOf(before).text).toBe('会话中断，未执行');
  expect(toolCallStateLabel(before.summary)).toBe('未执行 · 会话中断');
  const after = entry('computer.click', 'finalized', 'cancelled');
  Object.assign(after.summary, {
    outcome: { type: 'cancelled', reason: 'turn_interrupted' },
    side_effect_state: 'no_effect',
  });
  const { outcome, text } = stateOf(after);
  expect(text).toBe('会话中断，未执行');
  expect(toneOf(outcome, 'finalized')).toBe('unknown');
  expect(toolCallTone(after)).toBe('unknown');
  expect(toolCallStateLabel(after.summary)).toBe('未执行 · 会话中断');
  Object.assign(after.summary, { side_effect_state: 'partially_applied' });
  expect(stateOf(after).text).toBe('已中断');
  expect(toolCallTone(after)).toBe('failed');
});

test('a call the user stopped reads as stopped, one a shutdown cut as interrupted', () => {
  const stopped = entry('shell.powershell', 'finalized', 'cancelled');
  Object.assign(stopped.summary, { outcome: { type: 'cancelled', reason: 'user_requested' } });
  expect(stateOf(stopped).text).toBe('已停止');
  Object.assign(stopped.summary, { outcome: { type: 'cancelled', reason: 'core_shutdown' } });
  expect(stateOf(stopped).text).toBe('已中断');
});

// A command that exited with an error read "成功 · 进程退出，退出码 1" with a green check.
test('a command that exited with an error or was stopped reads as ended, not as a success', () => {
  for (const brief of [
    '命令报错退出（退出码 1），输出已交给模型',
    '已停止：超过时限，之前的输出已交给模型',
  ]) {
    const ran = entry('shell.powershell', 'finalized', 'succeeded');
    ran.display.summary = brief;
    const { outcome, text } = stateOf(ran);
    expect(text).toBe('已结束');
    expect(toneOf(outcome, 'finalized')).toBe('unknown');
    expect(callUnderWay(ran)).toBe(false);
  }
  const fine = entry('shell.powershell', 'finalized', 'succeeded');
  fine.display.summary = '进程退出，退出码 0';
  expect(stateOf(fine).text).toBe('成功');
});

// A call that waits for the user's answer to its approval waits for the user; nothing spins.
test('a call waiting for approval says it waits for the user and is not running', () => {
  const waiting = entry('fs.delete', 'waiting_approval', null);
  expect(toolCallStateLabel(waiting.summary)).toBe('等待你批准');
  expect(toneOf(undefined, waiting.summary.status)).toBe('waiting');
});

// A file call waiting for another call to finish with its file read 运行中, and one that gave up
// read 状态已变化.
test('a call waiting for a file another call holds says it waits, then that it did not run', () => {
  const waiting = entry('fs.write', 'waiting_resource', null);
  waiting.display.summary = '在等另一个调用用完这个文件';
  const { outcome, text } = stateOf(waiting);
  expect(text).toBe('等待中');
  expect(toneOf(outcome, waiting.summary.status)).toBe('waiting');
  const refused = entry('fs.write', 'finalized', 'operation_rejected');
  Object.assign(refused.summary, {
    error_code: 'agent.resource_wait_exhausted',
    error_class: 'conflict',
    side_effect_state: 'no_effect',
  });
  expect(stateOf(refused).text).toBe('文件被占用，未执行');
});
