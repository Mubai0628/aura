// Copies the Electron Main runtime modules `tsc` does not emit.
//
// `tsc` emits `.js` for `.ts` and copies nothing else into `outDir`. The binding
// rules are `.mjs` on purpose: the same files are loaded by Main, by the ordinary
// `corepack pnpm dev` preflight and by the desktop tests, and none of those should
// need a build step or a second copy of the rules to read them. Without this step
// the compiled `published-core.js` resolves `./core-binding.mjs` to a file that was
// never emitted, so the packaged application fails at startup.
//
// This runs after `tsc -b` and before the Preload bundle, so the emitted directory
// is complete before anything consumes it.
import { copyFile, mkdir, readdir } from 'node:fs/promises';
import { basename, join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const source = resolve(root, 'apps/desktop/electron-main');
const destination = resolve(root, 'apps/desktop/dist/electron-main');

const runtimeModules = (await readdir(source)).filter((name) => name.endsWith('.mjs'));
if (runtimeModules.length === 0) {
  throw new Error('electron-main has no runtime .mjs module to emit');
}

await mkdir(destination, { recursive: true });
for (const name of runtimeModules) {
  // `basename` is redundant with a directory listing, and it is kept so a future
  // change that walks subdirectories cannot silently write outside the target.
  await copyFile(join(source, name), join(destination, basename(name)));
}
console.log(`emitted electron-main runtime modules: ${runtimeModules.sort().join(', ')}`);
