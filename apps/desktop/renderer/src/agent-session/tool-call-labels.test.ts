import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { expect, test } from 'vitest';
import { toolCallStateLabel } from '../tool-call-state-label';
import { callUnderWay, stateOf, toneOf } from './tool-call-tone';

// A row's state names the cause its code records, so it never contradicts the line under it:
// a call the user took over, a file another program holds, a Chrome that did not answer.

const call = (
  name: string,
  outcome: Record<string, unknown> | null,
  more: Record<string, unknown> = {},
  summary = '',
): AgentToolActivityEntry =>
  ({
    summary: {
      tool_call_id: 'tcl_1',
      tool_batch_id: 'tbt_1',
      ordinal: 1,
      tool_id: 'tol_1',
      tool_version: { definition_revision: '1', semantic_version: null },
      display_name: name,
      status: 'finalized',
      outcome,
      side_effect_state: 'no_effect',
      started_at: null,
      completed_at: null,
      can_cancel: false,
      ...more,
    },
    model_step_id: 'step',
    display: {
      title: name,
      target: null,
      summary,
      details: [],
      truncated: false,
      internal: false,
    },
  }) as unknown as AgentToolActivityEntry;

const rejected = (name: string, code: string, error_class: string) =>
  stateOf(call(name, { type: 'operation_rejected' }, { error_code: code, error_class })).text;

test('a refusal names the cause its code records', () => {
  expect(rejected('computer.act', 'computer.taken_over', 'other')).toBe('你已接管，未执行');
  expect(rejected('computer.act', 'computer.busy', 'unavailable')).toBe('电脑被占用，未执行');
  expect(rejected('computer.act', 'computer.stopped', 'other')).toBe('已停止，未执行');
  expect(rejected('computer.act', 'computer.cancelled', 'other')).toBe('已取消，未执行');
  expect(rejected('computer.act', 'computer.timeout', 'other')).toBe('超时，未执行');
  expect(rejected('chrome.click', 'chrome.timed_out', 'other')).toBe('超时，未执行');
  expect(rejected('chrome.click', 'chrome.stopped', 'other')).toBe('已取消，未执行');
  expect(rejected('chrome.click', 'chrome.disconnected', 'unavailable')).toBe(
    'Chrome 已断开，未执行',
  );
  expect(rejected('fs.write', 'fs.file_in_use', 'conflict')).toBe('文件被别的程序占用，未执行');
  expect(rejected('fs.write', 'fs.deferred_by_editor_activity', 'conflict')).toBe(
    '文件正在编辑，未写入',
  );
  expect(rejected('memory.write', 'memory.secret_refused', 'invalid_request')).toBe(
    '含密钥，未写入',
  );
  expect(rejected('git.commit', 'git.repository_changed', 'conflict')).toBe('仓库已变化，未执行');
  expect(rejected('http.request', 'tool.runtime.io_limit_exceeded', 'invalid_request')).toBe(
    '超过大小上限',
  );
  expect(rejected('http.request', 'tool.runtime.http_gateway_failed', 'unavailable')).toBe(
    '网络请求没有完成',
  );
  const partial = call(
    'http.download',
    { type: 'failed' },
    {
      error_code: 'tool.result.http_partial_response',
      error_class: 'other',
    },
  );
  expect(stateOf(partial).text).toBe('仅收到分段内容，未保存');
});

// A call its run ended without a result is not at work: it does not spin.
test('a call left without a result when its run ended is interrupted, not running', () => {
  const left = call('shell.powershell', null, {
    status: 'recovery_required',
    side_effect_state: null,
  });
  const { outcome, text } = stateOf(left);
  expect(text).toBe('已中断，结果未知');
  expect(toneOf(outcome, 'recovery_required')).toBe('unknown');
  expect(callUnderWay(left)).toBe(false);
});

// /init that wrote nothing because the user was editing the files did not succeed.
test('an /init that wrote nothing is not shown as a success', () => {
  const deferred = call(
    'workspace_init.complete',
    { type: 'succeeded' },
    {},
    '没有写入：你正在编辑器里改这些文件；保存或关闭后再运行 /init。',
  );
  expect(stateOf(deferred)).toEqual({ outcome: 'ended', text: '未写入' });
});

// The debug card uses words, never a raw reason value.
test('an unknown outcome reads as words on every surface', () => {
  const unknown = call('shell.powershell', {
    type: 'indeterminate',
    reason: 'reconciliation_inconclusive',
  });
  expect(toolCallStateLabel(unknown.summary)).toBe('结果未知');
});

// A program list read while computer control is off read nothing; a call Core stopped before it
// started names why.
test('a list read with computer control off, and a call stopped before it started, say so', () => {
  const off = call(
    'computer.apps',
    { type: 'succeeded' },
    {},
    '电脑操控没有打开，没有读取程序列表；需要时在 设置 → 电脑操控 打开。',
  );
  expect(stateOf(off)).toEqual({ outcome: 'ended', text: '未读取' });
  const notStarted = (error_class: string) =>
    stateOf(
      call(
        'fs.read',
        { type: 'not_executed', reason: 'runtime_unavailable' },
        { error_code: 'tool.runtime.dispatch_failed', error_class },
      ),
    ).text;
  expect(notStarted('other')).toBe('会话中断，未执行');
  expect(notStarted('conflict')).toBe('开始前被拦下，未执行');
  expect(notStarted('permission')).toBe('权限拒绝，未执行');
});
