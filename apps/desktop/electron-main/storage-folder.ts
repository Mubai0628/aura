import { spawn } from 'node:child_process';
import { stat } from 'node:fs/promises';
import { join } from 'node:path';
import { shell, type BrowserWindow } from 'electron';
import type { AgentMainParams, AgentMainResult } from '../shared/generated/routes.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { isDirectory, ordinaryPath, selectFolder } from './folder-dialog.js';

type Selected = AgentMainResult['storage.select_folder'];
type OpenRequest = AgentMainParams['storage.open_folder'];
type Opened = AgentMainResult['storage.open_folder'];
type Restart = AgentMainResult['storage.restart'];

/**
 * Settings → 存储: Main's own folder dialog for where the data goes. Core checks the folder
 * before anything is written and again before the move starts.
 */
export function selectStorageFolder(owner: BrowserWindow): Promise<Selected> {
  return selectFolder(owner, '选择数据要放的文件夹');
}

/** Opens the data's or the installation's folder as Core's overview names it. */
export async function openStorageFolder(
  request: OpenRequest,
  supervisor: CoreSupervisor,
): Promise<Opened> {
  const overview = await supervisor.requestAgent('storage.overview', {});
  const place = request.part === 'data' ? overview.data : overview.install;
  if (!place) return { completed: false, message: '开发版没有安装文件夹。' };
  const folder = ordinaryPath(place.path);
  if (!(await isDirectory(folder))) return { completed: false, message: '这个文件夹已不存在。' };
  const failure = await shell.openPath(folder);
  return failure
    ? { completed: false, message: '无法打开这个文件夹，请检查它是否仍存在与文件权限。' }
    : { completed: true, message: '已打开' };
}

/**
 * Closes Aura so the launcher starts it again and does the move first. A development run has no
 * launcher: it stays open, and the move waits for its next start.
 */
export function restartForStorage(restart: (() => boolean) | undefined): Restart {
  return { restarting: restart?.() ?? false };
}

/** The installed setup program, which moves the installation it sits in. */
export const INSTALLED_SETUP = 'Uninstall Aura.exe';

/** The setup program did not start; Aura stays open and nothing moved. */
export const INSTALL_NOT_STARTED =
  '安装程序没能启动（可能被 Windows 或安全软件拦截），安装位置没有移动，Aura 保持打开；请稍后再试，或手动运行安装目录里的 Uninstall Aura.exe。';

/**
 * Opens the installed setup program to move the installation or its data, then closes Aura, whose
 * folder is what moves; the program starts Aura again when it is done or given up. A development
 * run has no installation and stays open.
 */
export async function moveInstallation(
  supervisor: CoreSupervisor,
  open: (program: string, args: string[]) => boolean,
  quit: () => void,
): Promise<Restart> {
  const overview = await supervisor.requestAgent('storage.overview', {});
  if (!overview.install) return { restarting: false };
  const program = join(ordinaryPath(overview.install.path), INSTALLED_SETUP);
  if (!(await isFile(program))) return { restarting: false };
  if (!open(program, ['--relocate', '--from-aura'])) throw new Error(INSTALL_NOT_STARTED);
  quit();
  return { restarting: true };
}

const isFile = (path: string): Promise<boolean> =>
  stat(path).then(
    (found) => found.isFile(),
    () => false,
  );

/** Starts `program` on its own, so it outlives Aura; whether it started. */
export function openDetached(program: string, args: string[]): boolean {
  const child = spawn(program, args, { detached: true, stdio: 'ignore' });
  // A program that could not start has no process id; its error event then needs a listener.
  child.on('error', () => undefined);
  child.unref();
  return child.pid !== undefined;
}
