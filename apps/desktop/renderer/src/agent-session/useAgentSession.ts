import { userMessage } from './user-message';
import type { AgentSessionProjection, AgentTurnView } from '@aura/contracts/generated';
import { useCallback, useEffect, useRef, useState } from 'react';

const sessions = new Map<string, AgentSessionProjection>();
/**
 * A
 * conversation opens with its newest turns, read and drawn at once (ten turns with all their steps
 * took 0.8–1.3 s in the owner's longest conversations); earlier turns come a few at a time as the
 * top of the conversation nears (`HistorySentinel`). The polls that follow a running turn read only
 * the newest turns too; what was already read stays.
 */
const NEWEST_TURNS = 2;
const EARLIER_TURNS = 5;
function mergeMessages(older: AgentTurnView, newer: AgentTurnView): AgentTurnView {
  const users = new Map(older.user_messages.map((m) => [m.event_id, m]));
  const assistants = new Map(older.assistant_messages.map((m) => [m.model_step_id, m]));
  for (const message of newer.user_messages) users.set(message.event_id, message);
  for (const message of newer.assistant_messages) assistants.set(message.model_step_id, message);
  return {
    ...newer,
    user_messages: [...users.values()],
    assistant_messages: [...assistants.values()],
    next_messages: older.next_messages ?? null,
  };
}
function remember(view: AgentSessionProjection) {
  sessions.delete(view.session_id);
  sessions.set(view.session_id, view);
  if (sessions.size > 20) {
    const oldest = sessions.keys().next().value;
    if (oldest) sessions.delete(oldest);
  }
}

const reading = new Set<string>();

/**
 * A conversation the pointer rests on, or the keyboard reaches,
 * in the navigation is read ahead — its newest turns, into the window's cache — so a click shows
 * it at once (the first read of a conversation after Core starts took up to 0.6 s in the owner's
 * longest). The opened conversation reads it again as usual.
 */
export function prefetchAgentSession(sessionId: string): void {
  if (!sessionId || sessions.has(sessionId) || reading.has(sessionId)) return;
  reading.add(sessionId);
  void window.aura.v1
    .readAgentHistory({ session_id: sessionId, before_turn_task_id: null, limit: NEWEST_TURNS })
    .then((result) => {
      if (result.ok && result.value.session_id === sessionId && !sessions.has(sessionId))
        remember(result.value);
    })
    .catch(() => undefined)
    .finally(() => reading.delete(sessionId));
}

/** One serialized session read loop. Old sessions and old runtimes cannot merge
 * their delayed responses into the visible conversation. */
