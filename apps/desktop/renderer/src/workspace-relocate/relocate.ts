import { userMessage } from '../agent-session/user-message';
import {
  currentWorkspace,
  lifecycleOutcome,
  withdrawLifecycle,
} from '../session-lifecycle/workspace-lifecycle';

/** Core's answer when the chosen folder is not the folder object the Workspace had. */
const CONFIRMATION_REQUIRED = 'workspace.relocate_confirmation_required';

/** Why a move did not happen, said for the move; other codes fall back to the shared sentences. */
const REFUSALS: Record<string, string> = {
  'workspace.root_conflict': '这个文件夹已经是另一个工作区，请选择别的文件夹。',
  'workspace.invalid_root': '选择的不是一个文件夹，请重新选择。',
  'workspace.path_resolution_failed': '打不开这个文件夹。请确认它还在、能够访问，再重新选择。',
  'workspace.lifecycle_in_progress': '这个工作区正在进行另一项操作，请等它完成后再更改位置。',
  'workspace.revision_conflict': '工作区刚刚有了变化，请重新更改位置。',
  'workspace.archived': '这个工作区已经归档，请先在“设置 → 归档”里恢复。',
};
const BLOCKED = '工作区里还有没完成的文件操作，工作区仍在原来的位置。等这些操作完成后再更改位置。';
const NOT_FINISHED = '更改位置没有完成，工作区仍在原来的位置。请重新更改位置。';

/** A move that did not happen, with the sentence the user reads. */
export class RelocateRefused extends Error {}

const codeOf = (error: unknown): string | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as Record<string, unknown>).code;
  return typeof code === 'string' ? code : undefined;
};

/** The sentence for a failed move. */
export function relocateMessage(error: unknown): string {
  if (error instanceof RelocateRefused) return error.message;
  const code = codeOf(error);
  return (code !== undefined ? REFUSALS[code] : undefined) ?? userMessage(error, NOT_FINISHED);
}

/** The folder the user picks in Main's dialog, or undefined when they cancel it. */
export async function pickWorkspaceFolder(): Promise<string | undefined> {
  const picked = await window.aura.v1.selectWorkspaceDirectory();
  if (!picked.ok)
    throw new RelocateRefused(userMessage(picked.error.message, '未能打开文件夹选择器，请重试。'));
  return picked.value.status === 'selected' ? picked.value.selected_root : undefined;
}

/**
 * Points a Workspace at `folder`. Core switches at once when it is the folder object the
 * Workspace had (moved or renamed on its drive); another folder, such as a copy, moves only once
 * the user has said it is the same project, and until then the answer is `'confirm'`. Either way
 * Core withdraws the Workspace's earlier path approvals, so later uses are asked again.
 */
export async function relocateWorkspace(
  workspaceId: string,
  folder: string,
  confirmed: boolean,
): Promise<'moved' | 'confirm'> {
  const current = await currentWorkspace(workspaceId);
  const begun = await window.aura.v1.relocateWorkspace({
    workspace_id: workspaceId,
    selected_root: folder,
    confirmation: confirmed ? 'user_confirmed_same_logical_project' : 'require_same_object',
    expected_workspace_revision: current.revision,
  });
  if (!begun.ok) {
    if (codeOf(begun.error) === CONFIRMATION_REQUIRED) return 'confirm';
    throw new RelocateRefused(relocateMessage(begun.error));
  }
  const summary = await lifecycleOutcome(begun.value.operation_id);
  if (summary.state === 'completed') return 'moved';
  if (summary.state === 'blocked') {
    await withdrawLifecycle(summary);
    throw new RelocateRefused(BLOCKED);
  }
  throw new RelocateRefused(NOT_FINISHED);
}
