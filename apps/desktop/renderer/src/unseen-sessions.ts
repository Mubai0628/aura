import { useSyncExternalStore } from 'react';

/** The conversations with a notification the user has not looked at, as Main has them. */
export type UnseenSessions = ReadonlySet<string>;

const NONE: UnseenSessions = new Set();

// One subscription to Main for every part of the window that shows it, while any of them is shown.
let known: UnseenSessions = NONE;
const listeners = new Set<() => void>();
let stop: (() => void) | undefined;

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (listeners.size === 1)
    stop = window.aura?.v1?.notifications?.onUnseenSessions?.((sessionIds) => {
      known = new Set(sessionIds);
      for (const each of listeners) each();
    });
  return () => {
    listeners.delete(listener);
    if (listeners.size > 0) return;
    stop?.();
    stop = undefined;
    known = NONE;
  };
}

/**
 * The conversations whose notification (a finished task, an approval, a question) the user has
 * not looked at: marked in the sidebar as the red dot on the taskbar button marks Aura, and gone
 * with it once each has been in front.
 */
export function useUnseenSessions(): UnseenSessions {
  return useSyncExternalStore(subscribe, () => known);
}
