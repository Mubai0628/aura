// The Chrome extension a release carries: the build for the release's namespace (its key, so
// Chrome gives it the id that installation pairs with, and the relay host it reaches) at the
// release's version. The build itself comes first:
//
//   corepack pnpm --filter @aura/chrome-extension build                     released namespace
//   AURA_CHROME_TARGET=test AURA_CHROME_NAMESPACE=aura.desktop.test.<run> \
//     corepack pnpm --filter @aura/chrome-extension build                   a test namespace
import { createHash } from 'node:crypto';
import { readdir, readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { PRODUCTION_NAMESPACE } from './layout.mjs';

const EXTENSION = resolve(import.meta.dirname, '../../apps/chrome-extension');
const PRODUCTION_HOST = 'com.aura.chrome_control';

/** The build a namespace pairs with: the released extension, or the test one. */
export const extensionTarget = (namespace) =>
  namespace === PRODUCTION_NAMESPACE ? 'production' : 'test';

/** Where the extension's build for a namespace is written. */
export const extensionBuild = (namespace) => join(EXTENSION, 'dist', extensionTarget(namespace));

/** The native-messaging host name of a namespace, as aura-chrome-domain names it. */
export function nativeHostName(namespace) {
  if (namespace === PRODUCTION_NAMESPACE) return PRODUCTION_HOST;
  const digest = createHash('sha256').update(namespace, 'utf8').digest('hex');
  return `${PRODUCTION_HOST}.t${digest.slice(0, 12)}`;
}

/** The host names the built scripts connect to. */
async function hostNames(folder) {
  const found = new Set();
  for (const entry of await readdir(folder, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith('.js')) continue;
    const text = await readFile(join(entry.parentPath, entry.name), 'utf8');
    for (const match of text.matchAll(
      /["'`](com\.aura\.chrome_control(?:\.t[0-9a-f]{12})?)["'`]/gu,
    ))
      found.add(match[1]);
  }
  return [...found];
}

function buildCommand(namespace) {
  const build = 'corepack pnpm --filter @aura/chrome-extension build';
  return extensionTarget(namespace) === 'production'
    ? build
    : `AURA_CHROME_TARGET=test AURA_CHROME_NAMESPACE=${namespace} ${build}`;
}

/** Refuses a build that is not this release's extension; returns its manifest. */
export async function checkExtension(
  folder,
  { namespace, version, keys = join(EXTENSION, 'keys') },
) {
  const target = extensionTarget(namespace);
  const rebuild = `build it for this release: ${buildCommand(namespace)}`;
  let manifest;
  try {
    manifest = JSON.parse(await readFile(join(folder, 'manifest.json'), 'utf8'));
  } catch {
    throw new Error(`no Chrome extension build in ${folder}; ${rebuild}`);
  }
  const key = (await readFile(join(keys, `${target}.pub`), 'utf8')).trim();
  if (manifest.key !== key) {
    throw new Error(`the Chrome extension in ${folder} is not the ${target} build; ${rebuild}`);
  }
  if (manifest.version !== version) {
    throw new Error(
      `the Chrome extension in ${folder} is version ${manifest.version}, not ${version}; set apps/chrome-extension/package.json's version and ${rebuild}`,
    );
  }
  const hosts = await hostNames(folder);
  const expected = nativeHostName(namespace);
  if (hosts.length !== 1 || hosts[0] !== expected) {
    throw new Error(
      `the Chrome extension in ${folder} reaches ${hosts.join(', ') || 'no host'}, not ${expected}; ${rebuild}`,
    );
  }
  return manifest;
}
