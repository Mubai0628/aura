import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import type { AppUpdateNotice } from '../../shared/app-updates.js';
import { compareVersions } from './feed.js';

/** The installation this Aura runs from, and the setup program in it that changes it. */
export interface Installation {
  root: string;
  setup: string;
  dataRoot: string;
  current: string;
}

export const UNINSTALLER = 'Uninstall Aura.exe';

/** The installation, when this Aura runs from one that can update itself. */
export function installationOf(options: {
  appIsPackaged: boolean;
  versionDirectory: string;
  dataRoot: string;
  current: string;
}): Installation | null {
  if (!options.appIsPackaged) return null;
  const root = resolve(options.versionDirectory, '..', '..');
  const setup = join(root, UNINSTALLER);
  if (!existsSync(setup) || !existsSync(join(root, 'active.json'))) return null;
  return { root, setup, dataRoot: options.dataRoot, current: options.current };
}

export interface InstallState {
  active: string;
  previous: string | null;
  pending: string | null;
}

export async function readInstallState(root: string): Promise<InstallState> {
  const value = JSON.parse(await readFile(join(root, 'active.json'), 'utf8')) as Record<
    string,
    unknown
  >;
  const text = (field: unknown): string | null => (typeof field === 'string' ? field : null);
  return {
    active: text(value.active) ?? '',
    previous: text(value.previous),
    pending: text(value.pending),
  };
}

const FAILED_TRIAL = join('launcher', 'trial-failed.v1.json');
const RESTORE_REQUEST = join('launcher', 'restore.v1.json');

/**
 * Whether the launcher copied data before `version` first started: it copies only the stores a
 * newer version migrates, which the version before it can no longer read.
 */
export const copiedBefore = (dataRoot: string, version: string): boolean =>
  existsSync(join(dataRoot, 'backups', `before-${version}`));

/** The version the launcher last tried and put back, if the user has not dismissed it. */
export async function readNotice(
  dataRoot: string,
  active: string,
): Promise<AppUpdateNotice | null> {
  let value: Record<string, unknown>;
  try {
    value = JSON.parse(await readFile(join(dataRoot, FAILED_TRIAL), 'utf8')) as Record<
      string,
      unknown
    >;
  } catch {
    return null;
  }
  if (typeof value.version !== 'string' || typeof value.kept_version !== 'string') return null;
  const goingBack = compareVersions(value.version, value.kept_version) < 0;
  return {
    version: value.version,
    kept_version: value.kept_version,
    detail: typeof value.detail === 'string' ? value.detail : '',
    going_back: goingBack,
    data_restorable: goingBack && copiedBefore(dataRoot, active),
  };
}

export const dismissNotice = (dataRoot: string): Promise<void> =>
  rm(join(dataRoot, FAILED_TRIAL), { force: true });

/** Asks the launcher to put the data from before `version` first started back before the next start. */
export async function requestRestore(dataRoot: string, version: string): Promise<void> {
  const path = join(dataRoot, RESTORE_REQUEST);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, `${JSON.stringify({ before_version: version })}\n`, 'utf8');
}

/** Takes back a restore request whose rollback did not happen. */
export const withdrawRestore = (dataRoot: string): Promise<void> =>
  rm(join(dataRoot, RESTORE_REQUEST), { force: true });

export type RunProgram = (program: string, args: readonly string[]) => Promise<string>;

export const runProgram: RunProgram = (program, args) =>
  new Promise((done, fail) => {
    execFile(program, args, { windowsHide: true, timeout: 15 * 60_000 }, (error, stdout) => {
      // The setup program reports its outcome as one JSON line, also when it fails.
      if (stdout.trim()) done(stdout);
      else fail(error ?? new Error('the setup program said nothing'));
    });
  });

/** The setup program's refusal, and whether the package it was given can be installed as it is. */
export class SetupRefusal extends Error {
  constructor(
    message: string,
    readonly packageIntact: boolean,
  ) {
    super(message);
  }
}

/** Runs the installation's setup program and returns the version it lets wait, or throws its error. */
export async function runSetup(
  installation: Installation,
  args: readonly string[],
  run: RunProgram = runProgram,
): Promise<string> {
  const output = await run(installation.setup, args);
  const line = output.trim().split(/\r?\n/u).at(-1) ?? '';
  let value: Record<string, unknown>;
  try {
    value = JSON.parse(line) as Record<string, unknown>;
  } catch {
    throw new Error('安装程序没有给出结果');
  }
  if (typeof value.waiting === 'string') return value.waiting;
  throw new SetupRefusal(
    typeof value.error === 'string' ? value.error : '安装程序没有完成',
    value.package_intact === true,
  );
}
