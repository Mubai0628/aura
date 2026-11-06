/** Data URLs kept, by their length. A picture is a few hundred KiB, so this holds a long conversation's. */
const BUDGET = 48 * 1024 * 1024;

/** Least recently shown first: a hit moves its picture to the end. */
const kept = new Map<string, string>();
const reading = new Map<string, Promise<string | null | undefined>>();
let keptLength = 0;

/**
 * A picture of a conversation's own record as a data URL: `null` when Core cannot give it, and
 * `undefined` without the bridge. An artifact is one fixed content, so a picture read once is
 * kept and shown again without reading it again; one that failed is asked for again next time.
 */
export function readPicture(
  sessionId: string,
  artifactId: string,
): Promise<string | null | undefined> {
  const key = `${sessionId}/${artifactId}`;
  const hit = kept.get(key);
  if (hit !== undefined) {
    kept.delete(key);
    kept.set(key, hit);
    return Promise.resolve(hit);
  }
  let pending = reading.get(key);
  if (pending === undefined) {
    pending = readOnce(key, sessionId, artifactId).finally(() => reading.delete(key));
    reading.set(key, pending);
  }
  return pending;
}

async function readOnce(key: string, sessionId: string, artifactId: string) {
  const result = await window.aura.v1.browser
    ?.readScreenshot({ session_id: sessionId, artifact_id: artifactId })
    .catch(() => null);
  if (result === undefined) return undefined;
  if (result === null || !result.ok) return null;
  const url = `data:${result.value.media_type};base64,${result.value.data_base64}`;
  keep(key, url);
  return url;
}

function keep(key: string, url: string) {
  if (kept.has(key) || url.length > BUDGET) return;
  kept.set(key, url);
  keptLength += url.length;
  for (const [oldest, old] of kept) {
    if (keptLength <= BUDGET) break;
    kept.delete(oldest);
    keptLength -= old.length;
  }
}

export function resetPictureReadsForTests(): void {
  kept.clear();
  reading.clear();
  keptLength = 0;
}
