import { createHash } from 'node:crypto';
import { cp, copyFile, mkdir, readFile, symlink, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

const gitTestSwitch = Buffer.from('AURA_GIT_TEST_PROFILE');
const memoryTestSwitch = Buffer.from('AURA_MEMORY_TEST_ROOT');

/** An isolated run needs the Git test switch; an ordinary release Core must not carry it. */
export function assertCoreProfile(bytes: Buffer, gitTestCore: boolean) {
  const hasSwitch = bytes.includes(gitTestSwitch);
  if (gitTestCore && !hasSwitch) {
    throw new Error('The performance Core does not contain the Git test isolation switch');
  }
  if (gitTestCore && !bytes.includes(memoryTestSwitch)) {
    throw new Error('The performance Core does not contain the Memory test isolation switch');
  }
  if (!gitTestCore && hasSwitch) {
    throw new Error(
      'target/release/aura-core.exe contains the Git test switch; rebuild the ordinary release Core',
    );
  }
  if (!gitTestCore && bytes.includes(memoryTestSwitch)) {
    throw new Error('target/release/aura-core.exe contains the Memory test switch');
  }
}

/** Use an explicit optimized test Core when Git fixtures require an isolated environment. */
export async function releasePerformanceBundle(root: string) {
  const gitTestCore = process.env.AURA_S08_PERF_GIT_TEST_CORE;
  const gitTestProfile = process.env.AURA_GIT_TEST_PROFILE;
  if (gitTestProfile && !gitTestCore) {
    throw new Error('Git-isolated performance runs require AURA_S08_PERF_GIT_TEST_CORE');
  }
  if (gitTestCore && !gitTestProfile) {
    throw new Error('The Git test Core requires AURA_GIT_TEST_PROFILE');
  }
  const repository = resolve(import.meta.dirname, '../../../..');
  const executable = gitTestCore
    ? resolve(gitTestCore)
    : join(repository, 'target/release/aura-core.exe');
  const bytes = await readFile(executable);
  assertCoreProfile(bytes, Boolean(gitTestCore));
  const profile = gitTestCore ? 'release-git-test-profile' : 'release';
  const bundle = join(root, 'bundle');
  const desktop = join(bundle, 'apps/desktop');
  const runtime = join(bundle, 'target/aura-dev-runtime');
  await mkdir(desktop, { recursive: true });
  await mkdir(runtime, { recursive: true });
  await mkdir(join(bundle, 'target/debug'), { recursive: true });
  await cp(join(repository, 'apps/desktop/dist'), join(desktop, 'dist'), { recursive: true });
  await copyFile(join(repository, 'apps/desktop/package.json'), join(desktop, 'package.json'));
  await copyFile(join(repository, 'pnpm-workspace.yaml'), join(bundle, 'pnpm-workspace.yaml'));
  await symlink(
    join(repository, 'apps/desktop/node_modules'),
    join(desktop, 'node_modules'),
    'junction',
  );
  const hash = createHash('sha256').update(bytes).digest('hex');
  await copyFile(executable, join(bundle, 'target/debug/aura-core.exe'));
  // The development loader's filename is fixed. Bind the copied optimized artifact
  // itself; the report distinguishes the test feature from an ordinary release.
  const manifest = JSON.parse(
    await readFile(join(repository, 'target/aura-dev-runtime/component-manifest.json'), 'utf8'),
  );
  const core = manifest.components.find((component: { kind: string }) => component.kind === 'core');
  core.content_hash = { algorithm: 'sha256', hex: hash };
  const binding = JSON.parse(
    await readFile(join(repository, 'target/aura-dev-runtime/development-binding.json'), 'utf8'),
  );
  binding.core_sha256 = hash;
  binding.execution_components.core = hash;
  await writeFile(join(runtime, 'component-manifest.json'), JSON.stringify(manifest));
  await writeFile(join(runtime, 'development-binding.json'), JSON.stringify(binding));
  return { desktop, artifact: executable, profile, sha256: hash };
}
