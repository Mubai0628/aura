import { answerMissing, connectionCause, refusedAction } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/**
 * What the storage page says when an action did not go through: what did not happen, why, what is
 * in force, and what to do next.
 */

const codeOf = (error: unknown): string | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
};

/** Main's own failure: its sentence says what happened and what to do. */
const MAIN_FAILURE = 'ipc.main_request_failed';

/**
 * `what` did not happen: Main's own sentence as it is; otherwise why, and `rest`. With `mayHave`,
 * one whose answer did not come may have happened; the page reads its state again.
 */
export const storageFailure =
  (what: string, rest: string, mayHave?: string) =>
  (error: unknown): string => {
    if (mayHave !== undefined && answerMissing(error))
      return `没有等到 Aura Core 的答复（${connectionCause(error)}），${mayHave}；请看这一页的提示核对。`;
    if (codeOf(error) === MAIN_FAILURE) {
      const said = userMessage(error, '');
      if (said) return said;
    }
    return refusedAction(error, what, rest);
  };

/** The move was recorded, but Aura did not close for it. */
export const NOT_CLOSED =
  'Aura 没能自动关闭，数据还没搬（搬动已记下）。请手动退出 Aura 再打开，启动时会搬。';

/** The last move did not happen: where to, why, what is in force and what can be done. */
export const moveFailed = (place: string, reason: string): string =>
  `没有搬到${place}：${reason.replace(/[。.；;，,\s]+$/u, '')}。数据仍在原来的位置，Aura 照常使用；可以换个位置再搬，或点“知道了”清掉这条记录。`;

/** Old data that was not removed; one whose answer did not come may have been. */
export const oldDataNotRemoved = (error: unknown): string =>
  answerMissing(error)
    ? `没有等到 Aura Core 的答复（${connectionCause(error)}），旧数据可能已经删除；请重新打开这一页核对。`
    : refusedAction(error, '旧数据没有删除', '空间没有释放；请稍后重试。');

/** A worktree location that was not changed. */
export const worktreeNotMoved = (error: unknown): string =>
  refusedAction(error, '工作副本位置没有改', '新的工作副本仍放在原位置；请重试。');
