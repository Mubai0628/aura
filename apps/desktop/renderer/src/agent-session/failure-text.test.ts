import { expect, test } from 'vitest';
import { connectionCause, failedAction, isConflict } from './failure-text';

const timeout = { code: 'ipc.request_timeout', message: 'Core request timed out' };
const lost = { code: 'ipc.transport_lost', message: 'Aura Core pipe disconnected' };
const notReady = { code: 'ipc.core_not_ready', message: 'Core is not ready' };

test('the cause names Aura Core when the connection to it is why', () => {
  expect(connectionCause(timeout)).toBe('Aura Core 没有及时回复');
  expect(connectionCause(lost)).toBe('与 Aura Core 的连接断开了');
  expect(connectionCause(notReady)).toBe('Aura Core 还没有就绪');
  expect(connectionCause(new Error('Aura Core pipe disconnected'))).toBe(
    '与 Aura Core 的连接断开了',
  );
  expect(connectionCause({ code: 'agent.session_missing', message: 'x' })).toBeUndefined();
});

test('a failed read says what was not read, why, what it means and what to do', () => {
  expect(
    failedAction(timeout, '更早的消息没有读到', '已显示的内容不受影响；再向上滚动会重新读取。'),
  ).toBe(
    '更早的消息没有读到（Aura Core 没有及时回复），已显示的内容不受影响；再向上滚动会重新读取。',
  );
  // Without a cause Aura knows, the sentence still says what happened and what to do.
  expect(
    failedAction(
      { code: 'agent.history_unreadable', message: 'agent.history_unreadable' },
      '更早的消息没有读到',
      '已显示的内容不受影响；再向上滚动会重新读取。',
    ),
  ).toBe('更早的消息没有读到，已显示的内容不受影响；再向上滚动会重新读取。');
});

test('Core’s own sentence wins over the caller’s', () => {
  expect(
    failedAction(
      {
        code: 'agent.inbox_revision_conflict',
        message: 'agent.inbox_revision_conflict: 队列刚刚变了，这次没有改动。',
      },
      '这次删除没有生效',
      '请按最新的队列再操作。',
    ),
  ).toBe('队列刚刚变了，这次没有改动。');
});

test('a change made elsewhere first gets the caller’s conflict sentence', () => {
  const conflict = { code: 'task_lineage.revision_conflict', message: 'revision conflict' };
  expect(isConflict(conflict)).toBe(true);
  expect(isConflict({ code: 'configuration.revision_stale', message: 'stale' })).toBe(true);
  expect(isConflict(timeout)).toBe(false);
  expect(
    failedAction(conflict, '没能开始实施计划', '计划保持原样；请稍后再点“实施”。', {
      conflict: '计划刚刚有了变化，这次没有开始实施；请看最新的计划后再点“实施”。',
    }),
  ).toBe('计划刚刚有了变化，这次没有开始实施；请看最新的计划后再点“实施”。');
});

test('a command Core may have run before it stopped answering is not said to have failed', () => {
  const unconfirmed = {
    code: 'ipc.request_timeout',
    message: 'Core request timed out',
    details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
  };
  expect(failedAction(unconfirmed, '没能继续', '任务仍停在这里；请稍后再试。')).toContain(
    '是否已生效还不确定',
  );
});
