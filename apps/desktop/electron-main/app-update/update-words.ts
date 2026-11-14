import { DownloadRefused } from './download.js';
import { SetupRefusal } from './installation.js';

/**
 * What the update page says when a step did not go through: what did not happen, why, and what
 * to do next. The network's and the setup program's own English goes to Main's log only.
 */

export const reasonOf = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);
const HAS_CHINESE = /[㐀-鿿]/u;

/** Why a check failed, in words the page can show (an English network error is not shown). */
export const checkFailure = (error: unknown): string =>
  HAS_CHINESE.test(reasonOf(error))
    ? reasonOf(error)
    : '检查更新失败，可能是网络或代理暂时不通。请稍后点“检查更新”重试。';

/** Why GitHub refused a check, by the status it answered. */
export const checkRefused = (status: number): string =>
  status === 403 || status === 429
    ? '检查更新失败：GitHub 暂时限制了检查次数（同一网络检查太频繁），这次没能检查新版本。约一小时后再点“检查更新”重试，或换一个代理节点。'
    : status >= 500
      ? '检查更新失败：GitHub 暂时不可用，这次没能检查新版本。请稍后点“检查更新”重试。'
      : `检查更新失败：GitHub 拒绝了这次检查（HTTP ${status}），这次没能检查新版本。请稍后点“检查更新”重试。`;

/** A check that found what it looked for but could not write down when it checked. */
export const SETTINGS_NOT_WRITTEN =
  '检查结果没能写入磁盘（磁盘已满或没有写入权限），下次启动会重新检查；请腾出空间或检查数据文件夹权限。';

/** Why a file of the update could not be written, by the system's code. */
const writeFailure = (code: string): string => {
  switch (code) {
    case 'ENOSPC':
      return '更新包没能写入：磁盘空间不足。已下载的部分会保留，腾出磁盘空间后点“继续下载”接着下载。';
    case 'EPERM':
    case 'EACCES':
      return '更新包没能写入：没有写入权限，或文件被安全软件拦下了。已下载的部分会保留，稍后点“继续下载”接着下载。';
    case 'EBUSY':
      return '更新包没能写入：文件正被另一个程序占用。已下载的部分会保留，稍后点“继续下载”接着下载。';
    default:
      return '更新包没能写入磁盘。已下载的部分会保留，稍后点“继续下载”接着下载。';
  }
};

/** Why a download stopped part way and how it goes on: what arrived is kept for the next one. */
export function downloadFailure(error: unknown): string {
  if (error instanceof DownloadRefused) return error.message;
  const code = typeof error === 'object' && error !== null ? Reflect.get(error, 'code') : undefined;
  if (typeof code === 'string' && /^E[A-Z]+$/u.test(code)) return writeFailure(code);
  const reason = reasonOf(error);
  return HAS_CHINESE.test(reason)
    ? `${reason}。已下载的部分会保留，点“继续下载”从中断处接着下载。`
    : '下载中断，可能是网络或代理暂时不通。已下载的部分会保留，网络恢复后点“继续下载”从中断处接着下载。';
}

/** The setup program's refusals Aura words itself, by the code it reports. */
const SETUP_REFUSALS: Readonly<Record<string, string>> = {
  no_previous_version:
    '找不到可以回去的上一版本（可能已被清理），没有回退；可以从 GitHub 下载旧版安装包重新安装。',
  previous_incomplete:
    '上一版本的文件不完整（可能已被清理或损坏），没有回退；可以从 GitHub 下载旧版安装包重新安装。',
  release_not_signed:
    '这个更新包没有签名，Aura 没有安装它，已删除这次下载的更新包；请稍后重新检查更新。',
  signature_invalid:
    '这个更新包的签名不对，Aura 没有安装它，已删除这次下载的更新包；请稍后重新检查更新。',
  files_mismatch:
    '解开的更新文件和发布内容对不上，Aura 没有安装它，已删除这次下载的更新包；点“下载更新”会重新下载。',
};

const namedRefusal = (error: unknown): string | undefined =>
  error instanceof SetupRefusal && error.code !== undefined
    ? SETUP_REFUSALS[error.code]
    : undefined;

/** Why going back did not happen; the setup program's English is not shown. */
export function rollBackFailure(error: unknown): string {
  const named = namedRefusal(error);
  if (named !== undefined) return named;
  return error instanceof SetupRefusal && HAS_CHINESE.test(error.message)
    ? error.message
    : '没能回到上一版本，当前版本照常使用；请稍后再试，仍然失败时可以从 GitHub 下载旧版安装包重新安装。';
}

/** Why a downloaded package was not installed, once it has been deleted. */
export const applyFailure = (error: unknown): string =>
  namedRefusal(error) ??
  '更新没能安装，已删除这次下载的更新包。点“下载更新”会重新下载；仍然失败时请重新运行安装程序修复。';

/** A package that was not installed and could not be deleted either. */
export const PACKAGE_KEPT =
  '更新没能安装，这次下载的更新包也没能删掉（可能正被其他程序占用）；稍后点“下载更新”再试，仍然失败时请重新运行安装程序修复。';
