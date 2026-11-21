import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { createContext, useEffect, useState } from 'react';

const EMPTY: ReadonlyMap<string, AgentToolActivityEntry> = new Map();
/** A read that failed is tried again after this long, after the turn as well. */
const UNREADABLE_RETRY_MS = 5_000;

interface ReadStatus {
  unreadable: ReadonlySet<string>;
  missing: ReadonlySet<string>;
}

const EMPTY_STATUS: ReadStatus = { unreadable: new Set(), missing: new Set() };
export const ToolRecordReads = createContext<ReadStatus>(EMPTY_STATUS);
interface Directory extends ReadStatus {
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
  const [directory, setDirectory] = useState<Directory>();
  useEffect(() => {
    if (!sessionId || !visible) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let calls = directories.get(key) ?? EMPTY;
    let pending = visible.split(',').filter((id) => !calls.get(id)?.summary.outcome);
    const missing = new Set<string>();
    const refresh = async () => {
      if (!pending.length) return;
      const unreadable = new Set<string>();
      const updated = new Map(calls);
      for (let index = 0; index < pending.length; index += 100) {
        const ids = pending.slice(index, index + 100);
        try {
          const result = await window.aura.v1.readAgentToolActivity({
            session_id: sessionId,
            before_call_id: null,
            call_ids: ids,
            limit: 100,
          });
          if (!alive) return;
          if (result.ok && result.value.session_id === sessionId) {
            for (const entry of result.value.calls) updated.set(entry.summary.tool_call_id, entry);
            for (const id of result.value.unreadable_call_ids ?? []) unreadable.add(id);
            const returned = new Set(result.value.calls.map((entry) => entry.summary.tool_call_id));
            for (const id of ids) {
              if (!returned.has(id)) updated.delete(id);
              if (returned.has(id) || unreadable.has(id)) missing.delete(id);
              else missing.add(id);
            }
            continue;
          }
        } catch {
          if (!alive) return;
        }
        for (const id of ids) {
          unreadable.add(id);
          updated.delete(id);
          missing.delete(id);
        }
      }
      calls = updated;
      keep(key, calls);
      setDirectory({ key, calls, unreadable, missing: new Set(missing) });
      pending = pending.filter(
        (id) =>
          unreadable.has(id) || (!calls.get(id)?.summary.outcome && !(settled && missing.has(id))),
      );
      if (pending.length) {
        timer = setTimeout(() => void refresh(), unreadable.size ? UNREADABLE_RETRY_MS : 1500);
      }
    };
    void refresh();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [sessionId, key, visible, stateToken, settled]);
  return directory?.key === key
    ? directory
    : { key, calls: directories.get(key) ?? EMPTY, ...EMPTY_STATUS };
}
