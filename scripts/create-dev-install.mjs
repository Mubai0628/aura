import { createHash } from 'node:crypto';
import { access, cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const profileArgumentIndex = process.argv.indexOf('--profile');
const profile =
  profileArgumentIndex === -1 ? 'development' : process.argv[profileArgumentIndex + 1];
if (profile !== 'development' && profile !== 'acceptance') {
  throw new Error('--profile must be either development or acceptance');
}
const checkoutInstanceId = (await readFile(resolve(root, '.aura-dev/instance-id'), 'utf8')).trim();
if (!/^[0-9a-hjkmnp-tv-z]{26}$/u.test(checkoutInstanceId)) {
  throw new Error('.aura-dev/instance-id must contain one lowercase ULID');
}
const devInstanceId =
  profile === 'acceptance'
    ? encodeCrockford(
        createHash('sha256')
          .update(`aura-p05-clean-acceptance\0${checkoutInstanceId}`, 'utf8')
          .digest()
          .subarray(0, 16),
      ).toLowerCase()
    : checkoutInstanceId;
const bundleNamespace = `aura.desktop.development.${devInstanceId}`;
const unpacked = resolve(root, 'target/electron-builder-package/win-unpacked');
const installRoot = resolve(
  root,
  profile === 'acceptance' ? 'target/aura-acceptance-install' : 'target/aura-dev-install',
);
const launcher = resolve(root, 'target/release/AuraLauncher.exe');
const core = resolve(root, 'target/release/aura-core.exe');
const sandboxHost = resolve(root, 'target/release/aura-sandbox-host.exe');
const egressProxy = resolve(root, 'target/release/aura-egress-proxy.exe');
const elevatedHelper = resolve(root, 'target/release/aura-elevated-helper.exe');
const desktopSource = resolve(unpacked, 'Aura.exe');
const mainPipeBridgeSource = resolve(unpacked, 'resources/native/aura-main-pipe.node');
const desktopHash = await sha256File(desktopSource);
const mainPipeBridgeHash = await sha256File(mainPipeBridgeSource);
const coreHash = await sha256File(core);
const sandboxHostHash = await sha256File(sandboxHost);
const egressProxyHash = await sha256File(egressProxy);
const elevatedHelperHash = await sha256File(elevatedHelper);
const releaseBytes = createHash('sha256')
  .update(
    Buffer.from(
      desktopHash +
        mainPipeBridgeHash +
        coreHash +
        sandboxHostHash +
        egressProxyHash +
        elevatedHelperHash,
      'utf8',
    ),
  )
  .digest()
  .subarray(0, 16);
const releaseBundleId = `rel_${encodeCrockford(releaseBytes)}`;
const version = `0.0.0-dev+${releaseBundleId.slice(4).toLowerCase()}`;
const versionDirectory = resolve(installRoot, 'versions', version);
const desktopPath = resolve(versionDirectory, 'Aura.exe');
const corePath = resolve(versionDirectory, 'aura-core.exe');
const mainPipeBridgePath = resolve(versionDirectory, 'resources/native/aura-main-pipe.node');
const sandboxHostPath = resolve(versionDirectory, 'aura-sandbox-host.exe');
const egressProxyPath = resolve(versionDirectory, 'aura-egress-proxy.exe');
const elevatedHelperPath = resolve(versionDirectory, 'aura-elevated-helper.exe');
let buildCommit = '0000000000000000000000000000000000000000';
try {
  const candidate = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
  }).trim();
  if (/^[0-9a-f]{40}$/u.test(candidate)) buildCommit = candidate;
} catch {
  // The source archive may intentionally have no Git metadata.
}

