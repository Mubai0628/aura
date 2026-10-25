// Shared launcher for the desktop recovery diagnostics.
//
// Every diagnostic must resolve the built Electron entry from the package root
// (not from this directory), verify the artifacts it is about to load, fail
// fast on a broken preflight, and always recycle the app it started.
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { _electron as electron } from '@playwright/test';

export const desktopRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// apps/desktop -> repository root
export const repoRoot = resolve(desktopRoot, '..', '..');

export class PreflightError extends Error {}

const digest = (path) => {
  const hash = createHash('sha256');
  hash.update(readFileSync(path));
  return hash.digest('hex').toUpperCase();
};

/**
 * Verify the artifacts a diagnostic depends on before it starts anything.
 * A missing or stale bundle must fail loudly rather than silently exercise an
 * older build.
 */
export const preflight = ({ core = true } = {}) => {
  const entry = join(desktopRoot, 'dist/e2e/electron-main/main.js');
  const preload = join(desktopRoot, 'dist/preload/index.cjs');
  const coreExe = join(repoRoot, 'target/debug/aura-core.exe');
  const required = [
    ['e2e entry', entry],
    ['preload', preload],
  ];
  if (core) required.push(['aura-core', coreExe]);
  const facts = {};
  for (const [label, path] of required) {
    if (!existsSync(path)) {
      throw new PreflightError(`${label} is missing at ${path}`);
    }
    const stats = statSync(path);
    facts[label] = {
      path,
      bytes: stats.size,
      modified: stats.mtime.toISOString(),
      sha256: digest(path),
    };
  }
  return facts;
};

export const launch = async ({ dataRoot, coreMode = 'normal', environment = {} } = {}) => {
  if (dataRoot === undefined) throw new PreflightError('dataRoot is required');
  mkdirSync(dataRoot, { recursive: true });
  const entry = join(desktopRoot, 'dist/e2e/electron-main/main.js');
  const application = await electron.launch({
    args: [`--user-data-dir=${join(dataRoot, 'Electron')}`, entry],
    env: {
      ...process.env,
      APPDATA: join(dataRoot, 'Roaming'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: coreMode,
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
      LOCALAPPDATA: dataRoot,
      ...environment,
    },
  });
  return application;
};

const stamp = () => new Date().toISOString().replaceAll(':', '').replaceAll('.', '');

/** Timestamped evidence directory; never overwrites an earlier run's files. */
export const evidenceDir = (name, runRoot) => {
  const dir = join(runRoot ?? join(repoRoot, 'target'), `${name}-${stamp()}`);
  mkdirSync(dir, { recursive: true });
  return dir;
};

export const writeJson = (dir, name, value) => {
  const path = join(dir, name);
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
  return path;
};

/**
 * Wrap a diagnostic so that: a preflight failure exits non-zero before any app
 * starts, a captured step failure still exits non-zero, and the application is
 * always recycled.
 */
export const runDiagnostic = async (name, body) => {
  const dir = evidenceDir(name);
  const lines = [];
  const log = (...parts) => {
    const line = `${new Date().toISOString()} ${parts.join(' ')}`;
    lines.push(line);
    console.log(line);
  };
  let application;
  let failure;
  try {
    const artifacts = preflight();
    log('preflight', JSON.stringify(artifacts));
    writeJson(dir, 'preflight.json', artifacts);
    application = await body({ dir, log, launch, writeJson });
  } catch (error) {
    failure = error;
    log('FAILED', error instanceof Error ? error.message : String(error));
  } finally {
    if (application !== undefined) {
      try {
        await application.close();
      } catch (error) {
        log('close-failed', String(error));
      }
    }
    writeFileSync(join(dir, 'diagnostic.log'), `${lines.join('\n')}\n`);
    log('evidence', dir);
  }
  if (failure !== undefined) process.exitCode = 1;
};

export { electron };
