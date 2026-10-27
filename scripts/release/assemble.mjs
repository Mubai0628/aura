// Assembles one release folder from build outputs:
//
//   node scripts/release/assemble.mjs --version 0.1.0 --out <folder>
//       [--channel Stable] [--namespace production | --namespace test:<run id>]
//       [--desktop target/electron-builder-package/win-unpacked] [--binaries target/release]
//       [--runtime target/managed-runtimes/<runtime id>] [--commit <sha>]
//       [--extension apps/chrome-extension/dist/<production | test>]
//
// <folder>/payload is what an installer or update carries: Aura's icon and versions/<version>,
// which holds the launcher and the setup program the installation copies to its root, the Chrome
// extension built for the release's namespace, the license and the third-party notices. The
// manifest is written unsigned; scripts/release/sign-manifest.mjs signs it.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { copyFile, cp, mkdir, readdir, readFile, stat, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { ICON, brand } from './brand.mjs';
import { checkExtension, extensionBuild } from './chrome-extension.mjs';
import {
  BRANDED,
  CHANNELS,
  CHROME_EXTENSION_PATH,
  COMPONENTS,
  MANIFEST_VERSION,
  PRODUCTION_NAMESPACE,
  PROTOCOL_VERSION,
  TARGET_TRIPLE,
  TEST_NAMESPACE_PREFIX,
  VERSION_PATTERN,
  releaseBundleId,
  sha256,
  sha256File,
} from './layout.mjs';
import { releaseNotices } from './notices.mjs';

const REPOSITORY = resolve(import.meta.dirname, '../..');
const RUNTIME_PIN = JSON.parse(
  await readFile(join(REPOSITORY, 'runtimes/powershell.json'), 'utf8'),
);

function parse(argv) {
  const options = {
    channel: 'Stable',
    namespace: 'production',
    desktop: join(REPOSITORY, 'target/electron-builder-package/win-unpacked'),
    binaries: join(REPOSITORY, 'target/release'),
    runtime: join(REPOSITORY, 'target/managed-runtimes', RUNTIME_PIN.id),
  };
  for (let index = 0; index < argv.length; index += 2) {
    const [flag, value] = [argv[index], argv[index + 1]];
    const name = flag?.replace(/^--/u, '');
    if (
      ![
        'version',
        'out',
        'channel',
        'namespace',
        'desktop',
        'binaries',
        'runtime',
        'commit',
        'extension',
      ].includes(name) ||
      value === undefined
    ) {
      throw new Error(`unknown or incomplete option: ${flag}`);
    }
    options[name] = value;
  }
  assert.ok(
    options.version && VERSION_PATTERN.test(options.version),
    '--version x.y.z is required',
  );
  assert.ok(options.out, '--out <folder> is required');
  assert.ok(CHANNELS.includes(options.channel), `--channel must be one of ${CHANNELS.join(', ')}`);
  return options;
}

function namespaceOf(option) {
  if (option === 'production') return PRODUCTION_NAMESPACE;
  const run = /^test:([a-z0-9-]+)$/u.exec(option)?.[1];
  assert.ok(run, '--namespace is production or test:<run id>');
  return `${TEST_NAMESPACE_PREFIX}${run}`;
}

function buildCommit(option) {
  if (option) return option;
  try {
    return execFileSync('git', ['rev-parse', 'HEAD'], { cwd: REPOSITORY, encoding: 'utf8' }).trim();
  } catch {
    return '0'.repeat(40);
  }
}

/** The pinned runtime's own file list, checked against the pin before anything is copied. */
async function runtimeFiles(runtime) {
  const manifest = JSON.parse(await readFile(join(runtime, 'manifest.json'), 'utf8'));
  assert.equal(manifest.schema, 'r06-runtime-files-v1', 'runtime manifest schema');
  assert.equal(manifest.id, RUNTIME_PIN.id, 'runtime id');
  assert.equal(manifest.archive_sha256, RUNTIME_PIN.archive_sha256, 'runtime archive');
  const lines = manifest.files.map(
    (file) => `${file.path}|${file.bytes}|${file.sha256.toLowerCase()}`,
  );
  lines.sort();
  assert.equal(
    sha256(Buffer.from(lines.join('\n'), 'utf8')),
    RUNTIME_PIN.files_sha256,
    'runtime file list',
  );
  return manifest.files;
}

async function verifyFiles(root, files) {
  for (const file of files) {
    const path = join(root, file.path);
    assert.equal((await stat(path)).size, file.bytes, `runtime file size: ${file.path}`);
    assert.equal(await sha256File(path), file.sha256.toLowerCase(), `runtime file: ${file.path}`);
  }
}

