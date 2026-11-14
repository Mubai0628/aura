import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { win32 } from 'node:path';
import type { BrowserWindow } from 'electron';

import { notificationIdentity } from './notifications/identity.js';

const LAUNCHER = 'AuraLauncher.exe';
const ICON = 'Aura.ico';
/** Every start through the launcher carries it. */
const FROM_LAUNCHER = '--aura-parent-runtime-id=';

/**
 * The launcher of the installation this Aura.exe runs from (`<root>\versions\<version>\Aura.exe`
 * beside `<root>\AuraLauncher.exe`); none for a development run or an unpacked build.
 */
export function installedLauncher(
  executablePath: string,
  packaged: boolean,
  exists: (path: string) => boolean = existsSync,
): string | undefined {
  if (!packaged) return undefined;
  const versions = win32.dirname(win32.dirname(executablePath));
  if (win32.basename(versions).toLowerCase() !== 'versions') return undefined;
  const launcher = win32.join(win32.dirname(versions), LAUNCHER);
  return exists(launcher) ? launcher : undefined;
}

/** Starts a program on its own, outliving this run; its process id, none when it did not start. */
export type StartDetached = (
  path: string,
  args: readonly string[],
  cwd: string,
) => number | undefined;

const startDetached: StartDetached = (path, args, cwd) => {
  const child = spawn(path, args, { cwd, detached: true, stdio: 'ignore' });
  // A program that could not start has no process id; its error event then needs a listener.
  child.on('error', () => undefined);
  child.unref();
  return child.pid;
};

/**
 * An installed Aura.exe started without the launcher (from a shortcut Windows made from its
 * window, say) would skip what the launcher does first: the version choice, an update's trial, a
 * data move and the copy kept before an upgrade. It starts the launcher in its place and leaves;
 * the launcher starts Aura again. Returns when it was started by the launcher, or when the launcher
 * could not be started, and Aura then starts as it is.
 */
export function startThroughLauncher(
  argv: readonly string[],
  launcher: string | undefined,
  start: StartDetached = startDetached,
  exit: (code: number) => void = (code) => process.exit(code),
): void {
  if (launcher === undefined || argv.some((argument) => argument.startsWith(FROM_LAUNCHER))) return;
  const args = argv.includes('--safe-mode') ? ['--safe-mode'] : [];
  if (start(launcher, args, win32.dirname(launcher)) === undefined) return;
  exit(0);
}

/**
 * A pin Windows makes from the window starts the launcher, not this version's Aura.exe, which
 * skips the launcher and goes with the next update.
 */
export function pinThroughLauncher(
  window: Pick<BrowserWindow, 'setAppDetails'>,
  launcher: string | undefined,
  namespace: string | undefined,
  exists: (path: string) => boolean = existsSync,
): void {
  if (launcher === undefined || namespace === undefined) return;
  const icon = win32.join(win32.dirname(launcher), ICON);
  window.setAppDetails({
    appId: notificationIdentity(namespace),
    ...(exists(icon) ? { appIconPath: icon } : {}),
    relaunchCommand: `"${launcher}"`,
    relaunchDisplayName: 'Aura',
  });
}
