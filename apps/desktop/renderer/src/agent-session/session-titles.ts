const KEY = 'aura.pending-session-titles.v1';
interface PendingTitle {
  sessionId: string;
  title: string;
}
let cached: PendingTitle[] | undefined;
export const firstMessageTitle = (text: string) =>
  Array.from(text.trim().replace(/\s+/gu, ' ')).slice(0, 5).join('');
function read(): PendingTitle[] {
  if (cached) return cached;
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    return Array.isArray(value)
      ? value.filter(
          (v): v is PendingTitle =>
            !!v && typeof v.sessionId === 'string' && typeof v.title === 'string',
        )
      : [];
  } catch {
    return [];
  }
}
function save(items: PendingTitle[]): void {
  cached = items;
  // A metadata storage failure cannot change an already accepted message to unknown.
  try {
    localStorage.setItem(KEY, JSON.stringify(items));
  } catch {
    /* Retry in memory. */
  }
}
export function rememberFirstTitle(sessionId: string, text: string): void {
  const current = read();
  if (!current.some((item) => item.sessionId === sessionId))
    save([...current, { sessionId, title: firstMessageTitle(text) }]);
  void flushPendingTitles();
}
let pending: Promise<void> | undefined;
export function flushPendingTitles(): Promise<void> {
  if (pending) return pending;
  pending = (async () => {
    const attempted = new Set<string>();
    for (
      let item = read().find((value) => !attempted.has(value.sessionId));
      item;
      item = read().find((value) => !attempted.has(value.sessionId))
    ) {
      attempted.add(item.sessionId);
      const sessionId = item.sessionId;
      try {
        const result = await window.aura.v1.renameSession({
          session_id: item.sessionId,
          expected_title: '',
          title: item.title,
        });
        // A manual rename takes precedence over the automatic initial title.
        if (result.ok || result.error.message.includes('session.title_conflict'))
          save(read().filter((v) => v.sessionId !== sessionId));
      } catch {
        /* Preserve the idempotent metadata update across restart. Never resend a message. */
      }
    }
  })().finally(() => {
    pending = undefined;
  });
  return pending;
}
