import { connectionCause } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/**
 * What the file panel says when a file, a folder or the index could not be read: why, what it
 * means and what to do, with the buttons the panel really has (重新读取 on a file, 重试 on a
 * folder or a locate).
 */

const codeOf = (error: unknown): string | undefined => {
  const code =
    typeof error === 'object' && error !== null ? (error as { code?: unknown }).code : undefined;
  return typeof code === 'string' ? code : undefined;
};

const LOCKED =
  '读不出这个文件：它可能正被其他程序占用，或没有读取权限。关闭占用它的程序后点“重新读取”。';
const BUSY = '这个文件正被另一项操作使用，请稍后点“重新读取”。';
const OUT_OF_REACH =
  '工作区文件夹现在访问不到（可能被移动、改名或所在磁盘未连接）。连上后点“重新读取”。';

const FILE_FAILURES: Readonly<Record<string, string>> = {
  'file.not_found': '找不到这个文件，它可能已被移动或删除。',
  'file.wrong_kind': '这是一个文件夹，不是文件。',
  'file.scope_unavailable': '这个文件不在当前工作区里。',
  'file.encoding_ambiguous': '这个文件的编码无法确定，暂不能显示。',
  'fs.io_failed': LOCKED,
  'fs.permission_denied': LOCKED,
  'fs.resource_busy': BUSY,
  'file.external_resource_busy': BUSY,
  'workspace.busy': BUSY,
  'workspace.archived': '这个工作区已归档，文件暂不能读取。请先在 设置 → 归档 里恢复。',
  'workspace.offline': OUT_OF_REACH,
  'workspace.unavailable': OUT_OF_REACH,
  'workspace.catalog_unavailable': '工作区还在建立文件索引，请稍后点“重新读取”。',
};

/** A file the 文件 tab could not read. */
export function fileFailure(error: unknown): string {
  const known = FILE_FAILURES[codeOf(error) ?? ''];
  if (known !== undefined) return known;
  const cause = connectionCause(error);
  return userMessage(error, `文件没有读出来${cause ? `（${cause}）` : ''}；请点“重新读取”。`);
}

const LISTING_LOCKED = '文件列表读不出来：没有权限读取这个文件夹，或它正被占用。处理后点“重试”。';
const LISTING_OUT_OF_REACH =
  '文件列表读不出来：工作区文件夹现在访问不到（可能被移动、改名或所在磁盘未连接）。连上后点“重试”，或在侧栏菜单点“更改位置…”。';

const LISTING_FAILURES: Readonly<Record<string, string>> = {
  'workspace.catalog_unavailable': '文件列表暂时读不出来：工作区还在建立文件索引，请稍后重试。',
  'file.not_found': '文件列表暂时读不出来：这个文件夹已不存在。',
  'fs.io_failed': LISTING_LOCKED,
  'fs.permission_denied': LISTING_LOCKED,
  'fs.resource_busy': LISTING_LOCKED,
  'workspace.busy': LISTING_LOCKED,
  'workspace.offline': LISTING_OUT_OF_REACH,
  'workspace.unavailable': LISTING_OUT_OF_REACH,
  'workspace.archived': '文件列表读不出来：这个工作区已归档。请先在 设置 → 归档 里恢复。',
};

/** A folder the file tree could not list. */
export function listingFailure(error: unknown): string {
  const known = LISTING_FAILURES[codeOf(error) ?? ''];
  if (known !== undefined) return known;
  const cause = connectionCause(error);
  return userMessage(error, `文件列表没有读出来${cause ? `（${cause}）` : ''}；请点“重试”。`);
}

/** 在文件树中显示 waited for the index the whole time it could. */
export const NOT_LOCATED_WHILE_INDEXING = '还没定位到：工作区还在建立文件索引。稍后点“重试”。';

/** Why an index build failed, in words: Core records the system's own text for it. */
export function indexFailureCause(failure: string | null | undefined): string {
  if (!failure) return '原因没有记下';
  if (/[㐀-鿿]/u.test(failure)) return failure;
  if (/os error 5\b|access is denied|permission denied/iu.test(failure))
    return '读取工作区文件夹时被拒绝（没有权限，或磁盘不可用）';
  if (/os error (?:2|3|21)\b|cannot find the (?:path|file)|no such file|not found/iu.test(failure))
    return '工作区文件夹找不到了（可能被移动、改名或所在磁盘未连接）';
  if (/root identity changed/iu.test(failure)) return '工作区文件夹在建立索引时被换掉或移动了';
  return '读取工作区文件夹时出错';
}

/** The whole line for a failed build: its cause, what still works and what to do. */
export const indexFailureText = (failure: string | null | undefined): string =>
  `文件索引没有建立：${indexFailureCause(failure)}。文件树和模型仍能逐层读文件夹和文件，只是暂时不能按名字查找；处理后点“重建索引”。`;

/** A file an older message carried could not be read; the message is unchanged. */
export const attachmentUnreadable = (error: unknown): string =>
  userMessage(
    error,
    `这个附件读不出来（${connectionCause(error) ?? '记录里可能已没有它'}）。消息本身不受影响；需要时重新附加原文件。`,
  );
