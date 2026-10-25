// Build-time execution expectations. The release inventory seals the resulting
// metadata; runtime readers need neither the checkout nor the staging directory.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const fileHash = async (path) => sha256(await readFile(path));

export async function pinnedPowerShellHash(repository) {
  const pin = JSON.parse(await readFile(resolve(repository, 'runtimes/powershell.json'), 'utf8'));
  assert.match(pin.id, /^[a-zA-Z0-9.-]+$/u);
  assert.equal(pin.executable, 'pwsh.exe');
  const stage = resolve(repository, 'target/managed-runtimes', pin.id);
  const manifest = JSON.parse(await readFile(resolve(stage, 'manifest.json'), 'utf8'));
  assert.equal(manifest.schema, 'r06-runtime-files-v1');
  assert.equal(manifest.id, pin.id);
  assert.equal(manifest.version, pin.version);
  assert.equal(manifest.archive_sha256, pin.archive_sha256);
  const lines = manifest.files
    .map((file) => {
      assert.equal(typeof file.path, 'string');
      assert.ok(Number.isSafeInteger(file.bytes) && file.bytes >= 0);
      assert.match(file.sha256, /^[a-fA-F0-9]{64}$/u);
      return `${file.path}|${file.bytes}|${file.sha256.toLowerCase()}`;
    })
    .sort();
  assert.equal(
    sha256(lines.join('\n')),
    pin.files_sha256,
    'runtime inventory differs from the pin',
  );
  const entries = manifest.files.filter((file) => file.path === pin.executable);
  assert.equal(entries.length, 1, 'exactly one pinned executable');
  const bytes = await readFile(resolve(stage, 'payload', pin.executable));
  assert.equal(bytes.length, entries[0].bytes);
  const digest = sha256(bytes);
  assert.equal(
    digest,
    entries[0].sha256.toLowerCase(),
    'pinned executable differs from its inventory',
  );
  return digest;
}

export async function createReleaseExecutionBinding(repository, versionDirectory) {
  const core = await fileHash(resolve(versionDirectory, 'aura-core.exe'));
  return {
    schema: 'aura-release-execution-binding-v1',
    core_sha256: core,
    execution_components: {
      core,
      runner: await fileHash(resolve(versionDirectory, 'aura-command-runner.exe')),
      powershell: await pinnedPowerShellHash(repository),
    },
  };
}

export async function writeReleaseExecutionBinding(repository, versionDirectory) {
  const document = await createReleaseExecutionBinding(repository, versionDirectory);
  await writeFile(
    resolve(versionDirectory, 'execution-binding.json'),
    `${JSON.stringify(document, null, 2)}\n`,
    { flag: 'wx' },
  );
  return document;
}
