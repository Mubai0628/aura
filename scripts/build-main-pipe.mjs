import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const desktopRoot = resolve(root, 'apps/desktop');
const addonRoot = resolve(desktopRoot, 'native/aura-main-pipe');
const requireFromDesktop = createRequire(resolve(desktopRoot, 'package.json'));
const nodeGyp = requireFromDesktop.resolve('node-gyp/bin/node-gyp.js');

if (process.platform !== 'win32' || process.arch !== 'x64') {
  throw new Error('aura-main-pipe.node is supported only on win32-x64');
}
if (Number(process.versions.napi) < 10) {
  throw new Error('aura-main-pipe.node requires stable Node-API 10');
}

const nodeRoot =
  process.env.AURA_NODE_API_SDK ??
  resolve(process.env.LOCALAPPDATA ?? '', 'node-gyp', 'Cache', process.versions.node);
const nodeHeader = resolve(nodeRoot, 'include/node/node_api.h');
const nodeLibrary = resolve(nodeRoot, 'x64/node.lib');
try {
  await Promise.all([readFile(nodeHeader), readFile(nodeLibrary)]);
} catch (error) {
  if (error?.code !== 'ENOENT') throw error;
  throw new Error(
    `Node's headers are not in ${nodeRoot}: run "corepack pnpm --filter @aura/desktop exec node-gyp install --ensure", or set AURA_NODE_API_SDK to a folder with include/node and x64/node.lib`,
    { cause: error },
  );
}

const result = spawnSync(
  process.execPath,
  [
    nodeGyp,
    'rebuild',
    '--directory',
    addonRoot,
    '--arch=x64',
    '--nodedir=' + nodeRoot,
    '--node_lib_file=' + nodeLibrary,
  ],
  { cwd: root, stdio: 'inherit' },
);
if (result.error) throw result.error;
if (result.status !== 0) {
  throw new Error('aura-main-pipe.node build failed with exit code ' + String(result.status));
}

const source = resolve(addonRoot, 'build/Release/aura_main_pipe.node');
const nativeOutputDirectory =
  process.env.AURA_E2E_BUILD === '1' ? 'dist/e2e-native' : 'dist/native';
const destination = resolve(desktopRoot, nativeOutputDirectory, 'aura-main-pipe.node');
await mkdir(resolve(desktopRoot, nativeOutputDirectory), { recursive: true });
const built = await readFile(source);
/**
 * Publishing is a content operation, so a destination that already holds these bytes is
 * already published.
 *
 * Copying anyway is not free: a running desktop has the addon loaded, and Windows refuses
 * the write while that handle exists — which turns "rebuild and publish the same artifact"
 * into a failure of the whole packaging entry. Comparing content first keeps every outcome
 * the same (a different artifact is still copied, a missing one is still created) and only
 * removes the write that would have changed nothing.
 */
const existing = await readFile(destination).catch(() => null);
if (existing !== null && existing.equals(built)) {
  console.log('aura-main-pipe.node already holds this build; published copy left in place');
} else {
  await copyFile(source, destination);
}
const digest = createHash('sha256')
  .update(await readFile(destination))
  .digest('hex');
console.log('built aura-main-pipe.node sha256=' + digest);
