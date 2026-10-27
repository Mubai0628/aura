// The layout a release carries and an installation keeps.
//
//   <install root>/AuraLauncher.exe         a copy of the launcher the active release names
//   <install root>/Uninstall Aura.exe       a copy of the setup program it names
//   <install root>/active.json              which version runs, the one before it, one waiting
//   <install root>/versions/<version>/      one complete, immutable release
//       component-manifest.json            components, their hashes, the release identity
//       component-manifest.p7s             its detached signature
//       LICENSE.txt, THIRD-PARTY-NOTICES.txt
//       managed-runtimes/<runtime id>/     the pinned PowerShell, checked by its own file list
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';

export const PRODUCTION_NAMESPACE = 'aura.desktop.production';
export const TEST_NAMESPACE_PREFIX = 'aura.desktop.test.';
export const MANIFEST_VERSION = { major: 1, minor: 7 };
export const PROTOCOL_VERSION = { major: 1, minor: 7 };
export const TARGET_TRIPLE = 'x86_64-pc-windows-msvc';
export const CHANNELS = ['Stable', 'Preview', 'Nightly'];
export const VERSION_PATTERN = /^\d+\.\d+\.\d+$/u;

/** The components of a release, where they come from, and where they sit in its folder. */
export const COMPONENTS = [
  { id: 'aura-launcher', kind: 'launcher', path: 'AuraLauncher.exe', from: 'binaries' },
  { id: 'aura-setup', kind: 'setup', path: 'aura-setup.exe', from: 'binaries' },
  { id: 'aura-desktop', kind: 'desktop', path: 'Aura.exe', from: 'desktop' },
  {
    id: 'aura-main-pipe',
    kind: 'main_pipe_bridge',
    path: 'resources/native/aura-main-pipe.node',
    from: 'desktop',
  },
  { id: 'aura-core', kind: 'core', path: 'aura-core.exe', from: 'binaries' },
  {
    id: 'aura-sandbox-host',
    kind: 'sandbox_host',
    path: 'aura-sandbox-host.exe',
    from: 'binaries',
  },
  {
    id: 'aura-egress-proxy',
    kind: 'egress_proxy',
    path: 'aura-egress-proxy.exe',
    from: 'binaries',
  },
  {
    id: 'aura-elevated-helper',
    kind: 'elevated_helper',
    path: 'aura-elevated-helper.exe',
    from: 'binaries',
  },
  // Beside Core: the relay and Core accept each other only from the same folder.
  {
    id: 'aura-chrome-host',
    kind: 'chrome_native_host',
    path: 'aura-chrome-host.exe',
    from: 'binaries',
  },
];
/** Where the Chrome extension the user loads sits in a release's folder. */
export const CHROME_EXTENSION_PATH = 'resources/chrome-extension';
/** Components that get Aura's icon (the desktop also its version information) once copied. */
export const BRANDED = ['aura-launcher', 'aura-setup', 'aura-desktop'];

export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
export const sha256File = async (path) => sha256(await readFile(path));

export function encodeCrockford(bytes) {
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

/** A release identity from what makes the release: its version, channel and component bytes. */
export function releaseBundleId(version, channel, componentHashes) {
  const seed = sha256(Buffer.from([version, channel, ...componentHashes].join('\n'), 'utf8'));
  return `rel_${encodeCrockford(Buffer.from(seed, 'hex').subarray(0, 16))}`;
}

/** Compares dotted release versions numerically. */
export function compareVersions(left, right) {
  const a = left.split('.').map(Number);
  const b = right.split('.').map(Number);
  for (let index = 0; index < 3; index += 1) {
    if (a[index] !== b[index]) return a[index] < b[index] ? -1 : 1;
  }
  return 0;
}
