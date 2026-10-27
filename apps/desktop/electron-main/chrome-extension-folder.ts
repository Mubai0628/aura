/**
 * The Chrome extension's folder, which the user loads in Chrome (developer mode, "Load
 * unpacked"). Chrome keeps loading the folder it was given, and every update puts Aura in a new
 * version folder, so the user loads a copy in Aura's data folder that each start brings up to
 * date with the running version; Chrome takes the new files at its next start.
 */
import { app, shell } from 'electron';
import { createHash } from 'node:crypto';
import { cp, mkdir, readdir, readFile, rename, rm } from 'node:fs/promises';
import { dirname, join, relative, resolve } from 'node:path';
import { isDirectory } from './folder-dialog.js';

/**
 * The extension this Aura ships: beside its resources, or this checkout's test build (a
 * development namespace pairs with the test extension).
 */
export function bundledChromeExtension(): string {
  return app.isPackaged
    ? join(process.resourcesPath, 'chrome-extension')
    : resolve(app.getAppPath(), '..', 'chrome-extension', 'dist', 'test');
}

/** The copy Chrome loads, in the installation's data folder. */
export const chromeExtensionCopy = (dataRoot: string): string =>
  join(dataRoot, 'chrome', 'extension');

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
  dataRoot: string,
  log: (event: string, fields: Record<string, unknown>) => Promise<void> | void,
): Promise<void> {
  const copy = chromeExtensionCopy(dataRoot);
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

/**
 * Opens the copy in the file manager, made or brought up to date first; the path is given either
 * way. A copy that cannot be updated is still opened as it is.
 */
export async function openChromeExtensionFolder(
  dataRoot: string | undefined,
): Promise<{ path: string; opened: boolean }> {
  if (dataRoot === undefined) return { path: bundledChromeExtension(), opened: false };
  const path = chromeExtensionCopy(dataRoot);
  try {
    await refreshChromeExtensionCopy(bundledChromeExtension(), path);
  } catch {
    // The copy as it is, when there is one.
  }
  if (!(await isDirectory(path))) return { path, opened: false };
  return { path, opened: (await shell.openPath(path)) === '' };
}
