import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join, resolve, sep } from 'node:path';
import { test } from 'node:test';
import { pinnedPowerShellHash, writeReleaseExecutionBinding } from './execution-binding.mjs';

const base = resolve(import.meta.dirname, '../../target/execution-binding-tests');
const hash = (value) => createHash('sha256').update(value).digest('hex');
async function fixture(t) {
  await mkdir(base, { recursive: true });
  const root = await mkdtemp(join(base, 'case-'));
  t.after(() => {
    assert.ok(resolve(root).startsWith(base + sep));
    return rm(root, { recursive: true, force: true });
  });
  const stage = join(root, 'target/managed-runtimes/powershell-test');
  const version = join(root, 'version');
  await mkdir(join(stage, 'payload'), { recursive: true });
  await mkdir(join(root, 'runtimes'));
  await mkdir(version);
  const bytes = Buffer.from('pinned PowerShell');
  const file = { path: 'pwsh.exe', bytes: bytes.length, sha256: hash(bytes) };
  const pin = {
    id: 'powershell-test',
    version: 'test',
    executable: 'pwsh.exe',
    archive_sha256: 'a'.repeat(64),
    files_sha256: hash(`${file.path}|${file.bytes}|${file.sha256}`),
  };
  const manifest = { schema: 'r06-runtime-files-v1', ...pin, files: [file] };
  await writeFile(join(root, 'runtimes/powershell.json'), JSON.stringify(pin));
  await writeFile(join(stage, 'manifest.json'), JSON.stringify(manifest));
  await writeFile(join(stage, 'payload/pwsh.exe'), bytes);
  await writeFile(join(version, 'aura-core.exe'), 'core');
  await writeFile(join(version, 'aura-command-runner.exe'), 'runner');
  return { root, stage, version, bytes, manifest };
}

test('assembly binds actual images and the pin-verified runtime', async (t) => {
  const f = await fixture(t);
  const record = await writeReleaseExecutionBinding(f.root, f.version);
  assert.deepEqual(record.execution_components, {
    core: hash('core'),
    runner: hash('runner'),
    powershell: hash(f.bytes),
  });
  assert.deepEqual(
    JSON.parse(await readFile(join(f.version, 'execution-binding.json'), 'utf8')),
    record,
  );
  await assert.rejects(writeReleaseExecutionBinding(f.root, f.version), { code: 'EEXIST' });
});

test('an altered executable cannot be made an expectation by hashing it again', async (t) => {
  const f = await fixture(t);
  await writeFile(join(f.stage, 'payload/pwsh.exe'), 'a different shell');
  await assert.rejects(pinnedPowerShellHash(f.root));
  await assert.rejects(writeReleaseExecutionBinding(f.root, f.version));
  await assert.rejects(readFile(join(f.version, 'execution-binding.json')), { code: 'ENOENT' });
});

test('an altered inventory and a missing runtime fail before release binding is written', async (t) => {
  const f = await fixture(t);
  f.manifest.files[0].sha256 = 'b'.repeat(64);
  await writeFile(join(f.stage, 'manifest.json'), JSON.stringify(f.manifest));
  await assert.rejects(pinnedPowerShellHash(f.root), /differs from the pin/u);
  await rm(join(f.stage, 'manifest.json'));
  await assert.rejects(writeReleaseExecutionBinding(f.root, f.version), { code: 'ENOENT' });
});
