import { useCallback, useSyncExternalStore } from 'react';

/**
 * The sub-agent whose conversation the main area shows, per conversation; none shows the
 * conversation itself. The 多 Agent page picks it, the main area and the page's rows read it, and
 * it lasts while the app runs.
 */
const focused = new Map<string, string>();
const listeners = new Set<() => void>();

/** Shows sub-agent `agent` of conversation `sessionId` in the main area, or the conversation. */
export function focusAgent(sessionId: string, agent: string | undefined) {
  if (agent === focused.get(sessionId)) return;
  if (agent === undefined) focused.delete(sessionId);
  else focused.set(sessionId, agent);
  for (const listener of [...listeners]) listener();
}

/** The sub-agent of conversation `sessionId` the main area shows, if any. */
export function focusedAgent(sessionId: string | undefined): string | undefined {
  return sessionId === undefined ? undefined : focused.get(sessionId);
}

export function useFocusedAgent(sessionId: string | undefined): string | undefined {
  const listen = useCallback((listener: () => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, []);
  const current = useCallback(() => focusedAgent(sessionId), [sessionId]);
  return useSyncExternalStore(listen, current);
}

export function resetAgentFocusForTests() {
  focused.clear();
}
