// The development entry's execution binding, as a rule set rather than a report.
//
// These cases exercise `apps/desktop/electron-main/core-binding.mjs`, which is the
// one place the packaged Main process and the ordinary `corepack pnpm dev` entry
// decide which published Core they may run. A wrong answer here is what makes a
// developer believe the restricted Shell is prepared when it is not, so every
// refusal is asserted by its own status and not merely as "not selected".
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join, resolve, sep } from 'node:path';
import { test } from 'node:test';
import {
  describeCoreBinding,
  resolvePublishedCore,
} from '../apps/desktop/electron-main/core-binding.mjs';
import { describeDevExecution, inspectDevExecution } from './dev-execution-preflight.mjs';

const base = resolve(import.meta.dirname, '../target/dev-preflight-tests');
const id = '12345678-1234-1234-1234-123456789abc';
const sha256 = (text) => createHash('sha256').update(text).digest('hex');

async function fixture(t) {
  await mkdir(base, { recursive: true });
  const root = await mkdtemp(join(base, 'case-'));
  // Only this test's own directory is removed. It cannot resolve outside base.
  t.after(() => {
    assert.ok(resolve(root).startsWith(base + sep));
    return rm(root, { recursive: true, force: true });
  });
  const programData = join(root, 'ProgramData');
  const install = join(programData, 'Aura', 'R06-Spike', id);
  const debug = join(root, 'debug');
  await mkdir(install, { recursive: true });
  await mkdir(debug);
  const corePath = join(debug, 'aura-core.exe');
  const publishedPath = join(install, 'aura-core.exe');
  await writeFile(corePath, 'this build');
  await writeFile(publishedPath, 'published build');
  const expectedSha256 = sha256('this build');
  const manifest = {
    installation_id: id,
    revision: '1',
    core_image: {
      path: publishedPath,
      sha256: { algorithm: 'sha256', hex: sha256('published build') },
    },
  };
  const manifestPath = join(install, 'execution.json');
  await writeFile(manifestPath, JSON.stringify(manifest));
  return { corePath, programData, manifest, manifestPath, publishedPath, install, expectedSha256 };
}

/** Replaces the published image and its declared hash with this run's build. */
async function publishThisBuild(f) {
  await writeFile(f.publishedPath, 'this build');
  f.manifest.core_image.sha256.hex = f.expectedSha256;
  await writeFile(f.manifestPath, JSON.stringify(f.manifest));
}

test('the actual dev command builds, binds the Development Bundle, then preflights', async () => {
  const pkg = JSON.parse(
    await readFile(resolve(import.meta.dirname, '../apps/desktop/package.json'), 'utf8'),
  );
  // The order matters: the manifest has to be rebuilt from the binaries that were
  // just produced, or the published-Core comparison uses the previous build.
  assert.match(
    pkg.scripts.dev,
    /cargo build .*&& node \.\.\/\.\.\/scripts\/prepare-dev-release\.mjs && node \.\.\/\.\.\/scripts\/dev-execution-preflight\.mjs && concurrently/u,
  );
  assert.match(pkg.scripts.dev, /RUSTFLAGS=.*crt-static/u);
});

test('the Development Bundle is written after the components it binds', async () => {
  const pkg = JSON.parse(
    await readFile(resolve(import.meta.dirname, '../apps/desktop/package.json'), 'utf8'),
  );
  // `build:electron` binds the native bridge and the Rust images by hash. Writing the
  // binding first would record the previous bridge: the addon is rebuilt by every build,
  // so the recorded digest would never describe the file this run loads.
  const electron = pkg.scripts['build:electron'];
  const native = electron.indexOf('build:native');
  const binding = electron.indexOf('prepare-dev-release.mjs');
  assert.ok(native >= 0, 'build:electron builds the native components');
  assert.ok(binding > native, 'the binding record is written after those components exist');
});

