import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, test } from 'vitest';
import { LAUNCHER_READY_FILE, createLauncherReadyReporter } from './launcher-ready.js';

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

const reporter = async () => {
  const dataRoot = await mkdtemp(join(tmpdir(), 'aura-launcher-ready-'));
  roots.push(dataRoot);
  const reported = createLauncherReadyReporter({
    dataRoot,
    releaseBundleId: 'rel_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    productVersion: '0.1.1',
  });
  const read = () =>
    readFile(join(dataRoot, LAUNCHER_READY_FILE), 'utf8').then(
      (text) => JSON.parse(text) as Record<string, string>,
      () => null,
    );
  return { reported, read };
};

test('the release is reported once Core is ready and the window has shown, in either order', async () => {
  const first = await reporter();
  await first.reported.coreState('starting');
  await first.reported.windowShown();
  expect(await first.read()).toBeNull();
  await first.reported.coreState('ready');
  expect(await first.read()).toMatchObject({
    release_bundle_id: 'rel_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    product_version: '0.1.1',
  });

  const second = await reporter();
  await second.reported.coreState('ready');
  expect(await second.read()).toBeNull();
  await second.reported.windowShown();
  expect(await second.read()).not.toBeNull();
});

test('a Core that never gets ready is never reported', async () => {
  const { reported, read } = await reporter();
  await reported.windowShown();
  for (const state of ['starting', 'restarting', 'incompatible', 'failed']) {
    await reported.coreState(state);
  }
  expect(await read()).toBeNull();
});
