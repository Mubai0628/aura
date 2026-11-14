import { isUnconfirmedCommandOutcome } from '../../../shared/ipc-request-failure';
import { connectionCause, failedAction } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/**
 * What a sub-agent action that did not go through says: why, what it means for the sub-agent and
 * what to do next. A request that may have reached Core says its effect is not known.
 */

/** Whether Core may have acted on the request although no answer came back. */
const mayHaveActed = (error: unknown): boolean =>
  typeof error === 'object' &&
  error !== null &&
  'code' in error &&
  isUnconfirmedCommandOutcome(error as Parameters<typeof isUnconfirmedCommandOutcome>[0]);

/** 停止 that did not go through; `thrown` when the request itself threw, so nothing is known. */
export const stopFailure = (error: unknown, thrown = false): string =>
  thrown || mayHaveActed(error)
    ? '服务没有及时确认，子 Agent 可能仍在停止中。请稍后看它的状态，仍在运行就再点一次“停止”。'
    : failedAction(error, '没有停下', '子 Agent 仍在运行，请稍后再点一次“停止”。');

/** A mode switch that did not go through. */
export const modeFailure = (error: unknown): string =>
  mayHaveActed(error)
    ? '档位是否已切换没有确认（Aura Core 没有及时回复）；请重新打开菜单看当前档位，需要时再选一次。'
    : failedAction(error, '没有切换档位', '仍是原来的档位；请重新打开菜单再选一次。', {
        conflict: '没有切换档位（它的状态刚刚变化），仍是原来的档位；请重新打开菜单再选一次。',
      });

/** Its worktree did not open in the file manager. */
export const worktreeOpenFailure = (error: unknown): string =>
  userMessage(
    error,
    `没有打开它的工作副本（${connectionCause(error) ?? '副本可能已合回或丢弃'}）；请看它的状态，副本还在时再试一次。`,
  );

/** An allowance that was not revoked is still in force. */
export const revokeFailure = (error: unknown): string =>
  mayHaveActed(error)
    ? '撤销是否生效没有确认（Aura Core 没有及时回复），这项允许可能仍然有效；请重新打开这个窗口查看，需要时再点一次“撤销”，或先停止这个子 Agent。'
    : failedAction(
        error,
        '没有撤销',
        '这项允许仍然有效，相关操作仍会不经询问进行；请再点一次“撤销”，需要时先停止这个子 Agent。',
      );

/** The worktree's changes were not read; reading them changes nothing. */
export const changesFailure = (error: unknown, thrown = false): string =>
  userMessage(
    error,
    `没读到工作副本的改动（${connectionCause(error) ?? (thrown ? '与 Aura Core 的连接出了问题' : 'Git 读取失败')}），副本本身不受影响；收起“看改动”再打开会重新读取。`,
  );
