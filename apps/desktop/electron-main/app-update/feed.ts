// Where new versions are announced: the GitHub releases of Aura's public repository. A release
// counts when it is published (not a draft or pre-release), its tag is a version and it carries
// the update package for that version.

/** The public repository the releases come from. */
export const UPDATE_REPOSITORY = 'mubai0628/aura';

export interface FeedRelease {
  version: string;
  published_at: string | null;
  notes: string;
  asset: { name: string; url: string; size: number };
}

export const releasesUrl = (repository: string): string =>
  `https://api.github.com/repos/${repository}/releases?per_page=20`;

export const packageName = (version: string): string => `Aura-Update-${version}-x64.aura-update`;

const VERSION = /^v?(\d+\.\d+\.\d+)$/u;

/** Compares dotted versions part by part as numbers. */
export function compareVersions(left: string, right: string): number {
  const a = left.split('.').map(Number);
  const b = right.split('.').map(Number);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const difference = (a[index] ?? 0) - (b[index] ?? 0);
    if (difference !== 0) return Math.sign(difference);
  }
  return 0;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const originOf = (url: string): string | null => {
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
};

/** Over https, or from the origin of a test release's own listing, which may be plain http. */
const downloadable = (url: string, testFeed: string | undefined): boolean =>
  url.startsWith('https://') ||
  (testFeed !== undefined && originOf(url) !== null && originOf(url) === originOf(testFeed));

function release(value: unknown, testFeed: string | undefined): FeedRelease | null {
  if (!isRecord(value) || value.draft === true || value.prerelease === true) return null;
  const version =
    typeof value.tag_name === 'string' ? VERSION.exec(value.tag_name)?.[1] : undefined;
  if (!version || !Array.isArray(value.assets)) return null;
  const asset = value.assets.find(
    (candidate): candidate is Record<string, unknown> =>
      isRecord(candidate) && candidate.name === packageName(version),
  );
  if (
    !asset ||
    typeof asset.browser_download_url !== 'string' ||
    !downloadable(asset.browser_download_url, testFeed) ||
    typeof asset.size !== 'number' ||
    asset.size <= 0
  ) {
    return null;
  }
  return {
    version,
    published_at: typeof value.published_at === 'string' ? value.published_at : null,
    notes: typeof value.body === 'string' ? value.body.slice(0, 20_000) : '',
    asset: { name: packageName(version), url: asset.browser_download_url, size: asset.size },
  };
}

/**
 * The newest usable release newer than `current`, from a GitHub releases listing; `testFeed` is
 * the address of a test release's own listing when that is what was read.
 */
export function newestRelease(
  listing: unknown,
  current: string,
  testFeed?: string,
): FeedRelease | null {
  if (!Array.isArray(listing)) throw new Error('the release listing is not a list');
  let newest: FeedRelease | null = null;
  for (const entry of listing) {
    const candidate = release(entry, testFeed);
    if (
      candidate &&
      compareVersions(candidate.version, current) > 0 &&
      (!newest || compareVersions(candidate.version, newest.version) > 0)
    ) {
      newest = candidate;
    }
  }
  return newest;
}