async function countFiles(root) {
  let files = 0;
  let bytes = 0;
  for (const entry of await readdir(root, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue;
    files += 1;
    bytes += (await stat(join(entry.parentPath, entry.name))).size;
  }
  return { files, bytes };
}

export async function assemble(options) {
  const out = resolve(options.out);
  assert.ok(!existsSync(out), `the release folder already exists: ${out}`);
  const namespace = namespaceOf(options.namespace);
  const sources = { desktop: resolve(options.desktop), binaries: resolve(options.binaries) };
  const hashes = new Map();
  for (const component of COMPONENTS) {
    hashes.set(component.id, await sha256File(join(sources[component.from], component.path)));
  }
  const files = await runtimeFiles(resolve(options.runtime));
  await verifyFiles(join(resolve(options.runtime), 'payload'), files);
  const extension = resolve(options.extension ?? extensionBuild(namespace));
  await checkExtension(extension, { namespace, version: options.version });

  const payload = join(out, 'payload');
  const version = join(payload, 'versions', options.version);
  await mkdir(join(payload, 'versions'), { recursive: true });
  await cp(sources.desktop, version, { recursive: true, errorOnExist: true, force: false });
  for (const component of COMPONENTS.filter((entry) => entry.from === 'binaries')) {
    await cp(join(sources.binaries, component.path), join(version, component.path), {
      errorOnExist: true,
      force: false,
    });
  }
  await cp(extension, join(version, CHROME_EXTENSION_PATH), {
    recursive: true,
    errorOnExist: true,
    force: false,
  });
  await checkExtension(join(version, CHROME_EXTENSION_PATH), {
    namespace,
    version: options.version,
  });
  const runtime = join(version, 'managed-runtimes', RUNTIME_PIN.id);
  await cp(resolve(options.runtime), runtime, {
    recursive: true,
    errorOnExist: true,
    force: false,
  });
  await verifyFiles(join(runtime, 'payload'), files);
  await cp(ICON, join(payload, 'Aura.ico'), { errorOnExist: true });
  await copyFile(join(REPOSITORY, 'LICENSE'), join(version, 'LICENSE.txt'));
  await writeFile(join(version, 'THIRD-PARTY-NOTICES.txt'), releaseNotices().text, { flag: 'wx' });
  for (const component of COMPONENTS) {
    assert.equal(
      await sha256File(join(version, component.path)),
      hashes.get(component.id),
      `component changed while copied: ${component.id}`,
    );
  }
  // The copies get Aura's icon (and the desktop its version information); the manifest names the
  // bytes that ship.
  for (const component of COMPONENTS.filter((entry) => BRANDED.includes(entry.id))) {
    brand(
      join(version, component.path),
      component.id === 'aura-desktop'
        ? { version: options.version, description: 'Aura' }
        : { iconOnly: true },
    );
  }

  const components = [];
  for (const component of COMPONENTS) {
    const copied = await sha256File(join(version, component.path));
    components.push({
      component_id: component.id,
      kind: component.kind,
      version: options.version,
      relative_path: component.path,
      content_hash: { algorithm: 'sha256', hex: copied },
      protocol_version: PROTOCOL_VERSION,
    });
  }
  const releaseId = releaseBundleId(
    options.version,
    options.channel,
    components.map((component) => component.content_hash.hex),
  );
  const manifest = {
    manifest_version: MANIFEST_VERSION,
    bundle_namespace: namespace,
    release_bundle_id: releaseId,
    release_channel: options.channel,
    product_version: options.version,
    build_commit: buildCommit(options.commit),
    target_triple: TARGET_TRIPLE,
    components,
  };
  const bytes = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
  await writeFile(join(version, 'component-manifest.json'), bytes, { flag: 'wx' });
  const record = {
    schema: 'aura-release-assembly-v1',
    version: options.version,
    channel: options.channel,
    bundle_namespace: namespace,
    release_bundle_id: releaseId,
    component_manifest_sha256: sha256(bytes),
    ...(await countFiles(payload)),
  };
  await writeFile(join(out, 'release.json'), `${JSON.stringify(record, null, 2)}\n`, {
    flag: 'wx',
  });
  return { out, payload, version, record };
}

/** Paths inside a payload, with forward slashes, for archive entries. */
export const payloadPath = (payload, path) => relative(payload, path).split(sep).join('/');

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  assemble(parse(process.argv.slice(2)))
    .then(({ out, record }) => {
      console.log(`assembled ${record.version} (${record.release_bundle_id}) in ${out}`);
      console.log(`${record.files} files, ${record.bytes} bytes`);
    })
    .catch((error) => {
      console.error(error.message);
      process.exit(1);
    });
}
