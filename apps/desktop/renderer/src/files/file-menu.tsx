import type { WorkspaceFileOpener } from '@aura/contracts/generated';
import { runsCode } from '../../../shared/outside-files';
import { userMessage } from '../agent-session/user-message';
import { openInBrowser } from '../browser/browser-tabs';
import { copyText } from '../clipboard';
import { toolTabs } from '../right-workspace/tool-tabs';
import { ToolIcon } from '../tool-icon';
import type { MenuEntry } from '../ui/ContextMenu';
import { filesNamed, inWorkspace, isBareName } from './file-locate';
import { sidebarShows } from './file-paths';
import { fileSource } from './file-sources';
import { fileTrees, type FileScope } from './file-tree-store';

/** A conversation's workspace files, and where a problem with one is said. */
export interface FileActionScope extends FileScope {
  notify: (problem: string) => void;
}

/** The file on disk: the workspace root and its path, in Windows' spelling. */
export function absolutePath(root: string, path: string): string {
  return `${root.replace(/[\\/]+$/u, '')}\\${path.replaceAll('/', '\\')}`;
}

/** Asks Main to open the file outside Aura; a refusal or a failure is said. */
async function openOutside(
  scope: FileActionScope,
  path: string,
  opener: WorkspaceFileOpener,
): Promise<void> {
  try {
    const result = await window.aura.v1.openWorkspaceFileOutside({
      workspace_id: scope.workspace,
      session_id: scope.session,
      path,
      opener,
    });
    if (!result.ok) scope.notify(userMessage(result.error, '没有打开这个文件，请重试。'));
    else if (!result.value.completed) scope.notify(result.value.message);
  } catch (error) {
    scope.notify(userMessage(error, '没有打开这个文件，请重试。'));
  }
}

/**
 * A click opens what the sidebar shows (text, Markdown,
 * pictures) in a 文件 tab; a program or a script, which Aura never runs, is shown in the file
 * manager; anything else opens with its default application. A bare name not at the root
 * opens the workspace's only file of that name; with several, its tab lists them.
 */
export function openFile(scope: FileActionScope, path: string, line?: number): void {
  void located(scope, path).then((found) => openAt(scope, found, line));
}

/** The file a link names, as a 文件 tab entry: the link itself when nothing else fits. */
async function located(scope: FileActionScope, path: string): Promise<string> {
  if (isBareName(path)) {
    const found = await filesNamed(scope, path);
    if (found.length === 1) return found[0]!;
    if (found.length) return path;
    // A file the user attached from outside the workspace, named by its name alone.
    const attached = scope.attachmentNamed?.(path);
    if (attached !== undefined) return attached;
    return (await scope.zipEntryNamed?.(path)) ?? path;
  }
  // A file inside a zip a message carried,
  // unless the workspace has a file at that path.
  const zipped = await scope.zipEntryNamed?.(path);
  if (zipped === undefined) return path;
  return (await inWorkspace(scope, path)) ? path : zipped;
}

function openAt(scope: FileActionScope, path: string, line?: number): void {
  // What a message carried is always shown in the sidebar, whatever its kind.
  if (sidebarShows(path) || fileSource(path).kind !== 'workspace')
    toolTabs.revealFile(scope.session, path, line);
  else void openOutside(scope, path, runsCode(path) ? 'file_manager' : 'default_application');
}

async function copy(scope: FileActionScope, text: string): Promise<void> {
  const refused = await copyText(text, '没能复制路径，请重试。');
  if (refused) scope.notify(refused);
}

/** A page the built-in browser opens. */
const WEB_PAGE = /\.(?:html?|xhtml)$/iu;

/**
 * What can be done with a workspace file from its right-click menu. Where the
 * menu is decides the first items: a path in the conversation opens in the sidebar, a 文件 tab's
 * file shows in its tree, and a row of the tree opens.
 */
export function fileMenuEntries(
  scope: FileActionScope,
  path: string,
  where: 'conversation' | 'tab' | 'tree',
): MenuEntry[] {
  const icon = (
    name: 'panel-right-open' | 'folder-tree' | 'copy' | 'external-link' | 'folder-open' | 'globe',
  ) => <ToolIcon name={name} size={14} />;
  const entries: MenuEntry[] = [];
  if (where !== 'tab' && sidebarShows(path))
    entries.push({
      label: '在侧边栏打开',
      icon: icon('panel-right-open'),
      // From the conversation, a bare name or a zip's entry is found as a click finds it.
      run: () =>
        where === 'conversation' ? openFile(scope, path) : toolTabs.revealFile(scope.session, path),
    });
  if (WEB_PAGE.test(path))
    entries.push({
      label: '在内置浏览器打开',
      icon: icon('globe'),
      run: () =>
        void openInBrowser(scope.session, { workspaceId: scope.workspace, path }).then(
          (refused) => refused && scope.notify(refused),
        ),
    });
  const program = runsCode(path);
  entries.push(
    {
      label: '用默认应用打开',
      icon: icon('external-link'),
      disabled: program,
      ...(program ? { note: 'Aura 不直接运行可执行文件或脚本' } : {}),
      run: () => void openOutside(scope, path, 'default_application'),
    },
    {
      label: '在文件资源管理器中显示',
      icon: icon('folder-open'),
      run: () => void openOutside(scope, path, 'file_manager'),
    },
  );
  if (where !== 'tree')
    entries.push({
      label: '在文件树中显示',
      icon: icon('folder-tree'),
      run: () => {
        if (where === 'conversation') toolTabs.revealFile(scope.session, path);
        fileTrees.locate(scope, path);
      },
    });
  entries.push(
    'separator',
    {
      label: '复制路径',
      icon: icon('copy'),
      run: () => void copy(scope, absolutePath(scope.root, path)),
    },
    { label: '复制相对路径', icon: icon('copy'), run: () => void copy(scope, path) },
  );
  return entries;
}
