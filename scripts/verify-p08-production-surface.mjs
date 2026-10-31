import { readFile, readdir, stat } from 'node:fs/promises';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const mode = process.argv[2];
const forbidden = [
  // The E2E Core reads its loopback DNS and TLS fixture from this variable.
  'AURA_L01_NETWORK_FIXTURE',
  // The E2E Core starts user terminals without the owner's PowerShell profile.
  'AURA_TERMINAL_TEST_NO_PROFILE',
  // B0: the E2E-only gate holds one opening before its native launch.
  'AURA_TERMINAL_TEST_OPENING_GATE',
  'AURA_GIT_TEST_PROFILE',
  'AURA_MEMORY_TEST_ROOT',
];

const assertClean = async (path) => {
  const bytes = await readFile(path);
  for (const needle of forbidden) {
    if (bytes.includes(Buffer.from(needle, 'utf8'))) {
      throw new Error(`production surface contains the E2E-only marker ${needle}: ${path}`);
    }
  }
};

if (mode === 'core') {
  const name = process.platform === 'win32' ? 'aura-core.exe' : 'aura-core';
  await assertClean(resolve(root, 'target/debug', name));
  // Packaging and the development install copy target/release. A release Core built with a test
  // feature belongs in its own target directory, never here.
  const release = resolve(root, 'target/release', name);
  const releaseExists = await stat(release).then(
    (entry) => entry.isFile(),
    () => false,
  );
  if (releaseExists) await assertClean(release);
} else if (mode === 'desktop') {
  const productionRoots = [
    resolve(root, 'apps/desktop/dist/electron-main'),
    resolve(root, 'apps/desktop/dist/preload'),
  ];
  for (const productionRoot of productionRoots) {
    const pending = [productionRoot];
    while (pending.length > 0) {
      const directory = pending.pop();
      for (const entry of await readdir(directory, { withFileTypes: true })) {
        const path = resolve(directory, entry.name);
        if (entry.isDirectory()) pending.push(path);
        else if (entry.isFile()) await assertClean(path);
      }
    }
  }
  const packageConfiguration = await readFile(
    resolve(root, 'apps/desktop/electron-builder.yml'),
    'utf8',
  );
  if (!packageConfiguration.includes("'!dist/e2e/**/*'")) {
    throw new Error('production packaging does not exclude the E2E-only output');
  }
} else {
  throw new Error('usage: verify-p08-production-surface.mjs <core|desktop>');
}
