import { useEffect, useState } from 'react';
import { stateOf } from '../agent-session/tool-call-tone';

export interface RecentAction {
  id: string;
  title: string;
  target?: string | undefined;
  at?: string | undefined;
  failed: boolean;
  /** What happened to a call that did not succeed, in the row's own words. */
  state?: string | undefined;
}

/** The conversation's last three tool calls, newest first, read again on `refreshKey`. */
export function useRecentActions(session: string | undefined, refreshKey: string): RecentAction[] {
  const [actions, setActions] = useState<{ session?: string; list: RecentAction[] }>({ list: [] });
  useEffect(() => {
    if (!session) return;
    let alive = true;
    void window.aura.v1
      .readAgentToolActivity({ session_id: session, before_call_id: null, limit: 3 })
      .then((result) => {
        if (!alive || !result.ok) return;
        setActions({
          session,
          list: result.value.calls
            .filter((call) => !call.display.internal)
            .map((call) => ({
              id: call.summary.tool_call_id,
              title: call.display.title,
              target: call.display.target ?? undefined,
              at: call.summary.completed_at ?? call.summary.started_at ?? undefined,
              failed: call.summary.outcome != null && call.summary.outcome.type !== 'succeeded',
              state:
                call.summary.outcome != null && call.summary.outcome.type !== 'succeeded'
                  ? stateOf(call).text
                  : undefined,
            })),
        });
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [session, refreshKey]);
  // Another conversation's calls are never shown, even before its own are read.
  return actions.session === session ? actions.list : [];
}
