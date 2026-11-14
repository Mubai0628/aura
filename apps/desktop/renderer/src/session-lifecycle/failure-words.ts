import type { WorkspaceLifecycleSummary } from '@aura/contracts/generated';

import {
  answerMissing,
  connectionCause,
  isConflict,
  refusedAction,
} from '../agent-session/failure-text';

/**
 * What a sidebar or archive action that did not happen says: why, that what it acted on is as it
 * was, and what to do; one Core did not answer says it may have happened.
 */

/** A Workspace operation that ended without completing, with the sentence the user reads. */
export class LifecycleRefused extends Error {}

const codeOf = (error: unknown): string | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
};

/** A rename that did not take effect. */
export function renameFailure(error: unknown): string {
  if (isConflict(error)) return '会话名称刚被改过，这次没有改名；请关闭后重新命名。';
  if (answerMissing(error))
    return `${connectionCause(error)}，名称可能已经改好；请看侧栏里的名称。`;
  return refusedAction(error, '没有改名', '会话名称保持原样；请再试一次。');
}

/** An archive of a conversation or a Workspace (`what`) that did not take effect. */
export function archiveFailure(error: unknown, what: '会话' | '工作区'): string {
  if (error instanceof LifecycleRefused) return error.message;
  if (answerMissing(error))
    return `没有等到 Aura Core 的答复（${connectionCause(error)}），${what}可能已经归档；请看侧栏，它还在就再归档一次。`;
  return refusedAction(error, '没有归档', `${what}保持原样；请再试一次。`);
}

const ADD_REFUSALS: Readonly<Record<string, string>> = {
  'workspace.invalid_root': '这个位置不是一个现有的文件夹，没有添加工作区；请重新选择。',
  'workspace.root_conflict':
    '这个文件夹与已有的工作区重叠，没有添加；请选择另一个文件夹，或直接打开已有的工作区。',
  'workspace.path_resolution_failed':
    '打不开这个文件夹（可能已被移动、没有访问权限或所在磁盘未连接），没有添加工作区；请确认它能访问后重新选择。',
};

/** A folder that was not added as a Workspace. */
export function addWorkspaceFailure(error: unknown): string {
  const known = ADD_REFUSALS[codeOf(error) ?? ''];
  if (known !== undefined) return known;
  if (answerMissing(error)) return `${connectionCause(error)}，工作区可能已经添加；请看侧栏。`;
  return refusedAction(error, '没有添加工作区', '请重新选择文件夹再试。');
}

/** A branch was made but the conversation's model could not be given to it. */
export const BRANCH_MODEL_NOT_KEPT =
  '分支已建好，但没能沿用原会话的模型，它现在使用默认模型；需要时在输入框下方重新选择。';

const count = (summary: WorkspaceLifecycleSummary, kinds: readonly string[]): number =>
  (summary.preflight?.blockers ?? []).reduce(
    (sum, blocker) =>
      kinds.includes(blocker.blocker_kind) && 'count' in blocker ? sum + blocker.count : sum,
    0,
  );

/** A Workspace operation (`verb`) that ended without completing; the Workspace is as it was. */
export function lifecycleNotCompleted(summary: WorkspaceLifecycleSummary, verb: string): string {
  if (summary.state === 'blocked') {
    const reasons: string[] = [];
    const tasks = count(summary, ['runtime_objects_active', 'runtime_leases_active']);
    if (tasks > 0) reasons.push(`工作区里还有 ${tasks} 个没结束的任务或进程`);
    if (count(summary, ['mutation_ownership_active']) > 0)
      reasons.push('模型正在改这个工作区里的文件');
    const journals = count(summary, ['unfinished_journals']);
    if (journals > 0) reasons.push(`还有 ${journals} 项文件操作没写完`);
    if (reasons.length === 0) reasons.push('工作区里还有没完成的操作');
    return `没有${verb}：${reasons.join('；')}。工作区保持原样；等这些结束或先停止后再${verb}。`;
  }
  if (summary.error_code === 'workspace.editor_prepare_ack_timeout')
    return `没有${verb}：这个窗口没有及时确认，工作区保持原样，请再试一次。`;
  return `没有${verb}，工作区保持原样；请再试一次。`;
}

/** A delete that did not take effect, or that Core did not answer. */
export function deleteFailure(error: unknown): string {
  if (error instanceof LifecycleRefused) return error.message;
  if (answerMissing(error))
    return `没有等到 Aura Core 的答复（${connectionCause(error)}），删除可能仍在进行；稍后看它是否还在列表里，还在就再删一次。`;
  return refusedAction(error, '没有删除', '它保持原样，可以再试一次。');
}

/** A restore that did not take effect: it is still archived. */
export function restoreFailure(error: unknown): string {
  if (answerMissing(error))
    return `没有等到 Aura Core 的答复（${connectionCause(error)}），可能已经恢复；它还在归档列表里就再点一次“恢复”。`;
  return refusedAction(error, '没有恢复', '它仍在归档里，请再点一次“恢复”。', {
    conflictCause: '它刚在别处有了变化',
  });
}