test('an installation published for another version is refused by name', async (t) => {
  const f = await fixture(t);
  const before = await readFile(f.manifestPath);
  const resolution = await resolvePublishedCore({
    programData: f.programData,
    expectedSha256: f.expectedSha256,
  });
  assert.equal(resolution.status, 'not_published_for_this_version');
  assert.deepEqual(describeCoreBinding(resolution), {
    state: 'unbound',
    gap: 'not_published_for_this_version',
    corePath: null,
    installationId: null,
    declaredSha256: null,
  });
  assert.deepEqual(await readFile(f.manifestPath), before, 'the resolver never writes');
});

test('identical bytes at another path are not the published Core', async (t) => {
  const f = await fixture(t);
  await writeFile(f.publishedPath, 'this build');
  // Declared path points at the debug output, not the install root.
  f.manifest.core_image.path = f.corePath;
  f.manifest.core_image.sha256.hex = f.expectedSha256;
  await writeFile(f.manifestPath, JSON.stringify(f.manifest));
  const resolution = await resolvePublishedCore({
    programData: f.programData,
    expectedSha256: f.expectedSha256,
  });
  assert.equal(resolution.status, 'published_path_mismatch');
});

test('a matching path and hash is selected, and that is all it claims', async (t) => {
  const f = await fixture(t);
  await publishThisBuild(f);
  const resolution = await resolvePublishedCore({
    programData: f.programData,
    expectedSha256: f.expectedSha256,
  });
  assert.equal(resolution.status, 'selected');
  assert.equal(resolution.path, f.publishedPath);
  assert.equal(resolution.installationId, id);
  const described = describeCoreBinding(resolution);
  assert.equal(described.state, 'bound');
  assert.equal(described.gap, null);
});

test('a published file whose bytes moved is not selected', async (t) => {
  const f = await fixture(t);
  await publishThisBuild(f);
  await writeFile(f.publishedPath, 'tampered');
  const resolution = await resolvePublishedCore({
    programData: f.programData,
    expectedSha256: f.expectedSha256,
  });
  assert.equal(resolution.status, 'not_published_for_this_version');
});

test('two matching installations are ambiguous rather than a guess', async (t) => {
  const f = await fixture(t);
  await publishThisBuild(f);
  const other = join(f.programData, 'Aura', 'R06-Spike', 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee');
  await mkdir(other, { recursive: true });
  await writeFile(join(other, 'aura-core.exe'), 'this build');
  await writeFile(
    join(other, 'execution.json'),
    JSON.stringify({
      installation_id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
      revision: '1',
      core_image: {
        path: join(other, 'aura-core.exe'),
        sha256: { algorithm: 'sha256', hex: f.expectedSha256 },
      },
    }),
  );
  const resolution = await resolvePublishedCore({
    programData: f.programData,
    expectedSha256: f.expectedSha256,
  });
  assert.equal(resolution.status, 'ambiguous');
});

test('a malformed manifest is unreadable rather than missing', async (t) => {
  const f = await fixture(t);
  await writeFile(f.manifestPath, '{broken');
  const result = await inspectDevExecution({
    corePath: f.corePath,
    programData: f.programData,
    expectedSha256: f.expectedSha256,
  });
  assert.equal(result.installations[0].status, 'unreadable');
  assert.ok(describeDevExecution(result).length < 4096, 'the report stays bounded');
});

test('an absent installation store is reported as missing, never as ready', async (t) => {
  const f = await fixture(t);
  const resolution = await resolvePublishedCore({
    programData: join(f.programData, 'does-not-exist'),
    expectedSha256: f.expectedSha256,
  });
  assert.equal(resolution.status, 'installation_missing');
});

test('an unknown expected version selects nothing', async (t) => {
  const f = await fixture(t);
  await publishThisBuild(f);
  const resolution = await resolvePublishedCore({
    programData: f.programData,
    expectedSha256: '',
  });
  assert.equal(resolution.status, 'expected_version_unknown');
  assert.equal(describeCoreBinding(resolution).state, 'unbound');
});
