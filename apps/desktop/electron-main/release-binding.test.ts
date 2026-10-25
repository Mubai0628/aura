import { expect, test } from 'vitest';
import { runningEntry } from './release.js';

const manifest = (version: string) => ({
  bundle_namespace: 'aura.desktop.test.binding',
  product_version: version,
  release_bundle_id: `rel_${version}`,
});

const state = {
  active: '0.1.0',
  pending: '0.1.1' as string | null,
  previous: null,
  last_known_good: '0.1.0',
  release_descriptor: {
    bundle_namespace: 'aura.desktop.test.binding',
    identity: {
      release_bundle_id: 'rel_0.1.0',
      component_manifest_hash: { algorithm: 'sha256' as const, hex: 'aa' },
    },
  },
};

test('the active version runs when its manifest is the one active.json names', () => {
  expect(runningEntry(state, '0.1.0', manifest('0.1.0'), 'aa')).toBe('active');
  expect(() => runningEntry(state, '0.1.0', manifest('0.1.0'), 'bb')).toThrow(/does not match/u);
});

test('the version the launcher is trying runs too', () => {
  expect(runningEntry(state, '0.1.1', manifest('0.1.1'), 'cc')).toBe('trial');
});

test('a version active.json does not name, or of another installation, does not run', () => {
  expect(() => runningEntry({ ...state, pending: null }, '0.1.1', manifest('0.1.1'), 'cc')).toThrow(
    /does not match/u,
  );
  expect(() =>
    runningEntry(
      state,
      '0.1.1',
      { ...manifest('0.1.1'), bundle_namespace: 'aura.desktop.production' },
      'cc',
    ),
  ).toThrow(/does not match/u);
  expect(() => runningEntry(state, '0.1.1', manifest('0.1.2'), 'cc')).toThrow(/does not match/u);
});
