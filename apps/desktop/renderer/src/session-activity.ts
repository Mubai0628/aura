import type { AgentSessionActivity, AgentSessionActivityState } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';

/** How often the sidebar asks; a task that starts or ends shows within this. */
export const ACTIVITY_EVERY_MS = 1500;

export type SessionActivity = ReadonlyMap<string, AgentSessionActivityState>;

const same = (known: SessionActivity, sessions: readonly AgentSessionActivity[]) =>
  known.size === sessions.length &&
  sessions.every((entry) => known.get(entry.session_id) === entry.state);

/**
 * Which conversations have a turn
 * under way, as Core has them — running, or waiting for the user. Asked again every 1.5 s while
 * the window is shown; a conversation with nothing under way is absent.
 */
export function useSessionActivity(): SessionActivity {
  const [activity, setActivity] = useState<SessionActivity>(() => new Map());
  useEffect(() => {
    const read = window.aura?.v1?.readAgentSessionsActivity;
    if (!read) return;
    let alive = true;
    let timer = 0;
    const look = async () => {
      if (!document.hidden) {
        try {
          const result = await read({});
          if (alive && result.ok) {
            const sessions = result.value.sessions;
            setActivity((known) =>
              same(known, sessions)
                ? known
                : new Map(sessions.map((entry) => [entry.session_id, entry.state])),
            );
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
  }, []);
  return activity;
}
