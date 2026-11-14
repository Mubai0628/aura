import { connectionCause } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/** A plain folder has nothing to compare: the review page says so instead of reporting a failure. */
export const NOT_A_REPOSITORY =
  '这个工作区不是 Git 仓库，审查页没有可比较的改动。需要时在终端运行 git init，然后点刷新。';

const codeOf = (error: unknown): string | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
};

/** Core read the workspace and found no Git repository there. */
export const isNotRepository = (error: unknown): boolean => codeOf(error) === 'git.not_repository';

/** Why Git could not be read, in words, with what to do next. */
export function gitReadFailure(error: unknown): string {
  switch (codeOf(error)) {
    case 'git.unsafe_directory':
      return 'Git 拒绝读取：仓库所有者与当前 Windows 用户不同。在终端核对 safe.directory 设置后点刷新。';
    case 'git.index_locked':
      return 'Git 索引正被另一个 Git 操作锁定，等它结束后点刷新。';
    case 'git.timed_out':
      return 'Git 读取超时（仓库较大或电脑繁忙）。稍后点刷新。';
    default:
      return userMessage(
        error,
        '没有读到这个仓库的状态。稍后点刷新；仍然不行时在终端运行 git status 查看原因。',
      );
  }
}

/** A file's diff that could not be read: with nothing shown yet, or with the last one `kept`. */
export function diffReadFailure(error: unknown, kept: boolean): string {
  const cause = connectionCause(error) ?? 'Git 读取失败或服务没有回应';
  return userMessage(
    error,
    kept
      ? `最新的差异没读出来（${cause}），显示的是上次读到的内容；点“刷新”可以再读一次。`
      : `这个文件的差异没读出来（${cause}）；点“重试读取”再读一次。`,
  );
}
