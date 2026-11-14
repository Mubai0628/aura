import { describe, expect, test, vi } from 'vitest';

import {
  installedLauncher,
  pinThroughLauncher,
  startThroughLauncher,
} from './installed-launcher.js';

// A folder no machine has: nothing here may name a real installation, in case a start got through.
const ROOT = 'C:\\aura-installed-launcher-test-missing\\Aura';
const LAUNCHER = `${ROOT}\\AuraLauncher.exe`;
const INSTALLED = `${ROOT}\\versions\\0.3.0\\Aura.exe`;
const present =
  (...paths: string[]) =>
  (path: string) =>
    paths.includes(path);

/** A start that records what it was asked and starts nothing. */
function recording(pid: number | undefined) {
  const calls: unknown[][] = [];
  const exits: number[] = [];
  return {
    calls,
    exits,
    start: (...args: unknown[]) => {
      calls.push(args);
      return pid;
    },
    exit: (code: number) => {
      exits.push(code);
    },
  };
}

describe('installedLauncher', () => {
  test('an installed Aura.exe has the launcher of its installation', () => {
    expect(installedLauncher(INSTALLED, true, present(LAUNCHER))).toBe(LAUNCHER);
  });

  test('a development run, an unpacked build and a folder without the launcher have none', () => {
    expect(installedLauncher(INSTALLED, false, present(LAUNCHER))).toBeUndefined();
    const unpacked = `${ROOT}\\release\\win-unpacked\\Aura.exe`;
    expect(installedLauncher(unpacked, true, () => true)).toBeUndefined();
    expect(installedLauncher(INSTALLED, true, present())).toBeUndefined();
  });
});

describe('startThroughLauncher', () => {
  test('an installed Aura.exe started without the launcher starts it instead and leaves', () => {
    const run = recording(4242);
    startThroughLauncher([INSTALLED], LAUNCHER, run.start, run.exit);
    // Safe mode goes along, as the launcher passes it on.
    startThroughLauncher([INSTALLED, '--safe-mode'], LAUNCHER, run.start, run.exit);
    expect(run.calls).toEqual([
      [LAUNCHER, [], ROOT],
      [LAUNCHER, ['--safe-mode'], ROOT],
    ]);
    expect(run.exits).toEqual([0, 0]);
  });

  test('a start by the launcher, or one with no launcher to start, goes on as it is', () => {
    const run = recording(4242);
    const fromLauncher = '--aura-parent-runtime-id=run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    startThroughLauncher([INSTALLED, fromLauncher], LAUNCHER, run.start, run.exit);
    startThroughLauncher([INSTALLED], undefined, run.start, run.exit);
    expect(run.calls).toEqual([]);
    expect(run.exits).toEqual([]);
  });

  test('a launcher that cannot be started leaves Aura to start as it is', () => {
    const run = recording(undefined);
    startThroughLauncher([INSTALLED], LAUNCHER, run.start, run.exit);
    expect(run.calls).toHaveLength(1);
    expect(run.exits).toEqual([]);
  });
});

describe('pinThroughLauncher', () => {
  test("a pin of an installed Aura's window starts the launcher under the window's identity", () => {
    const window = { setAppDetails: vi.fn() };
    const icon = `${ROOT}\\Aura.ico`;
    pinThroughLauncher(window, LAUNCHER, 'aura.desktop.production', present(icon));
    expect(window.setAppDetails).toHaveBeenCalledWith({
      appId: 'Aura.aura.desktop.production',
      appIconPath: icon,
      relaunchCommand: `"${LAUNCHER}"`,
      relaunchDisplayName: 'Aura',
    });
  });

  test('a run that is not installed, or whose release is unknown, changes nothing', () => {
    const window = { setAppDetails: vi.fn() };
    pinThroughLauncher(window, undefined, 'aura.desktop.production');
    pinThroughLauncher(window, LAUNCHER, undefined);
    expect(window.setAppDetails).not.toHaveBeenCalled();
  });
});
