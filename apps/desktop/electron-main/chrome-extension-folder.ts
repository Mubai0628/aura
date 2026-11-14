/**
 * The Chrome extension's folder, which the user loads in Chrome (developer mode, "Load
 * unpacked"). Chrome keeps loading the folder it was given, and every update puts Aura in a new
 * version folder, so the user loads a copy in the installation's Chrome folder that each start
 * brings up to date with the running version; Chrome takes the new files at its next start. That
 * folder stays where it is when the data moves, so Chrome keeps finding the copy.
 */
import { app, shell } from 'electron';
import { createHash } from 'node:crypto';
import { cp, mkdir, readdir, readFile, rename, rm } from 'node:fs/promises';
import { dirname, join, relative, resolve } from 'node:path';
import { isDirectory } from './folder-dialog.js';
import { errorEnvelope } from './request-failure-envelopes.js';

/**
 * The extension this Aura ships: beside its resources, or this checkout's test build (a
 * development namespace pairs with the test extension).
 */
export function bundledChromeExtension(): string {
  return app.isPackaged
    ? join(process.resourcesPath, 'chrome-extension')
    : resolve(app.getAppPath(), '..', 'chrome-extension', 'dist', 'test');
}

/** The copy Chrome loads, in the installation's Chrome folder (`DesktopLocations.chromeRoot`). */
export const chromeExtensionCopy = (chromeRoot: string): string => join(chromeRoot, 'extension');

/** Every file's path in the folder and the hash of its bytes. */
async function contents(folder: string): Promise<Map<string, string>> {
  const files = new Map<string, string>();
  for (const entry of await readdir(folder, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const path = join(entry.parentPath, entry.name);
    files.set(
      relative(folder, path),
      createHash('sha256')
        .update(await readFile(path))
        .digest('hex'),
    );
  }
  return files;
}

const same = (left: Map<string, string>, right: Map<string, string>): boolean =>
  left.size === right.size && [...left].every(([path, hash]) => right.get(path) === hash);

export type ExtensionCopy = 'current' | 'updated' | 'missing';

/**
 * Makes the copy the same as the shipped extension, replacing it whole; `missing` when this Aura
 * has no extension to copy, which leaves the copy as it was.
 */
export async function refreshChromeExtensionCopy(
  source: string,
  copy: string,
): Promise<ExtensionCopy> {
  if (!(await isDirectory(source))) return 'missing';
  const present = (await isDirectory(copy)) ? await contents(copy) : null;
  if (present !== null && same(await contents(source), present)) return 'current';
  const next = `${copy}.next`;
  const old = `${copy}.old`;
  await rm(next, { recursive: true, force: true });
  await rm(old, { recursive: true, force: true });
  await mkdir(dirname(copy), { recursive: true });
  await cp(source, next, { recursive: true });
  if (present !== null) await rename(copy, old);
  await rename(next, copy);
  await rm(old, { recursive: true, force: true });
  return 'updated';
}

/** At start: a copy the user may have loaded follows this version; none is made unasked. */
export async function refreshLoadedChromeExtension(
  chromeRoot: string,
  log: (event: string, fields: Record<string, unknown>) => Promise<void> | void,
): Promise<void> {
  const copy = chromeExtensionCopy(chromeRoot);
  if (!(await isDirectory(copy))) return;
  try {
    const state = await refreshChromeExtensionCopy(bundledChromeExtension(), copy);
    if (state !== 'current') await log('chrome.extension.copy', { state });
  } catch (error) {
    await log('chrome.extension.copy_failed', {
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

/** Why a copy could not be made, by the system's code. */
function copyFailureCause(error: unknown): string {
  switch ((error as { code?: unknown } | null)?.code) {
    case 'ENOSPC':
      return '磁盘已满';
    case 'EPERM':
    case 'EACCES':
      return '没有写入权限';
    case 'EBUSY':
      return '文件正被其他程序使用，可能是 Chrome';
    default:
      return '磁盘已满、没有写入权限或文件正被 Chrome 使用';
  }
}

/**
 * Opens the copy in the file manager, made or brought up to date first; the path is given either
 * way. A copy that could not be made or brought up to date is not opened: an outdated one would
 * pair with this Aura badly.
 */
export async function openChromeExtensionFolder(
  chromeRoot: string | undefined,
): Promise<{ path: string; opened: boolean }> {
  if (chromeRoot === undefined) return { path: bundledChromeExtension(), opened: false };
  const path = chromeExtensionCopy(chromeRoot);
  try {
    await refreshChromeExtensionCopy(bundledChromeExtension(), path);
  } catch (error) {
    const message = `没能把 Chrome 扩展复制到 ${path}（${copyFailureCause(error)}），扩展文件夹没有准备好；关闭 Chrome 后再点一次“打开扩展文件夹”。`;
    throw Object.assign(new Error(message), {
      envelope: errorEnvelope('chrome_extension_copy_failed', message, 'failed'),
    });
  }
  if (!(await isDirectory(path))) return { path, opened: false };
  return { path, opened: (await shell.openPath(path)) === '' };
}
