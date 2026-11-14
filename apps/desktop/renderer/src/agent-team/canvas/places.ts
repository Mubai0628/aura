import { useCallback, useSyncExternalStore } from 'react';
import type { Point, Positions } from './layout';

/**
 * Where the user moved agents on a conversation's canvas, and which view of the 多 Agent page they
 * chose. The places last while the app runs; the view is a per-viewer convenience kept in this
 * browser.
 */
const places = new Map<string, Positions>();
const listeners = new Set<() => void>();
const EMPTY: Positions = {};
const VIEW_KEY = 'aura.ui.v1.agent-team-view';

export type TeamView = 'canvas' | 'list';

function changed() {
  for (const listener of [...listeners]) listener();
}

/** Keeps agent `key` of conversation `sessionId` where the user put it. */
export function place(sessionId: string, key: string, at: Point) {
  places.set(sessionId, { ...(places.get(sessionId) ?? EMPTY), [key]: at });
  changed();
}

/** Lets every agent of conversation `sessionId` go back to its own place. */
export function unplaceAll(sessionId: string) {
  if (places.delete(sessionId)) changed();
}

export function usePlaces(sessionId: string): Positions {
  const listen = useCallback((listener: () => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, []);
  const current = useCallback(() => places.get(sessionId) ?? EMPTY, [sessionId]);
  return useSyncExternalStore(listen, current);
}

export function readTeamView(): TeamView {
  try {
    return window.localStorage.getItem(VIEW_KEY) === 'list' ? 'list' : 'canvas';
  } catch {
    return 'canvas';
  }
}

export function writeTeamView(view: TeamView): void {
  try {
    window.localStorage.setItem(VIEW_KEY, view);
  } catch {
    // A choice that cannot be kept is only forgotten.
  }
}

export function resetPlacesForTests() {
  places.clear();
}
