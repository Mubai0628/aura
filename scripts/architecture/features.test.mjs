import assert from 'node:assert/strict';
import test from 'node:test';
import { PRODUCT_FEATURE_SETS, testOnlyFeatureViolations, treeArguments } from './features.mjs';

test('a test-only feature on any package of a product build is reported', () => {
  const tree = [
    'aura-core v0.1.10 (C:\\repo\\crates\\aura-core)|',
    'aura-core-base v0.1.10 (C:\\repo\\crates\\aura-core-base)|default,test-hooks',
    'tokio v1.48.0|default,fs,io-util',
    'aura-core-base v0.1.10 (C:\\repo\\crates\\aura-core-base)|default,test-hooks (*)',
  ].join('\r\n');
  assert.deepEqual(testOnlyFeatureViolations(tree), ['aura-core-base/test-hooks']);
});

test('a repeated package marked "(*)" is still read', () => {
  const tree = [
    'aura-git-service v0.1.10 (C:\\x)|test-hooks (*)',
    'aura-model-service v0.1.10 (C:\\y)|default,test-hooks (*)',
  ].join('\n');
  assert.deepEqual(testOnlyFeatureViolations(tree), [
    'aura-git-service/test-hooks',
    'aura-model-service/test-hooks',
  ]);
});

test('the listing is requested without colour', () => {
  const args = treeArguments([]);
  assert.equal(args[args.indexOf('--color') + 1], 'never');
});

test('the listing keeps the lock file but may fetch packages a fresh checkout lacks', () => {
  const args = treeArguments([]);
  assert.ok(args.includes('--locked'));
  assert.ok(!args.includes('--offline'), 'a fresh checkout has not downloaded the whole graph');
});

test('ordinary and similarly named features pass', () => {
  const tree = [
    'aura-fs-tools v0.1.10 (C:\\x)|search-test-faults',
    'aura-backend-windows-user v0.1.10 (C:\\y)|terminal-test-profile',
    'aura-network-control v0.1.10 (C:\\z)|test-fixture-tls,test-hooks-extra',
    '',
  ].join('\n');
  assert.deepEqual(testOnlyFeatureViolations(tree), []);
});

test('every product feature set is listed with dev edges off', () => {
  for (const features of PRODUCT_FEATURE_SETS) {
    const args = treeArguments(features);
    assert.deepEqual(args.slice(0, 5), ['tree', '-p', 'aura-core', '-e', 'no-dev']);
    if (features.length) assert.deepEqual(args.slice(-2), ['--features', features.join(',')]);
  }
});
