import { realpath, stat } from 'node:fs/promises';
import { isAbsolute, join, relative } from 'node:path';
import { shell } from 'electron';
import type { AgentMainParams, AgentMainResult } from '../shared/generated/routes.js';
import { runsCode, workspaceRelativeParts } from '../shared/outside-files.js';
import type { CoreSupervisor } from './core-supervisor.js';

type FileRequest = AgentMainParams['agent.files.open_outside'];
type LinkRequest = AgentMainParams['agent.links.open'];
type Opened = AgentMainResult['agent.files.open_outside'];

const refused = (message: string): Opened => ({ completed: false, message });

/** Whether `target` is `root` or lies under it (Windows compares without case). */
const inside = (root: string, target: string) => {
  const path = relative(root, target);
  return path === '' || (!path.startsWith('..') && !isAbsolute(path));
};

type Located = { root: string; target: string } | { refused: string };

/**
 * Where a workspace file of the conversation really is. The Renderer names it by its workspace
 * path only: Main asks Core which workspace and conversation those are, follows links and
 * junctions to where the file really is, and finds nothing outside the workspace.
 */
export async function locateWorkspaceFile(
  request: { workspace_id: string; session_id: string; path: string },
  supervisor: Pick<CoreSupervisor, 'refreshWorkspaceSessionSnapshot'>,
): Promise<Located> {
  const parts = workspaceRelativeParts(request.path);
  if (!parts) return { refused: '这个路径不在工作区里。' };
  const snapshot = await supervisor.refreshWorkspaceSessionSnapshot();
  const workspace = snapshot.payload.workspaces.find(
    (candidate) => candidate.workspace_id === request.workspace_id,
  );
  const scoped = snapshot.payload.sessions.some(
    (session) =>
      session.session_id === request.session_id && session.workspace_id === request.workspace_id,
  );
  if (!workspace || !scoped) return { refused: '这个会话或工作区已不存在，请刷新后再试。' };
  let root: string;
  let target: string;
  try {
    root = await realpath(workspace.canonical_root);
    target = await realpath(join(root, ...parts));
  } catch {
    return { refused: '找不到这个文件，它可能已被移动或删除。' };
  }
  // A link or junction in the workspace may lead out of it: where it ends is what counts.
  if (!inside(root, target))
    return { refused: '这个文件在工作区外，Aura 不会打开工作区外的文件。' };
  return { root, target };
}

/**
 * A workspace file of the conversation opens with the application Windows
 * gives its kind, or is shown in the file manager; nothing outside the workspace is opened.
 * Programs and scripts are never opened; they can still be shown
 * in the file manager.
 */
export async function openWorkspaceFileOutside(
  request: FileRequest,
  supervisor: CoreSupervisor,
): Promise<Opened> {
  const located = await locateWorkspaceFile(request, supervisor);
  if ('refused' in located) return refused(located.refused);
  const { target } = located;
  if (request.opener === 'file_manager') {
    shell.showItemInFolder(target);
    return { completed: true, message: '已在文件资源管理器中显示' };
  }
  if ((await stat(target)).isFile() && runsCode(target))
    return refused('这是可执行文件或脚本，Aura 不会直接运行它；可以在文件资源管理器中查看。');
  const failure = await shell.openPath(target);
  return failure
    ? refused(`没有打开这个文件：${failure}`)
    : { completed: true, message: '已用默认应用打开' };
}

/**
 * A web page the conversation links to opens in the default browser — an
 * http or https address only, and never one that carries an account or a password, which is
 * how a link can pass for another site.
 */
export async function openExternalLink(request: LinkRequest): Promise<Opened> {
  let url: URL;
  try {
    url = new URL(request.url);
  } catch {
    return refused('这个链接无法识别。');
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:')
    return refused('只会打开 http 和 https 网页链接。');
  if (url.username || url.password)
    return refused('这个链接里带有账号或密码，Aura 不会打开它，请先核对链接。');
  try {
    await shell.openExternal(url.href);
  } catch {
    return refused('没有打开这个链接，请稍后重试。');
  }
  return { completed: true, message: '已在默认浏览器中打开' };
}
