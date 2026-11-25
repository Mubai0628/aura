import type { AgentSessionActivity, AgentSessionActivityState } from '@aura/contracts/generated';
import { useSyncExternalStore } from 'react';

/** How often the window asks; a task that starts or ends shows within this. */
export const ACTIVITY_EVERY_MS = 1500;

export type SessionActivity = ReadonlyMap<string, AgentSessionActivityState>;

const NOTHING: SessionActivity = new Map();

const same = (known: SessionActivity, sessions: readonly AgentSessionActivity[]) =>
  known.size === sessions.length &&
  sessions.every((entry) => known.get(entry.session_id) === entry.state);

// One poll for every part of the window that shows activity.
let known: SessionActivity = NOTHING;
const listeners = new Set<() => void>();
let stop: (() => void) | undefined;

function start(): () => void {
  const read = window.aura?.v1?.readAgentSessionsActivity;
  if (!read) return () => undefined;
  let alive = true;
  let timer = 0;
  const look = async () => {
    if (!document.hidden) {
      try {
        const result = await read({});
        if (alive && result.ok) {
          const next = new Map(
            result.value.sessions.map((entry) => [entry.session_id, entry.state]),
          );
          if (!same(known, result.value.sessions)) {
            known = next;
            for (const listener of listeners) listener();
          }
        }
      } catch {
        // The next look tries again; the dots keep what they showed.
      }
    }
    if (alive) timer = window.setTimeout(() => void look(), ACTIVITY_EVERY_MS);
  };
  void look();
  return () => {
    alive = false;
    window.clearTimeout(timer);
  };
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (listeners.size === 1) stop = start();
  return () => {
    listeners.delete(listener);
    if (listeners.size > 0) return;
    stop?.();
    stop = undefined;
    known = NOTHING;
  };
}

/**
 * Which conversations have a turn under way, as Core has them — running, or waiting for the user
 * (an approval, an answer, a secure input). Asked again every 1.5 s while the window is shown; a
 * conversation with nothing under way is absent.
 */
export function useSessionActivity(): SessionActivity {
  return useSyncExternalStore(subscribe, () => known);
}
