/** Verified message pages that survive a session remount.
 *
 * The key is the queue identity, the body identity and the announced sealed
 * size. A retry, a new body or a new revision is a different key, so the old
 * pages are not shown as the new body. The cache is bounded and drops the
 * oldest entry first.
 */

const pages = new Map<string, { bytes: number; text: string }>();
/** One entry, measured in UTF-8 bytes. A larger body stays readable and is not stored. */
export const PAGE_CACHE_ENTRY_BYTES = 1024 * 1024;
/** All entries together, in UTF-8 bytes. */
export const PAGE_CACHE_TOTAL_BYTES = 4 * 1024 * 1024;
let used = 0;

const utf8Bytes = (text: string): number => new TextEncoder().encode(text).length;

export const pageCacheKey = (id: string, bodyId: string | null, totalBytes: number): string =>
  `${id}\u0000${bodyId ?? ''}\u0000${totalBytes}`;

export const readPageCache = (key: string): string | undefined => {
  const hit = pages.get(key);
  if (hit === undefined) return undefined;
  pages.delete(key);
  pages.set(key, hit);
  return hit.text;
};

/** Tests start from an empty cache so one fixture cannot satisfy the next. */
export const clearMessageTextCache = (): void => {
  pages.clear();
  used = 0;
};

export const writePageCache = (key: string, text: string): void => {
  const previous = pages.get(key);
  if (previous !== undefined) {
    used -= previous.bytes;
    pages.delete(key);
  }
  const bytes = utf8Bytes(text);
  if (bytes > PAGE_CACHE_ENTRY_BYTES || bytes > PAGE_CACHE_TOTAL_BYTES) return;
  while (used + bytes > PAGE_CACHE_TOTAL_BYTES && pages.size > 0) {
    const oldest = pages.keys().next().value;
    if (oldest === undefined) break;
    used -= pages.get(oldest)?.bytes ?? 0;
    pages.delete(oldest);
  }
  if (used + bytes > PAGE_CACHE_TOTAL_BYTES) return;
  pages.set(key, { bytes, text });
  used += bytes;
};
