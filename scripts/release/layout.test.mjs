import assert from 'node:assert/strict';
import { test } from 'node:test';
import { compareVersions, encodeCrockford, releaseBundleId } from './layout.mjs';

test('versions compare by number, not by text', () => {
  assert.equal(compareVersions('0.1.10', '0.1.9'), 1);
  assert.equal(compareVersions('0.2.0', '0.10.0'), -1);
  assert.equal(compareVersions('1.0.0', '1.0.0'), 0);
});

test('a release identity follows its version, channel and component bytes', () => {
  const hashes = ['a'.repeat(64), 'b'.repeat(64)];
  const first = releaseBundleId('0.1.0', 'Stable', hashes);
  assert.match(first, /^rel_[0-9A-HJKMNP-TV-Z]{26}$/u);
  assert.equal(releaseBundleId('0.1.0', 'Stable', hashes), first);
  assert.notEqual(releaseBundleId('0.1.1', 'Stable', hashes), first);
  assert.notEqual(releaseBundleId('0.1.0', 'Preview', hashes), first);
  assert.notEqual(releaseBundleId('0.1.0', 'Stable', [hashes[1], hashes[0]]), first);
});

test('Crockford base32 encodes 16 bytes in 26 characters', () => {
  assert.equal(encodeCrockford(Buffer.alloc(16)), '0'.repeat(26));
  assert.equal(encodeCrockford(Buffer.alloc(16, 0xff)).length, 26);
});
