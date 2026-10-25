// @vitest-environment node
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { assertCoreProfile, releasePerformanceBundle } from './release-bundle';

let root: string;

beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), 'aura-git-test-performance-'));
  vi.stubEnv('AURA_S08_PERF_GIT_TEST_CORE', '');
  vi.stubEnv('AURA_GIT_TEST_PROFILE', '');
});

afterEach(async () => {
  vi.unstubAllEnvs();
  await rm(root, { recursive: true, force: true });
});

test('an isolated Git performance run cannot fall back to an ordinary release Core', async () => {
  vi.stubEnv('AURA_GIT_TEST_PROFILE', join(root, 'profile.json'));
  await expect(releasePerformanceBundle(root)).rejects.toThrow(
    'Git-isolated performance runs require AURA_S08_PERF_GIT_TEST_CORE',
  );
});

test('a Git test Core cannot start without its isolated profile', async () => {
  vi.stubEnv('AURA_S08_PERF_GIT_TEST_CORE', join(root, 'test-core.exe'));
  await expect(releasePerformanceBundle(root)).rejects.toThrow(
    'The Git test Core requires AURA_GIT_TEST_PROFILE',
  );
});

// An isolated performance Core was once built into target/release, so a run without the test
// variables would have reported a test-feature binary as the ordinary release.
test('a test-feature binary cannot be reported as the ordinary release Core', () => {
  expect(() => assertCoreProfile(Buffer.from('... AURA_GIT_TEST_PROFILE ...'), false)).toThrow(
    'contains the Git test switch',
  );
  expect(() => assertCoreProfile(Buffer.from('ordinary release'), false)).not.toThrow();
  expect(() =>
    assertCoreProfile(Buffer.from('... AURA_GIT_TEST_PROFILE AURA_MEMORY_TEST_ROOT ...'), true),
  ).not.toThrow();
  expect(() => assertCoreProfile(Buffer.from('... AURA_MEMORY_TEST_ROOT ...'), false)).toThrow(
    'Memory test switch',
  );
  expect(() => assertCoreProfile(Buffer.from('... AURA_GIT_TEST_PROFILE ...'), true)).toThrow(
    'Memory test isolation',
  );
});

test('an ordinary binary cannot be relabelled as a Git test Core', async () => {
  const executable = join(root, 'ordinary-core.exe');
  await writeFile(executable, 'ordinary build without the test switch');
  vi.stubEnv('AURA_S08_PERF_GIT_TEST_CORE', executable);
  vi.stubEnv('AURA_GIT_TEST_PROFILE', join(root, 'profile.json'));
  await expect(releasePerformanceBundle(root)).rejects.toThrow(
    'The performance Core does not contain the Git test isolation switch',
  );
});
