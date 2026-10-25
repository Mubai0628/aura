/** What can write the renderer's storage to disk: Electron's session. */
export interface StorageSession {
  flushStorageData(): void;
}

/**
 * Chromium writes the
 * window's storage - the open conversation, the drafts, the layout - lazily, and a killed or crashed
 * Aura loses what it had not written yet. It is written at most `everyMs` after it changes.
 */
export function keepStorageWritten(session: StorageSession, everyMs = 2_000): () => void {
  const timer = setInterval(() => session.flushStorageData(), everyMs);
  timer.unref?.();
  return () => clearInterval(timer);
}
