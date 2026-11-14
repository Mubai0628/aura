import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { createContext, useEffect, useRef, useState } from 'react';

const EMPTY: ReadonlyMap<string, AgentToolActivityEntry> = new Map();
/** A read that failed is tried again after this long, after the turn as well. */
const UNREADABLE_RETRY_MS = 5_000;

/** Whether the conversation's call records could not be read the last time they were asked for:
 *  a call without a record is then of unknown state, not one that never ran. */
export const ToolCallsUnreadable = createContext(false);
interface Directory {
  key: string;
  calls: ReadonlyMap<string, AgentToolActivityEntry>;
}

/**
 * What the window has read, by Core instance and conversation, kept by the window
 * rather than by one view of it. The oldest conversations are forgotten first.
 */
const directories = new Map<string, ReadonlyMap<string, AgentToolActivityEntry>>();
const REMEMBERED = 20;

/** Tests start from a window that has read nothing. */
export function forgetToolCallDirectories(): void {
  directories.clear();
}

function keep(key: string, calls: ReadonlyMap<string, AgentToolActivityEntry>): void {
  directories.delete(key);
  directories.set(key, calls);
  if (directories.size > REMEMBERED) directories.delete(directories.keys().next().value!);
}

/** Fetch visible identities; terminal calls are stable. Scope includes the Core
 * instance. Unfinished calls refresh independently of streamed characters. `settled`: the
 * conversation's latest turn is over, so a call Core still has no record of never ran. */
export function useToolCallDirectory(
  sessionId: string | undefined,
  runtimeInstanceId: string | undefined,
  visibleCallIds: readonly string[],
  stateToken: string | undefined,
  settled = false,
  onUnreadable?: (unreadable: boolean) => void,
) {
  const told = useRef(onUnreadable);
  useEffect(() => {
    told.current = onUnreadable;
  });
  const key = `${runtimeInstanceId ?? ''}:${sessionId ?? ''}`;
  const visible = [...new Set(visibleCallIds)].join(',');
  const cache = useRef<Directory>({ key, calls: directories.get(key) ?? new Map() });
  const [directory, setDirectory] = useState<Directory>();
  useEffect(() => {
    if (!sessionId || !visible) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    if (cache.current.key !== key)
      cache.current = { key, calls: directories.get(key) ?? new Map() };
    const ids = visible.split(',');
    const refresh = async () => {
      const pending = ids.filter((id) => !cache.current.calls.get(id)?.summary.outcome);
      let failed = false;
      try {
        for (let index = 0; index < pending.length; index += 100) {
          const result = await window.aura.v1.readAgentToolActivity({
            session_id: sessionId,
            before_call_id: null,
            call_ids: pending.slice(index, index + 100),
            limit: 100,
          });
          if (!alive) return;
          if (!result.ok) {
            failed = true;
            break;
          }
          if (result.value.session_id !== sessionId) break;
          const calls = new Map(cache.current.calls);
          for (const entry of result.value.calls) calls.set(entry.summary.tool_call_id, entry);
          cache.current = { key, calls };
          keep(key, calls);
          setDirectory(cache.current);
        }
      } catch {
        // Do not invent a name or success state when the projection is unavailable.
        failed = true;
      }
      if (!alive) return;
      told.current?.(failed);
      // Unfinished calls are asked for again; once the turn is over, a call with no record at all
      // is not waited for any longer, unless the records could not be read.
      const waiting = ids.filter((id) => {
        const known = cache.current.calls.get(id);
        return known ? !known.summary.outcome : !settled || failed;
      });
      if (waiting.length) {
        timer = setTimeout(() => void refresh(), failed ? UNREADABLE_RETRY_MS : 1500);
      }
    };
    void refresh();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [sessionId, key, visible, stateToken, settled]);
  return directory?.key === key ? directory.calls : (directories.get(key) ?? EMPTY);
}
