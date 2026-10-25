import type { UserQuery } from '@aura/contracts/generated';
import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * A question nobody touches for 30 s shows a 30 s countdown; when it runs
 * out Core ends the question as "no response" and the task goes on. Clicking the question,
 * choosing, typing — any touch — starts the time again. Core keeps the deadline (it ends the
 * question even when this page is not open); this page tells it about touches and shows the time
 * left. The numbers mirror Core's `QUERY_IDLE_SECONDS` and `QUERY_COUNTDOWN_SECONDS`.
 */
export const QUERY_COUNTDOWN_SECONDS = 30;
const QUERY_IDLE_AND_COUNTDOWN_MS = 60_000;
/** A run of touches reaches Core at most this often; the last one always does. */
const TOUCH_EVERY_MS = 2_000;

export interface QueryCountdown {
  /** Whole seconds before the question ends as "no response"; undefined while unknown. */
  remaining: number | undefined;
  /** The countdown shows once the question has been left alone for its idle time. */
  visible: boolean;
  /** The user touched the question. */
  touch: () => void;
}

const parse = (value: string | null | undefined) => (value ? Date.parse(value) : undefined);

export function useQueryCountdown(
  query: UserQuery,
  sessionId: string,
  deadlineAt: string | null | undefined,
): QueryCountdown {
  // Core's newest deadline is taken, but never one older than a touch made here for this question.
  const [touched, setTouched] = useState<{ query: string; until: number }>();
  const local = touched?.query === query.query_id ? touched.until : undefined;
  const core = parse(deadlineAt);
  const deadline = core === undefined ? local : Math.max(core, local ?? 0);
  const raise = useCallback(
    (until: number) =>
      setTouched((old) => ({
        query: query.query_id,
        until: Math.max(old?.query === query.query_id ? old.until : 0, until),
      })),
    [query.query_id],
  );
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);
  const lastSent = useRef(0);
  const trailing = useRef<number | undefined>(undefined);
  useEffect(
    () => () => {
      if (trailing.current !== undefined) window.clearTimeout(trailing.current);
    },
    [],
  );
  const send = useCallback(() => {
    lastSent.current = Date.now();
    void window.aura.v1
      .touchAgentQuery({
        session_id: sessionId,
        agent_run_id: query.agent_run_id,
        query_id: query.query_id,
      })
      .then((result) => {
        if (result.ok) raise(Date.parse(result.value.deadline_at));
      })
      // A touch that does not arrive only leaves the countdown where Core has it.
      .catch(() => undefined);
  }, [query.agent_run_id, query.query_id, raise, sessionId]);
  const touch = useCallback(() => {
    const at = Date.now();
    raise(at + QUERY_IDLE_AND_COUNTDOWN_MS);
    setNow(at);
    const since = at - lastSent.current;
    if (since >= TOUCH_EVERY_MS) send();
    else
      trailing.current ??= window.setTimeout(() => {
        trailing.current = undefined;
        send();
      }, TOUCH_EVERY_MS - since);
  }, [raise, send]);
  const remaining =
    deadline === undefined ? undefined : Math.max(0, Math.ceil((deadline - now) / 1000));
  return {
    remaining,
    visible: remaining !== undefined && remaining <= QUERY_COUNTDOWN_SECONDS,
    touch,
  };
}
