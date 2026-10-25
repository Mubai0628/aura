// OpenSSL for the release key and manifest signature steps. Git for Windows carries one, so a
// machine that can build Aura already has it; AURA_OPENSSL names another explicitly.
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { delimiter, join } from 'node:path';

const GIT_LOCATIONS = ['usr/bin/openssl.exe', 'mingw64/bin/openssl.exe'];

function candidates(env) {
  const found = [];
  if (env.AURA_OPENSSL) found.push(env.AURA_OPENSSL);
  for (const entry of (env.PATH ?? env.Path ?? '').split(delimiter)) {
    if (entry) found.push(join(entry, 'openssl.exe'));
  }
  for (const base of [
    env.ProgramFiles,
    env['ProgramFiles(x86)'],
    env.LOCALAPPDATA && join(env.LOCALAPPDATA, 'Programs'),
  ]) {
    if (!base) continue;
    for (const relative of GIT_LOCATIONS) found.push(join(base, 'Git', relative));
  }
  return found;
}

/** The OpenSSL program to run, version 3 or later. */
export function findOpenssl(env = process.env) {
  for (const path of candidates(env)) {
    if (!existsSync(path)) continue;
    const version = spawnSync(path, ['version'], { encoding: 'utf8' });
    const major = /^OpenSSL (\d+)\./u.exec(version.stdout ?? '')?.[1];
    if (version.status === 0 && Number(major) >= 3) return path;
  }
  throw new Error('OpenSSL 3 was not found: install Git for Windows or set AURA_OPENSSL');
}

/**
 * Runs OpenSSL with `args`. Passphrases travel only in `secrets`, which become environment
 * variables of this one child process and are read by `-pass env:NAME`.
 */
export function openssl(program, args, { secrets = {}, input } = {}) {
  const result = spawnSync(program, args, {
    encoding: 'utf8',
    env: { ...process.env, ...secrets },
    input,
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || '')
      .trim()
      .split(/\r?\n/u)
      .slice(-3)
      .join(' ');
    throw new Error(`openssl ${args[0]} failed (${result.status}): ${detail}`);
  }
  return result.stdout;
}
