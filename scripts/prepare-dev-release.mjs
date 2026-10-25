// Builds the per-checkout Development Release Bundle manifest.
//
// The manifest is what every entry point compares the published execution Core
// against, so its component hashes have to describe the binaries this checkout
// actually builds. The checked-in `resources/component-manifest.example.json`
// carries recognisable placeholder digests; keeping them here would make "does a
// published Core match this build?" unanswerable, and `.aura-dev` would look like
// evidence of the current Core when it is only a namespace.
//
// Two things are bound, and they are not the same list:
//
// * the **desktop** components the Release Bundle describes — Core, Sandbox Host,
//   Egress Proxy, Elevated Helper and the Main Pipe Bridge,
// * the **execution** components the protected publication copies into the
//   installation — the Core again, plus the command runner, the execution helper,
//   the maintainer, and the pinned PowerShell runtime the installation manifest
//   declares as `powershell_image`.
//
// A run is only consistent with its source when *both* sets are the ones this build
// produced. Comparing the Core hash alone would miss a rebuilt runner entirely,
// because the installation's runner is a separate file object that can be replaced
// without touching Core — and the installation declares the runner and the pinned
// runtime separately, so both are recorded here and consumed at startup.
import { createHash, randomBytes } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
// Which outputs are bound, and where they live, is stated once so the candidate build
// and this entry cannot disagree about the set they both bind.
import { DEBUG_DIRECTORY, DEVELOPMENT_OUTPUTS, EXECUTION_OUTPUTS } from './execution-images.mjs';
import { pinnedPowerShellHash } from './execution/execution-binding.mjs';

const root = resolve(import.meta.dirname, '..');
const stateRoot = resolve(root, '.aura-dev');
const instancePath = resolve(stateRoot, 'instance-id');
const runtimeRoot = resolve(root, 'target/aura-dev-runtime');
const sourcePath = resolve(root, 'resources/component-manifest.example.json');
const debugDirectory = DEBUG_DIRECTORY;

const outputPath = (relative) => resolve(debugDirectory, relative);

await mkdir(stateRoot, { recursive: true });
const devInstanceId = await loadOrCreateInstanceId();
const bundleNamespace = `aura.desktop.development.${devInstanceId}`;
const source = JSON.parse(await readFile(sourcePath, 'utf8'));
const bound = new Map();
for (const [kind, name] of Object.entries(DEVELOPMENT_OUTPUTS)) {
  const digest = await sha256IfPresent(outputPath(name));
  if (digest !== null) bound.set(kind, digest);
}
const execution = new Map();
for (const [kind, name] of Object.entries(EXECUTION_OUTPUTS)) {
  const digest = await sha256IfPresent(outputPath(name));
  if (digest !== null) execution.set(kind, digest);
}
// The installation manifest also declares the pinned PowerShell runtime it publishes
// at `<install>/runtime/pwsh.exe`. It is not a Rust image this repository builds, but
// it is part of the execution version the installation declares, and an expectation
// the build can verify is worth recording: without it, "the runner matches" would be
// read as "the whole execution version matches" while the published shell could still
// be another version.
const pinnedRuntime = await pinnedPowerShellHash(root).catch((error) => {
  if (error?.code === 'ENOENT') return null;
  throw error;
});
if (pinnedRuntime !== null) execution.set('powershell', pinnedRuntime);
const releaseSeed = createHash('sha256')
  .update(bundleNamespace, 'utf8')
  .update(JSON.stringify(source), 'utf8')
  .digest()
  .subarray(0, 16);
const manifest = {
  ...source,
  bundle_namespace: bundleNamespace,
  release_bundle_id: `rel_${encodeCrockford(releaseSeed)}`,
  release_channel: 'Nightly',
  components: source.components.map((component) => {
    const digest = bound.get(component.kind);
    return digest === undefined
      ? component
      : { ...component, content_hash: { algorithm: 'sha256', hex: digest } };
  }),
};

await mkdir(runtimeRoot, { recursive: true });
await writeFile(
  resolve(runtimeRoot, 'component-manifest.json'),
  `${JSON.stringify(manifest, null, 2)}\n`,
  'utf8',
);
// The binding record is the evidence for "which component versions does this run
// expect". It is deliberately next to the manifest instead of inside it: the
// manifest is a Release Bundle document with a shape the Core validates, and adding
// a field to it would change the document the product compares.
await writeFile(
  resolve(runtimeRoot, 'development-binding.json'),
  `${JSON.stringify(
    {
      schema: 'aura-development-component-binding-v1',
      bundle_namespace: bundleNamespace,
      component_root: debugDirectory,
      main_pipe_bridge_path: outputPath(DEVELOPMENT_OUTPUTS.main_pipe_bridge),
      bound_components: [...bound.keys()].sort(),
      unbound_components: Object.keys(DEVELOPMENT_OUTPUTS)
        .filter((kind) => !bound.has(kind))
        .sort(),
      core_sha256: bound.get('core') ?? null,
      execution_components: Object.fromEntries([...execution.entries()].sort()),
      unbound_execution_components: [...Object.keys(EXECUTION_OUTPUTS), 'powershell']
        .filter((kind) => !execution.has(kind))
        .sort(),
      // A directory listing recorded next to the hashes, so "the file is present"
      // and "the file is the expected version" stay distinguishable.
      present_execution_files: await presentExecutionFiles(),
    },
    null,
    2,
  )}\n`,
  'utf8',
);

async function presentExecutionFiles() {
  try {
    const names = await readdir(debugDirectory);
    return names.filter((name) => name.endsWith('.exe')).sort();
  } catch {
    return [];
  }
}

async function sha256IfPresent(path) {
  try {
    const info = await stat(path);
    if (!info.isFile()) return null;
  } catch {
    return null;
  }
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(path)) hash.update(bytes);
  return hash.digest('hex');
}

async function loadOrCreateInstanceId() {
  try {
    return validateInstanceId((await readFile(instancePath, 'utf8')).trim());
  } catch (error) {
    if (error?.code !== 'ENOENT') throw error;
  }
  const candidate = encodeCrockford(randomBytes(16)).toLowerCase();
  try {
    await writeFile(instancePath, `${candidate}\n`, { encoding: 'utf8', flag: 'wx' });
    return candidate;
  } catch (error) {
    if (error?.code !== 'EEXIST') throw error;
    return validateInstanceId((await readFile(instancePath, 'utf8')).trim());
  }
}

function validateInstanceId(value) {
  if (!/^[0-9a-hjkmnp-tv-z]{26}$/u.test(value)) {
    throw new Error('.aura-dev/instance-id must contain one lowercase ULID');
  }
  return value;
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
