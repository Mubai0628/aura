import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { expect, test } from 'vitest';
import { groupConsecutiveToolCalls, isGroupedRun } from './tool-call-group';

const entry = (
  callId: string,
  options: {
    name?: string;
    toolId?: string;
    status?: string;
    outcome?: string | null;
    reason?: string;
    errorClass?: string | null;
    errorCode?: string;
    sideEffect?: string | null;
    title?: string;
    target?: string;
  } = {},
): AgentToolActivityEntry =>
  ({
    summary: {
      tool_call_id: callId,
      tool_batch_id: 'tbt_1',
      ordinal: 1,
      tool_id: options.toolId ?? 'tol_list',
      tool_version: { definition_revision: '1', semantic_version: null },
      display_name: options.name ?? 'fs.list',
      status: options.status ?? 'finalized',
      outcome:
        options.outcome === null
          ? null
          : {
              type: options.outcome ?? 'succeeded',
              ...(options.reason === undefined ? {} : { reason: options.reason }),
            },
      // The structured class the Store recorded. Absent on a legacy row, which is a
      // different fact from "the class is other".
      ...(options.errorClass === undefined ? {} : { error_class: options.errorClass }),
      ...(options.errorCode === undefined ? {} : { error_code: options.errorCode }),
      side_effect_state: options.sideEffect ?? null,
      started_at: null,
      completed_at: null,
      can_cancel: false,
    },
    model_step_id: 'step',
    display: {
      title: options.title ?? '列出目录',
      target: options.target ?? null,
      summary: '',
      details: [],
      truncated: false,
      internal: false,
    },
    shell_query_available: false,
    permission_mode: 'read_only',
    permission_revision: 1,
  }) as unknown as AgentToolActivityEntry;

const call = (callId: string, value: AgentToolActivityEntry | undefined) => ({
  callId,
  entry: value,
});

test('consecutive identical successes collapse and keep every call identity', () => {
  const groups = groupConsecutiveToolCalls([
    call('a', entry('a', { target: 'src' })),
    call('b', entry('b', { target: 'docs' })),
    call('c', entry('c', { target: 'tests' })),
  ]);
  expect(groups).toHaveLength(1);
  expect(groups[0]?.callIds).toEqual(['a', 'b', 'c']);
  expect(groups[0]?.state).toBe('成功');
  expect(groups[0]?.title).toBe('列出目录');
  expect(isGroupedRun(groups[0]!)).toBe(true);
});

test('a different Tool, a different state or missing projection breaks the run', () => {
  const groups = groupConsecutiveToolCalls([
    call('a', entry('a')),
    call('b', entry('b', { name: 'fs.read', toolId: 'tol_read', title: '读取文件' })),
    call('c', entry('c')),
    call('d', entry('d', { outcome: 'failed' })),
    call('e', entry('e')),
    call('f', undefined),
    call('g', entry('g')),
  ]);
  expect(groups.map((group) => group.callIds)).toEqual([
    ['a'],
    ['b'],
    ['c'],
    ['d'],
    ['e'],
    ['f'],
    ['g'],
  ]);
  expect(groups.every((group) => !isGroupedRun(group))).toBe(true);
});

test('consecutive running calls of one tool share a row; queued and waiting stay apart', () => {
  const groups = groupConsecutiveToolCalls([
    call('a', entry('a', { status: 'running', outcome: null })),
    call('b', entry('b', { status: 'running', outcome: null })),
    call('c', entry('c', { status: 'queued', outcome: null })),
    call('d', entry('d', { status: 'queued', outcome: null })),
    call('e', entry('e', { status: 'waiting_permission', outcome: null })),
    call('f', entry('f', { status: 'waiting_permission', outcome: null })),
    call('g', entry('g', { status: 'finalized', outcome: 'not_executed' })),
    call('h', entry('h', { status: 'finalized', outcome: 'not_executed' })),
    call('i', entry('i', { sideEffect: 'unknown' })),
    call('j', entry('j', { sideEffect: 'unknown' })),
  ]);
  expect(groups.map((group) => group.callIds)).toEqual([
    ['a', 'b'],
    ['c'],
    ['d'],
    ['e'],
    ['f'],
    ['g', 'h'],
    ['i'],
    ['j'],
  ]);
  expect(groups[0]?.state).toBe('运行中');
  expect(groups[1]?.state).toBe('已排队');
  expect(groups[3]?.state).toBe('等待权限');
  expect(groups[5]?.state).toBe('未执行');
  expect(groups[6]?.state).toBe('结果未知');
  expect(groups.map((group) => group.tone)).toEqual([
    'running',
    'running',
    'running',
    'waiting',
    'waiting',
    'unknown',
    'unknown',
    'unknown',
  ]);
});

test('a finished call splits a running run and keeps the original order', () => {
  const groups = groupConsecutiveToolCalls([
    call('a', entry('a', { status: 'running', outcome: null })),
    call('b', entry('b', { status: 'finalized', outcome: 'succeeded' })),
    call('c', entry('c', { status: 'running', outcome: null })),
    call('d', entry('d', { status: 'running', outcome: null })),
    call('e', entry('e', { status: 'finalized', outcome: 'failed' })),
  ]);
  expect(groups.map((group) => group.callIds)).toEqual([['a'], ['b'], ['c', 'd'], ['e']]);
  expect(groups.map((group) => group.state)).toEqual(['运行中', '成功', '运行中', '执行失败']);
});

