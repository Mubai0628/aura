import { spawn as spawnProcess } from 'node:child_process';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';

/**
 * Does the data move the 存储 page asked for before the development window opens: a development
 * run has no launcher, so `aura-core --move-data` does what the launcher does for an
 * installation, and answers at once when nothing was asked. A move that fails leaves the data
 * where it was and records why for the 存储 page, so the start carries on whatever it answers.
 * Resolves with the command's exit code.
 */
export function moveRequestedData({ root, env, spawn = spawnProcess, print = console.log }) {
  const core = resolve(root, 'target/debug/aura-core.exe');
  const manifest = resolve(root, 'target/aura-dev-runtime/component-manifest.json');
  return new Promise((done) => {
    const child = spawn(core, ['--move-data', `--component-manifest=${manifest}`], {
      cwd: root,
      env,
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    for (const stream of [child.stdout, child.stderr]) {
      createInterface({ input: stream }).on('line', (line) => {
        if (!nothingAsked(line)) print(`[move] ${line}`);
      });
    }
    child.on('error', (error) => {
      print(`[move] ${error.message}`);
      done(1);
    });
    child.on('exit', (code) => done(code ?? 1));
  });
}

function nothingAsked(line) {
  try {
    return JSON.parse(line).outcome === 'NothingAsked';
  } catch {
    return false;
  }
}
