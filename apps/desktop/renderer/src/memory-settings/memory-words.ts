import { answerMissing, connectionCause, refusedAction } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/**
 * What each memory action says when it did not go through: what did not happen, why, what did
 * not change, and what to do next.
 */

/** A memory request Core or Main refused, keeping its envelope so its code is not lost. */
export class MemoryRequestFailed extends Error {
  constructor(readonly failure: unknown) {
    const message =
      typeof failure === 'object' && failure !== null
        ? (failure as { message?: unknown }).message
        : undefined;
    super(typeof message === 'string' ? message : 'memory request failed');
  }
}

/** The envelope a failed request carried, or the error itself. */
export const failureOf = (cause: unknown): unknown =>
  cause instanceof MemoryRequestFailed ? cause.failure : cause;

/** What one failed memory action says. */
export type MemoryFailure = (cause: unknown) => string;

/** `what` did not happen: why, and `rest`; with `mayHave`, one whose answer did not come may have. */
const failed =
  (what: string, rest: string, mayHave?: string): MemoryFailure =>
  (cause) => {
    const error = failureOf(cause);
    if (mayHave !== undefined && answerMissing(error))
      return `没有等到 Aura Core 的答复（${connectionCause(error)}），${mayHave}；请刷新后核对。`;
    return refusedAction(error, what, rest);
  };

/** Main does these itself, and its own sentence already says what happened and what to do. */
const mainAction =
  (what: string, rest: string): MemoryFailure =>
  (cause) => {
    const error = failureOf(cause);
    return userMessage(error, '') || refusedAction(error, what, rest);
  };

export const MEMORY_FAILURE = {
  saveSettings: failed(
    '记忆设置没有保存',
    '仍按原来的设置运行；请稍后重试。',
    '记忆设置可能已经保存',
  ),
  openLog: mainAction('整理日志没有打开', '请稍后重试。'),
  openDirectory: mainAction('记忆目录没有打开', '请检查目录是否还在，稍后重试。'),
  export: mainAction('Vault 没有导出', '请稍后重试。'),
  validate: failed('校验没有完成', '记忆内容没有改变；请稍后重试。'),
  preview: failed('修复预览没有生成', '记忆内容没有改变；请稍后重试。'),
  rebuild: failed('索引没有重建', '记忆内容没有改变；请稍后重试。', '索引可能已经开始重建'),
  apply: failed('修复没有应用', '记忆文件没有改变；请重新预览后再应用。', '修复可能已经应用'),
} as const satisfies Record<string, MemoryFailure>;
