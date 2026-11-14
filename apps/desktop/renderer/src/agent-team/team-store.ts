import type { AgentTeamView } from '@aura/contracts/generated';
import { useCallback, useSyncExternalStore } from 'react';
import { ACTIVITY_EVERY_MS } from '../session-activity';

/**
 * A conversation's sub-agents as Core lists them, read by one loop per conversation for every
 * view of them: the approvals above the composer, the 多 Agent page and the 近期活动 card. The
 * loop reads again every 1.5 s while a view follows the conversation (a turn is under way, or the
 * page is shown) or a sub-agent still works, and once otherwise.
 */
interface Entry {
  view?: AgentTeamView | undefined;
  /** Each view's listener, and whether that view follows the conversation. */
  listeners: Map<() => void, boolean>;
  timer: number;
  reading: boolean;
  again: boolean;
  queued: boolean;
  /** The last read failed: the views may show an older list; it is read again soon. */
  unread: boolean;
}

/** How long a list that could not be read waits before it is read again. */
const RETRY_MS = 3_000;

const entries = new Map<string, Entry>();
/** Conversations whose last view closed keep their list for an instant show, this many. */
const KEPT = 32;

const working = (view: AgentTeamView | undefined) =>
  view?.members.some((member) => member.state === 'running') ?? false;

function follows(entry: Entry): boolean {
  for (const follow of entry.listeners.values()) if (follow) return true;
  return working(entry.view);
}

function put(session: string, entry: Entry, view: AgentTeamView) {
  if (entry.view !== undefined && JSON.stringify(entry.view) === JSON.stringify(view)) return;
  entry.view = view;
  for (const listener of [...entry.listeners.keys()]) listener();
  // A changed list may start or end the following.
  if (!entry.reading) schedule(session, entry);
}

function schedule(session: string, entry: Entry) {
  window.clearTimeout(entry.timer);
  if (entry.listeners.size && follows(entry))
    entry.timer = window.setTimeout(() => void read(session), ACTIVITY_EVERY_MS);
}

async function read(session: string) {
  const entry = entries.get(session);
  if (!entry) return;
  if (entry.reading) {
    entry.again = true;
    return;
  }
  window.clearTimeout(entry.timer);
  entry.reading = true;
  let fresh: AgentTeamView | undefined;
  let failed = false;
  try {
    const list = window.aura?.v1?.listAgentTeam;
    if (list && !document.hidden) {
      const result = await list({ session_id: session });
      if (result.ok) fresh = result.value;
      else failed = true;
    }
  } catch {
    // The views keep what they showed and say it may be old; the list is read again.
    failed = true;
  } finally {
    entry.reading = false;
  }
  if (entries.get(session) !== entry) return;
  if (entry.unread !== failed) {
    entry.unread = failed;
    for (const listener of [...entry.listeners.keys()]) listener();
  }
  if (fresh) put(session, entry, fresh);
  if (entry.again) {
    entry.again = false;
    void read(session);
  } else if (failed && entry.listeners.size) {
    window.clearTimeout(entry.timer);
    entry.timer = window.setTimeout(() => void read(session), RETRY_MS);
  } else schedule(session, entry);
}

/** Reads once soon: the views that ask in the same moment share one read. */
function soon(session: string, entry: Entry) {
  if (entry.queued) return;
  entry.queued = true;
  void Promise.resolve().then(() => {
    entry.queued = false;
    if (entry.listeners.size) void read(session);
  });
}

function subscribe(session: string, listener: () => void, follow: boolean): () => void {
  let entry = entries.get(session);
  if (!entry) {
    entry = {
      listeners: new Map(),
      timer: 0,
      reading: false,
      again: false,
      queued: false,
      unread: false,
    };
    entries.set(session, entry);
  }
  const kept = entry;
  kept.listeners.set(listener, follow);
  soon(session, kept);
  return () => {
    kept.listeners.delete(listener);
    if (kept.listeners.size) return;
    window.clearTimeout(kept.timer);
    if (entries.size > KEPT)
      for (const [other, value] of entries)
        if (!value.listeners.size && other !== session && entries.size > KEPT)
          entries.delete(other);
  };
}

/**
 * The sub-agents of conversation `sessionId`; `follow` while this view should see changes
 * within a moment. Undefined until the first read.
 */
export function useAgentTeam(
  sessionId: string | undefined,
  follow: boolean,
): AgentTeamView | undefined {
  const listen = useCallback(
    (listener: () => void) =>
      sessionId === undefined ? () => undefined : subscribe(sessionId, listener, follow),
    [sessionId, follow],
  );
  const current = useCallback(
    () => (sessionId === undefined ? undefined : entries.get(sessionId)?.view),
    [sessionId],
  );
  return useSyncExternalStore(listen, current);
}

/** Whether the last read of conversation `sessionId`'s sub-agents failed. */
export function useAgentTeamUnread(sessionId: string): boolean {
  const listen = useCallback(
    (listener: () => void) => subscribe(sessionId, listener, false),
    [sessionId],
  );
  const current = useCallback(() => entries.get(sessionId)?.unread ?? false, [sessionId]);
  return useSyncExternalStore(listen, current);
}

/** Reads the conversation's sub-agents again now: something the user did changed them. */
export function refreshAgentTeam(sessionId: string) {
  const entry = entries.get(sessionId);
  if (entry?.listeners.size) void read(sessionId);
}

/** Shows a list Core answered with, as a read would. */
export function showAgentTeam(sessionId: string, view: AgentTeamView) {
  const entry = entries.get(sessionId);
  if (entry) put(sessionId, entry, view);
}

export function resetAgentTeamsForTests() {
  for (const entry of entries.values()) window.clearTimeout(entry.timer);
  entries.clear();
}
