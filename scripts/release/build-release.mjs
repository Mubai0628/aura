// Builds the artifacts of one release from outputs that are already built (release binaries in
// target/release, the desktop package in target/electron-builder-package, the Chrome extension
// built for the release's namespace in apps/chrome-extension/dist, the pinned PowerShell in
// target/managed-runtimes):
//
//   node scripts/release/build-release.mjs --version 0.1.0 [--namespace production|test:<run>]
//       [--channel Stable] [--out target/release-artifacts/<version>] [--no-sign]
//
// It assembles the release folder, signs its manifest (a production release always; the signing
// key passphrase is asked for), packs the setup program (the release's own setup program with the
// payload appended) and the update package, and writes SHA256SUMS.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { assemble } from './assemble.mjs';
import { sha256File } from './layout.mjs';
import { signManifest } from './sign-manifest.mjs';

const REPOSITORY = resolve(import.meta.dirname, '../..');
const PRODUCTION_ROOT = join(
  REPOSITORY,
  'resources/release-trust/production/aura-production-release-root-v1.der',
);
// The programs that check a production release's signature against the root built into them.
const VERIFYING = ['aura-setup.exe', 'aura-core.exe', 'aura-elevated-helper.exe'];

/** Refuses release programs built without the production root (AURA_PRODUCTION_RELEASE_BUILD=1). */
export function requireProductionTrust(binaries, root = readFileSync(PRODUCTION_ROOT)) {
  for (const name of VERIFYING) {
    if (!readFileSync(join(binaries, name)).includes(root)) {
      throw new Error(
        `${name} carries no production release root: build the release programs with AURA_PRODUCTION_RELEASE_BUILD=1`,
      );
    }
  }
}

function parse(argv) {
  const options = { namespace: 'production', channel: 'Stable', sign: true };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === '--no-sign') {
      options.sign = false;
      continue;
    }
    const value = argv[index + 1];
    const name = flag?.replace(/^--/u, '');
    assert.ok(
      ['version', 'namespace', 'channel', 'out', 'test-keys'].includes(name) && value,
      `unknown or incomplete option: ${flag}`,
    );
    options[name === 'test-keys' ? 'testKeys' : name] = value;
    index += 1;
  }
  assert.ok(options.version, '--version is required');
  assert.ok(
    options.namespace !== 'production' || options.sign,
    'a production release is always signed',
  );
  options.out ??= join(
    REPOSITORY,
    'target/release-artifacts',
    options.namespace === 'production'
      ? options.version
      : `${options.namespace.replace(':', '-')}-${options.version}`,
  );
  return options;
}

export async function buildRelease(options) {
  if (options.namespace === 'production')
    requireProductionTrust(join(REPOSITORY, 'target/release'));
  const out = resolve(options.out);
  const { payload, version, record } = await assemble({
    version: options.version,
    channel: options.channel,
    namespace: options.namespace,
    out,
    desktop: join(REPOSITORY, 'target/electron-builder-package/win-unpacked'),
    binaries: join(REPOSITORY, 'target/release'),
    runtime: join(REPOSITORY, 'target/managed-runtimes/powershell-7.6.5-win-x64'),
  });
  if (options.sign) {
    await signManifest({
      folder: version,
      ...(options.testKeys ? { testDir: resolve(options.testKeys) } : {}),
    });
  }
  const stub = join(version, 'aura-setup.exe');
  const setup = join(out, `Aura-Setup-${options.version}-x64.exe`);
  const update = join(out, `Aura-Update-${options.version}-x64.aura-update`);
  const packed = JSON.parse(
    execFileSync(
      join(REPOSITORY, 'target/release/aura-release-pack.exe'),
      [payload, stub, setup, update],
      {
        encoding: 'utf8',
      },
    ),
  );
  const sums = [];
  for (const file of [setup, update]) sums.push(`${await sha256File(file)}  ${basename(file)}`);
  writeFileSync(join(out, 'SHA256SUMS'), `${sums.join('\n')}\n`);
  return { out, record, packed, signed: options.sign };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  buildRelease(parse(process.argv.slice(2)))
    .then((result) => console.log(JSON.stringify(result, null, 2)))
    .catch((error) => {
      console.error(error.message);
      process.exit(1);
    });
}
