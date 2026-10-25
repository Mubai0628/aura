/**
 * A workspace web page opens in a page of the
 * user's own. Main finds where the file really is, inside the conversation's workspace, before
 * the page may open it and the other files of that workspace.
 */
import { stat } from 'node:fs/promises';
import { sep } from 'node:path';
import { pathToFileURL } from 'node:url';

import type { BrowserCommandResult, BrowserUserCommand } from '../../shared/browser-api.js';
import type { CoreSupervisor } from '../core-supervisor.js';
import { locateWorkspaceFile } from '../open-outside.js';
import type { BrowserPages } from './pages.js';

const WEB_PAGE = /\.(?:html?|xhtml)$/iu;

type OpenFile = Extract<BrowserUserCommand, { kind: 'open-file' }>;

export async function openWorkspacePage(
  pages: Pick<BrowserPages, 'openFile' | 'state'>,
  core: Pick<CoreSupervisor, 'refreshWorkspaceSessionSnapshot'> | undefined,
  command: OpenFile,
): Promise<BrowserCommandResult> {
  const { sessionId, pageId } = command;
  if (!WEB_PAGE.test(command.path)) return { ok: false, message: '只能在内置浏览器中打开网页文件' };
  if (core === undefined) return { ok: false, message: 'Aura Core 尚未就绪，请稍后再试' };
  let root: string;
  let target: string;
  try {
    const located = await locateWorkspaceFile(
      { workspace_id: command.workspaceId, session_id: sessionId, path: command.path },
      core,
    );
    if ('refused' in located) return { ok: false, message: located.refused };
    ({ root, target } = located);
    // Where a link leads is what is opened: it must still be a web page, and a file.
    if (!WEB_PAGE.test(target) || !(await stat(target)).isFile())
      return { ok: false, message: '只能在内置浏览器中打开网页文件' };
  } catch {
    return { ok: false, message: '没有打开这个文件，请重试。' };
  }
  const folder = pathToFileURL(root.endsWith(sep) ? root : `${root}${sep}`).href;
  const refused = pages.openFile(sessionId, pageId, pathToFileURL(target).href, folder);
  return refused === undefined
    ? { ok: true, state: pages.state(sessionId, pageId) }
    : { ok: false, message: refused };
}
