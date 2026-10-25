import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';

const EMPTY: ReadonlyMap<string, AgentToolActivityEntry> = new Map();
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
) {
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
      try {
        for (let index = 0; index < pending.length; index += 100) {
          const result = await window.aura.v1.readAgentToolActivity({
            session_id: sessionId,
            before_call_id: null,
            call_ids: pending.slice(index, index + 100),
            limit: 100,
          });
          if (!alive) return;
          if (!result.ok || result.value.session_id !== sessionId) break;
          const calls = new Map(cache.current.calls);
          for (const entry of result.value.calls) calls.set(entry.summary.tool_call_id, entry);
          cache.current = { key, calls };
          keep(key, calls);
          setDirectory(cache.current);
        }
      } catch {
        // Do not invent a name or success state when the projection is unavailable.
      }
      // Unfinished calls are asked for again; once the turn is over, a call with no record at all
      // is not waited for any longer.
      const waiting = ids.filter((id) => {
        const known = cache.current.calls.get(id);
        return known ? !known.summary.outcome : !settled;
      });
      if (alive && waiting.length) {
        timer = setTimeout(() => void refresh(), 1500);
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
