import { expect, test } from 'vitest';
import {
  changesFailure,
  modeFailure,
  revokeFailure,
  stopFailure,
  worktreeOpenFailure,
} from './failure-words';

// What a sub-agent action that did not go through says: why, what it means for the sub-agent,
// and what to do next.

const timeout = {
  code: 'ipc.request_timeout',
  message: 'Core request timed out: agents.stop',
  terminal: 'cancelled',
  details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
};
const lost = { code: 'ipc.transport_lost', message: 'Transport is closed', details: {} };
const english = { code: 'agents.internal', message: 'internal failure', details: {} };
const conflict = { code: 'security.revision_conflict', message: 'stale', details: {} };

test('a stop Core did not confirm may still be stopping; a refused one is still running', () => {
  expect(stopFailure(timeout)).toBe(
    '服务没有及时确认，子 Agent 可能仍在停止中。请稍后看它的状态，仍在运行就再点一次“停止”。',
  );
  expect(stopFailure(new Error('bridge gone'), true)).toBe(stopFailure(timeout));
  expect(stopFailure(english)).toBe('没有停下，子 Agent 仍在运行，请稍后再点一次“停止”。');
});

test('a mode that was not switched keeps the old one, and an unconfirmed one says to look', () => {
  expect(modeFailure(conflict)).toBe(
    '没有切换档位（它的状态刚刚变化），仍是原来的档位；请重新打开菜单再选一次。',
  );
  expect(modeFailure(english)).toBe('没有切换档位，仍是原来的档位；请重新打开菜单再选一次。');
  expect(modeFailure(timeout)).toBe(
    '档位是否已切换没有确认（Aura Core 没有及时回复）；请重新打开菜单看当前档位，需要时再选一次。',
  );
});

test('a worktree that did not open says why it may be gone', () => {
  expect(worktreeOpenFailure(english)).toBe(
    '没有打开它的工作副本（副本可能已合回或丢弃）；请看它的状态，副本还在时再试一次。',
  );
  expect(worktreeOpenFailure(lost)).toBe(
    '没有打开它的工作副本（与 Aura Core 的连接断开了）；请看它的状态，副本还在时再试一次。',
  );
});

test('an allowance that was not revoked is still in force', () => {
  expect(revokeFailure(english)).toBe(
    '没有撤销，这项允许仍然有效，相关操作仍会不经询问进行；请再点一次“撤销”，需要时先停止这个子 Agent。',
  );
  expect(revokeFailure(timeout)).toBe(
    '撤销是否生效没有确认（Aura Core 没有及时回复），这项允许可能仍然有效；请重新打开这个窗口查看，需要时再点一次“撤销”，或先停止这个子 Agent。',
  );
});

test('changes that could not be read leave the worktree as it was', () => {
  expect(changesFailure(english)).toBe(
    '没读到工作副本的改动（Git 读取失败），副本本身不受影响；收起“看改动”再打开会重新读取。',
  );
  expect(changesFailure(lost)).toBe(
    '没读到工作副本的改动（与 Aura Core 的连接断开了），副本本身不受影响；收起“看改动”再打开会重新读取。',
  );
});
