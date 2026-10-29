import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { expect, test } from 'vitest';
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
