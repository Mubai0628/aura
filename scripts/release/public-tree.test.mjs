import assert from 'node:assert/strict';
import { test } from 'node:test';
import { TRANSFORMS } from './public-tree.mjs';

test('the exported size baseline keeps the ceilings of exported files and the crate ceilings', () => {
  const baseline = {
    adopted_on: '2026-10-10',
    metric: 'physical lines',
    deferred_cleanup: 'later',
    crate_ceilings: {
      adopted_on: '2026-10-29',
      metric: 'production lines',
      ceilings: { 'crates/aura-core': 100 },
    },
    ceilings: { 'crates/a/src/lib.rs': 1200, 'crates/private/src/lib.rs': 1300 },
  };
  const exported = new Set(['crates/a/src/lib.rs']);
  const text = TRANSFORMS['scripts/architecture/file-size-baseline.json'](
    JSON.stringify(baseline),
    exported,
  );
  assert.deepEqual(JSON.parse(text), {
    metric: 'physical lines',
    crate_ceilings: baseline.crate_ceilings,
    ceilings: { 'crates/a/src/lib.rs': 1200 },
  });
});