export function useAgentSession(sessionId: string) {
  const [projection, setProjection] = useState<AgentSessionProjection | undefined>(() =>
    sessions.get(sessionId),
  );
  const [error, setError] = useState<string>();
  // A conversation the window has already read is shown at once when the view
  // comes back (from 设置, say); the read below refreshes it without saying it is loading.
  const [loading, setLoading] = useState(() => !sessions.has(sessionId));
  const [loadingOlder, setLoadingOlder] = useState(false);
  const current = useRef<AgentSessionProjection | undefined>(sessions.get(sessionId));
  const generation = useRef(0);
  const turnReads = useRef(new Set<string>());
  const [refreshKey, setRefreshKey] = useState(0);
  const refresh = useCallback(() => setRefreshKey((n) => n + 1), []);

  useEffect(() => {
    let alive = true;
    const token = ++generation.current;
    let timer: number;
    const read = async () => {
      try {
        const result = await window.aura.v1.readAgentHistory({
          session_id: sessionId,
          before_turn_task_id: null,
          limit: NEWEST_TURNS,
        });
        if (!alive || generation.current !== token) return;
        if (!result.ok) {
          setError(result.error.message);
          return;
        }
        const next = result.value;
        if (next.session_id !== sessionId) return;
        const previous = current.current;
        if (
          previous?.session_id === sessionId &&
          previous.runtime_instance_id === next.runtime_instance_id
        ) {
          if (BigInt(previous.revision) > BigInt(next.revision)) return;
          if (previous.revision === next.revision) {
            setError(undefined);
            // What a running step writes into a tool call is not stored and does not move the
            // revision: it is taken from every poll.
            const live = withToolInput(previous, next);
            if (live !== previous) {
              current.current = live;
              remember(live);
              setProjection(live);
            }
            return;
          }
          const newest = new Map(next.turns.map((turn) => [turn.turn.turn_task_id, turn]));
          const older = new Map(previous.turns.map((turn) => [turn.turn.turn_task_id, turn]));
          next.turns = next.turns.map((turn) => {
            const old = older.get(turn.turn.turn_task_id);
            return old ? mergeMessages(old, turn) : turn;
          });
          const retained = previous.turns.filter((turn) => !newest.has(turn.turn.turn_task_id));
          next.turns = [...retained, ...next.turns];
          if (retained.length)
            next.next_before_turn_task_id = previous.next_before_turn_task_id ?? null;
        }
        current.current = next;
        remember(next);
        setProjection(next);
        setError(undefined);
      } catch (reason) {
        if (alive) setError(userMessage(reason, '无法读取会话，请检查 Core 连接。'));
      } finally {
        if (alive && generation.current === token) {
          setLoading(false);
          timer = window.setTimeout(() => void read(), current.current?.active ? 250 : 1000);
        }
      }
    };
    void read();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [sessionId, refreshKey]);

  /** Read one page of earlier turns.
   *
   *  Resolves `true` when the page really moved the history, `false` when the
   *  request ran and brought nothing, and `undefined` when nothing was asked
   *  for (no cursor yet, or a read of this page is already in flight). The
   *  caller uses exactly that distinction to decide whether to keep paging
   *  automatically or to offer a retry. */
  const loadOlder = async (): Promise<boolean | undefined> => {
    const base = current.current;
    if (!base?.next_before_turn_task_id || loadingOlder) return undefined;
    const token = generation.current;
    setLoadingOlder(true);
    try {
      const result = await window.aura.v1.readAgentHistory({
        session_id: sessionId,
        before_turn_task_id: base.next_before_turn_task_id,
        limit: EARLIER_TURNS,
      });
      if (
        generation.current !== token ||
        current.current?.runtime_instance_id !== base.runtime_instance_id
      )
        return undefined;
      if (!result.ok) {
        setError(result.error.message);
        return false;
      }
      if (
        result.value.runtime_instance_id !== base.runtime_instance_id ||
        result.value.session_id !== sessionId
      )
        return undefined;
      const rows = new Map<string, AgentTurnView>(
        result.value.turns.map((t) => [t.turn.turn_task_id, t]),
      );
      for (const row of current.current.turns) rows.set(row.turn.turn_task_id, row);
      const next = {
        ...current.current,
        turns: [...rows.values()],
        next_before_turn_task_id: result.value.next_before_turn_task_id ?? null,
      };
      current.current = next;
      remember(next);
      setProjection(next);
      return (
        next.turns.length > base.turns.length ||
        next.next_before_turn_task_id !== base.next_before_turn_task_id
      );
    } catch {
      setError('无法加载更早的消息。');
      return false;
    } finally {
      setLoadingOlder(false);
    }
  };
  /** Read one page of earlier messages inside one turn, with the same reporting
   *  contract as `loadOlder`. */
  const loadTurnOlder = async (id: string): Promise<boolean | undefined> => {
    const base = current.current;
    const cursor = base?.turns.find((t) => t.turn.turn_task_id === id)?.next_messages;
    if (!base || !cursor || turnReads.current.has(id)) return undefined;
    const token = generation.current;
    turnReads.current.add(id);
    try {
      const result = await window.aura.v1.readAgentHistory({
        session_id: sessionId,
        before_turn_task_id: null,
        within_turn: cursor,
        limit: 10,
      });
      if (
        generation.current !== token ||
        current.current?.runtime_instance_id !== base.runtime_instance_id
      )
        return undefined;
      if (!result.ok) {
        setError(result.error.message);
        return false;
      }
      if (
        result.value.session_id !== sessionId ||
        result.value.runtime_instance_id !== base.runtime_instance_id
      )
        return undefined;
      const older = result.value.turns.find((t) => t.turn.turn_task_id === id);
      if (!older) return false;
      const before = current.current.turns.find((t) => t.turn.turn_task_id === id);
      const merged = before ? mergeMessages(older, before) : older;
      const next = {
        ...current.current,
        turns: current.current.turns.map((t) =>
          t.turn.turn_task_id === id ? mergeMessages(older, t) : t,
        ),
      };
      current.current = next;
      remember(next);
      setProjection(next);
      if (!before) return true;
      return (
        merged.user_messages.length > before.user_messages.length ||
        merged.assistant_messages.length > before.assistant_messages.length ||
        merged.next_messages !== before.next_messages
      );
    } catch {
      setError('无法加载该任务的更早消息。');
      return false;
    } finally {
      turnReads.current.delete(id);
    }
  };
  return {
    projection: projection?.session_id === sessionId ? projection : undefined,
    loading,
    error,
    refresh,
    loadOlder,
    loadingOlder,
    loadTurnOlder,
  };
}

/** `previous` with the tool input `next` reports for its running steps; `previous` itself when
 *  nothing changed. */
function withToolInput(
  previous: AgentSessionProjection,
  next: AgentSessionProjection,
): AgentSessionProjection {
  const reported = new Map(
    next.turns.flatMap((turn) =>
      turn.assistant_messages.map((message) => [message.model_step_id, message.tool_input ?? []]),
    ),
  );
  let changed = false;
  const turns = previous.turns.map((turn) => {
    const messages = turn.assistant_messages.map((message) => {
      const input = reported.get(message.model_step_id);
      if (input === undefined || JSON.stringify(input) === JSON.stringify(message.tool_input ?? []))
        return message;
      changed = true;
      return { ...message, tool_input: input };
    });
    return messages.some((message, index) => message !== turn.assistant_messages[index])
      ? { ...turn, assistant_messages: messages }
      : turn;
  });
  return changed ? { ...previous, turns } : previous;
}
