/**
 * What Main says when Aura cannot start or its window goes away: the cause, what it means and
 * what to do, in words. The technical reason goes to the Main log.
 */

/** Why the page process ended, as Electron names it, for a sentence. */
const GONE: Readonly<Record<string, string>> = {
  oom: '内存不足',
  'memory-eviction': '内存不足',
  crashed: '程序出错',
  'abnormal-exit': '程序出错',
  killed: '被系统或其他程序结束',
  'launch-failed': '没能启动',
  'integrity-failure': '代码完整性校验没有通过',
};

/** The window was rebuilt after its page ended; drafts are kept by the page itself. */
export const rendererGoneText = (reason: string): string =>
  `Aura 界面意外退出（${GONE[reason] ?? '原因不明'}），已自动重新打开；Core 和正在运行的任务不受影响，未发送的输入已尽量保留。反复出现时请关闭不用的会话后重启 Aura。`;

export const RENDERER_CRASH_LOOP =
  'Aura 界面反复意外退出，已停止自动重新打开；Core 和正在运行的任务不受影响。请退出 Aura 后重新打开，仍然如此请重新运行安装程序修复。';

const CRASH_WINDOW_MS = 120_000;

/** Page ends within the last two minutes: the third stops the automatic reopening. */
export class RendererCrashes {
  #times: number[] = [];

  record(now: number): 'reopen' | 'stop' {
    this.#times = this.#times.filter((at) => now - at <= CRASH_WINDOW_MS);
    this.#times.push(now);
    return this.#times.length >= 3 ? 'stop' : 'reopen';
  }
}

/** The page did not load: the installed files, or a development run's server. */
export const pageLoadFailureText = (developmentUrl: string | undefined): string =>
  developmentUrl === undefined
    ? 'Aura 界面没有加载出来（界面文件缺失或损坏），Aura 现在不能使用；请重新运行安装程序修复，或在启动器中回到上一版本。'
    : `Aura 界面没有加载出来（开发服务器 ${developmentUrl} 没有响应），Aura 现在不能使用；请先启动开发服务器再打开。`;

/** Core was not started: the release failed its check, or starting its supervisor threw. */
export const startFailureText = (kind: 'release' | 'supervisor'): string =>
  kind === 'release'
    ? 'Aura 的安装文件不完整或被改动过（校验没有通过），Aura Core 没有启动，所有功能暂不可用；请重新运行安装程序修复，或在启动器中回到上一版本。'
    : 'Aura Core 没能启动（启动过程出了意外的错误，详情记在 Main 日志里），所有功能暂不可用；请重启 Aura，仍然如此请重新运行安装程序修复。';

/** What the error box says when Aura does not know where its data is. */
export function dataLocationText(error: unknown): string {
  const missing =
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    error.code === 'location_missing' &&
    'location' in error &&
    typeof error.location === 'string' &&
    error.location.length > 0
      ? error.location
      : undefined;
  return missing === undefined
    ? 'Aura 没能确定数据放在哪里，没有启动；请从开始菜单的 Aura 快捷方式打开，仍不行请重新运行安装程序修复。'
    : `数据文件夹 ${missing} 不在了（可能在没接上的移动硬盘上，或已被移动、删除），Aura 没有启动；请接回该磁盘或把文件夹放回原处，再从开始菜单打开 Aura。`;
}
