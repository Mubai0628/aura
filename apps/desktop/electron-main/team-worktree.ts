import { shell } from 'electron';
import type {
  AgentCoreParams,
  AgentCoreResult,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { isDirectory, ordinaryPath } from './folder-dialog.js';

type OpenRequest = AgentMainParams['agent.team.worktree.open'];
type Opened = AgentMainResult['agent.team.worktree.open'];
type FolderRequest = AgentCoreParams['agent.team.worktree.folder'];
type Folder = AgentCoreResult['agent.team.worktree.folder'];

/** The longest a request from Main may wait; the folder is one read of the team's records. */
const BUDGET_MS = 15_000;

/**
 * A sub-agent's worktree in the file manager. The window names the conversation and the
 * sub-agent; Core names the folder, and only for a worktree still in use; only a folder opens.
 */
export async function openAgentWorktree(
  request: OpenRequest,
  supervisor: Pick<CoreSupervisor, 'requestWithBudget'>,
): Promise<Opened> {
  const asked: FolderRequest = { session_id: request.session_id, agent: request.agent };
  const found = (await supervisor.requestWithBudget(
    'agent.team.worktree.folder',
    asked,
    BUDGET_MS,
  )) as Folder;
  const folder = ordinaryPath(found.path);
  if (!(await isDirectory(folder)))
    return { completed: false, message: '它的工作副本文件夹已不存在。' };
  const failure = await shell.openPath(folder);
  return failure
    ? { completed: false, message: '无法打开它的工作副本文件夹，请检查它是否仍存在与文件权限。' }
    : { completed: true, message: '已打开' };
}
