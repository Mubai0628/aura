import { expect, test } from 'vitest';
import { compareVersions, newestRelease, packageName } from './feed.js';

const release = (tag: string, extra: Record<string, unknown> = {}) => {
  const version = tag.replace(/^v/u, '');
  return {
    tag_name: tag,
    draft: false,
    prerelease: false,
    published_at: '2026-11-01T00:00:00Z',
    body: `notes ${version}`,
    assets: [
      {
        name: packageName(version),
        browser_download_url: `https://github.com/example/releases/download/${tag}/${packageName(version)}`,
        size: 1234,
      },
    ],
    ...extra,
  };
};

test('versions compare by number', () => {
  expect(compareVersions('0.1.10', '0.1.9')).toBe(1);
  expect(compareVersions('0.2.0', '0.10.0')).toBe(-1);
  expect(compareVersions('1.0.0', '1.0.0')).toBe(0);
});

test('the newest published release newer than this one is offered', () => {
  const newest = newestRelease([release('v0.1.1'), release('v0.1.3'), release('v0.1.2')], '0.1.0');
  expect(newest).toMatchObject({
    version: '0.1.3',
    notes: 'notes 0.1.3',
    asset: { name: 'Aura-Update-0.1.3-x64.aura-update', size: 1234 },
  });
  expect(newestRelease([release('v0.1.0'), release('0.0.9')], '0.1.0')).toBeNull();
});

test('drafts, pre-releases, other tags and releases without a usable package are passed over', () => {
  const listing = [
    release('v0.2.0', { draft: true }),
    release('v0.3.0', { prerelease: true }),
    release('nightly-2026'),
    release('v0.4.0', { assets: [] }),
    release('v0.5.0', {
      assets: [{ name: packageName('0.5.0'), browser_download_url: 'http://insecure/x', size: 1 }],
    }),
    release('v0.6.0', {
      assets: [{ name: packageName('0.6.0'), browser_download_url: 'https://x/y', size: 0 }],
    }),
    release('v0.1.1'),
  ];
  expect(newestRelease(listing, '0.1.0')?.version).toBe('0.1.1');
  expect(() => newestRelease({ message: 'rate limited' }, '0.1.0')).toThrow();
});

test('a test release’s own listing may serve its packages from its own origin, and only there', () => {
  const served = (url: string) => [
    release('v0.1.1', {
      assets: [{ name: packageName('0.1.1'), browser_download_url: url, size: 5 }],
    }),
  ];
  const local = served('http://127.0.0.1:47123/download/Aura-Update-0.1.1-x64.aura-update');
  expect(newestRelease(local, '0.1.0', 'http://127.0.0.1:47123/releases')?.asset.url).toBe(
    'http://127.0.0.1:47123/download/Aura-Update-0.1.1-x64.aura-update',
  );
  expect(newestRelease(local, '0.1.0')).toBeNull();
  expect(newestRelease(local, '0.1.0', 'http://127.0.0.1:47124/releases')).toBeNull();
  expect(
    newestRelease(served('http://localhost:47123/x'), '0.1.0', 'http://127.0.0.1:47123/releases'),
  ).toBeNull();
  expect(newestRelease(served('not a url'), '0.1.0', 'http://127.0.0.1:47123/releases')).toBeNull();
});
