// The compiled Main must load from its own directory, not from the repository.
//
// `tsc` does not check that a relative import still resolves after emission, and a
// source-tree test cannot see the difference: an import back into the repository
// works in the checkout and fails the moment Main is compiled and packaged. These
// cases walk the real dependency closure of the emitted entry point and then load
// it from a directory that contains nothing but the emitted output, which is the
// property a standalone candidate package has.
//
// Requires `pnpm --filter @aura/desktop build:electron` (or at least
// `tsc -b` plus `scripts/copy-electron-runtime-modules.mjs`) to have run.
import assert from 'node:assert/strict';
import { cp, mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { test } from 'node:test';

const desktopRoot = resolve(import.meta.dirname, '../apps/desktop');
const emittedRoot = resolve(desktopRoot, 'dist/electron-main');
const base = resolve(import.meta.dirname, '../target/compiled-main-tests');

/** Relative and absolute specifiers a module asks for, in source order. */
async function importSpecifiers(file) {
  const source = await readFile(file, 'utf8');
  const found = new Set();
  for (const match of source.matchAll(
    /(?:^|[^\w.])(?:import|export)\s*(?:[^'"]*?\bfrom\s*)?['"]([^'"]+)['"]/gu,
  )) {
    found.add(match[1]);
  }
  for (const match of source.matchAll(/\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)/gu)) {
    found.add(match[1]);
  }
  return [...found];
}

const isLocal = (specifier) => specifier.startsWith('./') || specifier.startsWith('../');

/**
 * Every local file reachable from `entry`, with the modules that reach it.
 *
 * A specifier that does not resolve after one extension attempt is reported rather
 * than retried with a guess: Node's own loader is the authority, so the resolved
 * set must match what `import()` will ask for.
 */
async function dependencyClosure(entry) {
  const resolved = new Map();
  const pending = [entry];
  const unresolved = [];
  while (pending.length > 0) {
    const file = pending.pop();
    if (resolved.has(file)) continue;
    resolved.set(file, true);
    for (const specifier of await importSpecifiers(file)) {
      if (!isLocal(specifier)) continue;
      const target = resolve(dirname(file), specifier);
      try {
        const info = await stat(target);
        if (!info.isFile()) {
          unresolved.push({ file, specifier, reason: 'not a file' });
          continue;
        }
      } catch {
        unresolved.push({ file, specifier, reason: 'missing' });
        continue;
      }
      if (!resolved.has(target)) pending.push(target);
    }
  }
  return { resolved: [...resolved.keys()].sort(), unresolved };
}

/** The emitted desktop package, as a candidate package would carry it. */
async function standalonePackage(t) {
  await mkdir(base, { recursive: true });
  const root = await mkdtemp(join(base, 'case-'));
  t.after(() => {
    assert.ok(resolve(root).startsWith(base + sep));
    return rm(root, { recursive: true, force: true });
  });
  const target = join(root, 'package');
  await mkdir(target, { recursive: true });
  await cp(resolve(desktopRoot, 'dist'), join(target, 'dist'), { recursive: true });
  const pkg = JSON.parse(await readFile(resolve(desktopRoot, 'package.json'), 'utf8'));
  // The packaged application keeps the desktop package.json. It is copied rather
  // than linked so the layout has nothing pointing back into the repository.
  await writeFile(join(target, 'package.json'), JSON.stringify(pkg, null, 2));
  return { root, target };
}

test('the compiled entry point resolves its whole local dependency closure', async () => {
  const entry = join(emittedRoot, 'release.js');
  await stat(entry);
  const { resolved, unresolved } = await dependencyClosure(entry);
  assert.deepEqual(unresolved, [], 'every relative import in the emitted closure must exist');
  const names = resolved.map((file) => relative(emittedRoot, file).replace(sep, '/'));
  assert.ok(names.includes('release.js'));
  assert.ok(names.includes('published-core.js'));
  assert.ok(
    names.includes('core-binding.mjs'),
    `the emitted closure must include the binding rules: ${names.join(', ')}`,
  );
  assert.ok(
    resolved.every((file) => file.startsWith(emittedRoot + sep)),
    'no emitted module may reach outside its own directory',
  );
});

test('the compiled entry point loads from a directory holding only emitted output', async (t) => {
  const { target } = await standalonePackage(t);
  const url = pathToFileURL(join(target, 'dist/electron-main/release.js')).href;
  const loaded = await import(url);
  assert.equal(typeof loaded.loadReleaseContext, 'function');
});

test('the packaged file list carries the runtime modules tsc does not emit', async () => {
  const builder = await readFile(resolve(desktopRoot, 'electron-builder.yml'), 'utf8');
  // `asar: true` with `files: dist/**/*` ships whatever is in `dist`. The check that
  // matters is therefore that the emitted directory really contains the module,
  // which the closure case above proves; this asserts the packaging rule keeps
  // shipping `dist` wholesale rather than an allowlist that could omit it.
  assert.match(builder, /asar:\s*true/u);
  assert.match(builder, /- dist\/\*\*\/\*/u);
  const copied = await stat(join(emittedRoot, 'core-binding.mjs'));
  assert.ok(copied.isFile());
});

test('the candidate preparation emits the runtime modules in the same order', async () => {
  const script = await readFile(
    resolve(import.meta.dirname, 'execution/prepare-candidate.ps1'),
    'utf8',
  );
  // The candidate runs the build steps itself instead of `build:electron`, so it has to
  // run this one too. It packages `dist/**/*`, which means a stale or missing emitted
  // module is silently shipped: the repository copy resolves, the packaged one does not.
  let cursor = -1;
  for (const step of [
    'tsc -b tsconfig.electron.json --force',
    'copy-electron-runtime-modules.mjs',
    'vite build --config vite.preload.config.ts',
  ]) {
    const at = script.indexOf(step);
    assert.ok(at > cursor, `${step} must run after the step before it`);
    cursor = at;
  }
});
