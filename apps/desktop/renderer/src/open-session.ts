/**
 * The conversation open in this window. It is kept the moment a conversation opens, not when Aura quits, so a crash or a
 * killed process still leaves it; the next start opens it again while it exists. A preference of
 * this window only, like the layout and the drafts.
 */
export const OPEN_SESSION_KEY = 'aura.ui.v1.open-session';

export interface OpenSession {
  workspaceId: string;
  sessionId: string;
}

export function readOpenSession(): OpenSession | undefined {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(OPEN_SESSION_KEY) ?? 'null');
    if (typeof value !== 'object' || value === null) return undefined;
    const { workspaceId, sessionId } = value as Record<string, unknown>;
    return typeof workspaceId === 'string' && typeof sessionId === 'string'
      ? { workspaceId, sessionId }
      : undefined;
  } catch {
    return undefined;
  }
}

export function rememberOpenSession(open: OpenSession): void {
  try {
    localStorage.setItem(OPEN_SESSION_KEY, JSON.stringify(open));
  } catch {
    /* Not kept: the next start opens the first conversation. */
  }
}
