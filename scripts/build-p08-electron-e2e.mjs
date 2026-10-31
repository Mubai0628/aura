import { spawnSync } from 'node:child_process';
import { rm } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const desktopRoot = resolve(root, 'apps/desktop');
const output = resolve(desktopRoot, 'dist/e2e');

await rm(output, { force: true, recursive: true });
if (process.env.AURA_E2E_BUILD !== '1') process.exit(0);

const result = spawnSync(
  process.execPath,
  [resolve(desktopRoot, 'node_modules/typescript/bin/tsc'), '--project', 'tsconfig.e2e.json'],
  { cwd: desktopRoot, env: process.env, stdio: 'inherit' },
);
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