test('only the same explicit final state collapses', () => {
  const groups = groupConsecutiveToolCalls([
    call('a', entry('a', { outcome: 'succeeded' })),
    call('b', entry('b', { outcome: 'failed' })),
    call('c', entry('c', { outcome: 'failed' })),
    call('d', entry('d', { outcome: 'cancelled' })),
    call('e', entry('e', { outcome: 'cancelled' })),
    call('f', entry('f', { outcome: 'indeterminate' })),
    call('g', entry('g', { outcome: 'indeterminate' })),
  ]);
  expect(groups.map((group) => group.callIds)).toEqual([['a'], ['b', 'c'], ['d', 'e'], ['f', 'g']]);
  expect(groups.map((group) => group.state)).toEqual(['成功', '执行失败', '已取消', '结果未知']);
  expect(groups.map((group) => group.tone)).toEqual(['success', 'failed', 'failed', 'unknown']);
});

/**
 * The four Shell cleanup refusals were `operation_rejected` + `not_found` and were shown
 * as 已拒绝, which reads as a permission problem. The reason now comes from the class the
 * Store recorded, and two calls only share a row when they share that reason too.
 */
test('a rejected call is described by the class the store recorded, not by its outcome alone', () => {
  const groups = groupConsecutiveToolCalls([
    call('a', entry('a', { outcome: 'operation_rejected', errorClass: 'not_found' })),
    call('b', entry('b', { outcome: 'operation_rejected', errorClass: 'not_found' })),
    call('c', entry('c', { outcome: 'operation_rejected', errorClass: 'permission' })),
    call('d', entry('d', { outcome: 'operation_rejected', errorClass: 'conflict' })),
    call('e', entry('e', { outcome: 'operation_rejected', errorClass: 'invalid_request' })),
    call('f', entry('f', { outcome: 'operation_rejected', errorClass: 'unavailable' })),
    call('g', entry('g', { outcome: 'operation_rejected' })),
  ]);
  expect(groups.map((group) => group.callIds)).toEqual([
    ['a', 'b'],
    ['c'],
    ['d'],
    ['e'],
    ['f'],
    ['g'],
  ]);
  expect(groups.map((group) => group.state)).toEqual([
    '目标不存在',
    '权限拒绝',
    '状态已变化',
    '请求无效，未执行',
    '工具暂不可用',
    '已拒绝',
  ]);
});

test('a timeout, a tab in the background and a stale memory read say their kind', () => {
  const rejected = (id: string, errorClass: string, errorCode: string) =>
    call(id, entry(id, { outcome: 'operation_rejected', errorClass, errorCode }));
  const groups = groupConsecutiveToolCalls([
    rejected('a', 'other', 'chrome.timed_out'),
    rejected('b', 'unavailable', 'chrome.background_tab'),
    rejected('c', 'conflict', 'memory.hash_conflict'),
    rejected('d', 'unavailable', 'chrome.not_connected'),
    rejected('e', 'unavailable', 'chrome.busy'),
    call('f', entry('f', { outcome: 'cancelled', reason: 'timeout' })),
  ]);
  expect(groups.map((group) => group.state)).toEqual([
    '超时，可重试',
    '未执行：标签页在后台',
    '冲突，未写入',
    '工具暂不可用',
    '超时',
  ]);
  expect(groups.map((group) => group.callIds)).toEqual([['a'], ['b'], ['c'], ['d', 'e'], ['f']]);
});

test('a validation refusal reads as a parameter mistake and never as a permission problem', () => {
  const groups = groupConsecutiveToolCalls([
    call('a', entry('a', { outcome: 'not_executed', reason: 'schema_validation_failed' })),
    call('b', entry('b', { outcome: 'not_executed', reason: 'purpose_required' })),
    call('c', entry('c', { outcome: 'not_executed', reason: 'permission_denied' })),
    call('d', entry('d', { outcome: 'not_executed' })),
  ]);
  expect(groups.map((group) => group.state)).toEqual([
    '参数错误，未执行',
    '缺少调用说明，未执行',
    '权限拒绝，未执行',
    '未执行',
  ]);
});

test('an unconfirmed side effect outranks every error text', () => {
  const groups = groupConsecutiveToolCalls([
    call(
      'a',
      entry('a', {
        outcome: 'operation_rejected',
        errorClass: 'permission',
        sideEffect: 'unknown',
      }),
    ),
    call(
      'b',
      entry('b', { outcome: 'operation_rejected', errorClass: 'not_found', sideEffect: 'unknown' }),
    ),
  ]);
  expect(groups.map((group) => group.state)).toEqual(['结果未知', '结果未知']);
  expect(groups.map((group) => group.tone)).toEqual(['unknown', 'unknown']);
});

test('the same Tool at a different definition revision stays a separate call', () => {
  const older = entry('a');
  const newer = entry('b');
  newer.summary.tool_version = { definition_revision: '2', semantic_version: null };
  const groups = groupConsecutiveToolCalls([call('a', older), call('b', newer)]);
  expect(groups.map((group) => group.callIds)).toEqual([['a'], ['b']]);
});