const manifest = {
  manifest_version: { major: 1, minor: 7 },
  bundle_namespace: bundleNamespace,
  release_bundle_id: releaseBundleId,
  release_channel: 'Nightly',
  product_version: version,
  build_commit: buildCommit,
  target_triple: 'x86_64-pc-windows-msvc',
  components: [
    {
      component_id: 'aura-desktop',
      kind: 'desktop',
      version: '0.1.0',
      relative_path: 'Aura.exe',
      content_hash: { algorithm: 'sha256', hex: desktopHash },
      protocol_version: { major: 1, minor: 7 },
    },
    {
      component_id: 'aura-core',
      kind: 'core',
      version: '0.1.0',
      relative_path: 'aura-core.exe',
      content_hash: { algorithm: 'sha256', hex: coreHash },
      protocol_version: { major: 1, minor: 7 },
    },
    {
      component_id: 'aura-main-pipe',
      kind: 'main_pipe_bridge',
      version: '0.1.0',
      relative_path: 'resources/native/aura-main-pipe.node',
      content_hash: { algorithm: 'sha256', hex: mainPipeBridgeHash },
      protocol_version: { major: 1, minor: 7 },
    },
    {
      component_id: 'aura-sandbox-host',
      kind: 'sandbox_host',
      version: '0.1.0',
      relative_path: 'aura-sandbox-host.exe',
      content_hash: { algorithm: 'sha256', hex: sandboxHostHash },
      protocol_version: { major: 1, minor: 7 },
    },
    {
      component_id: 'aura-egress-proxy',
      kind: 'egress_proxy',
      version: '0.1.0',
      relative_path: 'aura-egress-proxy.exe',
      content_hash: { algorithm: 'sha256', hex: egressProxyHash },
      protocol_version: { major: 1, minor: 7 },
    },
    {
      component_id: 'aura-elevated-helper',
      kind: 'elevated_helper',
      version: '0.1.0',
      relative_path: 'aura-elevated-helper.exe',
      content_hash: { algorithm: 'sha256', hex: elevatedHelperHash },
      protocol_version: { major: 1, minor: 7 },
    },
  ],
};
const manifestBytes = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
const componentManifestHash = createHash('sha256').update(manifestBytes).digest('hex');

await mkdir(resolve(installRoot, 'versions'), { recursive: true });
if (!(await exists(versionDirectory))) {
  await mkdir(versionDirectory, { recursive: true });
  await cp(unpacked, versionDirectory, { recursive: true });
  await cp(core, corePath);
  await cp(sandboxHost, sandboxHostPath);
  await cp(egressProxy, egressProxyPath);
  await cp(elevatedHelper, elevatedHelperPath);
  await writeFile(resolve(versionDirectory, 'component-manifest.json'), manifestBytes);
} else {
  const [
    installedDesktopHash,
    installedCoreHash,
    installedMainPipeBridgeHash,
    installedSandboxHostHash,
    installedEgressProxyHash,
    installedElevatedHelperHash,
    installedManifestHash,
  ] = await Promise.all([
    sha256File(desktopPath),
    sha256File(corePath),
    sha256File(mainPipeBridgePath),
    sha256File(sandboxHostPath),
    sha256File(egressProxyPath),
    sha256File(elevatedHelperPath),
    sha256File(resolve(versionDirectory, 'component-manifest.json')),
  ]);
  if (
    installedDesktopHash !== desktopHash ||
    installedCoreHash !== coreHash ||
    installedMainPipeBridgeHash !== mainPipeBridgeHash ||
    installedSandboxHostHash !== sandboxHostHash ||
    installedEgressProxyHash !== egressProxyHash ||
    installedElevatedHelperHash !== elevatedHelperHash ||
    installedManifestHash !== componentManifestHash
  ) {
    throw new Error(`immutable development release directory is inconsistent: ${versionDirectory}`);
  }
}

await cp(launcher, resolve(installRoot, 'AuraLauncher.exe'));

let previous = null;
try {
  const current = JSON.parse(await readFile(resolve(installRoot, 'active.json'), 'utf8'));
  if (typeof current.active === 'string' && current.active !== version) {
    previous = current.active;
  } else if (typeof current.previous === 'string') {
    previous = current.previous;
  }
} catch {
  // This is the first development install, or the previous pointer is not reusable.
}
await writeFile(
  resolve(installRoot, 'active.json'),
  `${JSON.stringify(
    {
      active: version,
      release_descriptor: {
        bundle_namespace: bundleNamespace,
        identity: {
          release_bundle_id: releaseBundleId,
          component_manifest_hash: { algorithm: 'sha256', hex: componentManifestHash },
        },
      },
      previous,
      pending: null,
      last_known_good: version,
    },
    null,
    2,
  )}\n`,
  'utf8',
);

console.log(`created ${installRoot}`);
console.log(`profile ${profile}`);
console.log(`bundle namespace ${bundleNamespace}`);
console.log(`release bundle ${releaseBundleId}`);

async function sha256File(path) {
  return createHash('sha256')
    .update(await readFile(path))
    .digest('hex');
}

async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

function encodeCrockford(bytes) {
  const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
  let value = 0n;
  for (const byte of bytes) value = (value << 8n) | BigInt(byte);
  let body = '';
  for (let index = 0; index < 26; index += 1) {
    body = `${alphabet[Number(value & 31n)]}${body}`;
    value >>= 5n;
  }
  return body;
}
