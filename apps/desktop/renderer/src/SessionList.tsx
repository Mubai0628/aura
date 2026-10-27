import { AppIcon } from './app-icons';
import { prefetchAgentSession } from './agent-session/useAgentSession';
import { SidebarItemMenu } from './SidebarItemMenu';
import type { AppScreen, WorkspaceNavigationItem } from './app-shell-types';
import type { SessionActivity } from './session-activity';
import { ListSentinel } from './ui/SkeletonRows';
import { useIncrementalList } from './ui/useIncrementalList';

/** A conversation the pointer rests on this long is read ahead (`prefetchAgentSession`). */
const RESTING_MS = 80;
let resting: number | undefined;
const rest = (sessionId: string) => {
  window.clearTimeout(resting);
  resting = window.setTimeout(() => prefetchAgentSession(sessionId), RESTING_MS);
};
const leave = () => window.clearTimeout(resting);

/** What a screen reader hears for a conversation with a turn under way. */
const ACTIVITY_WORDS = { running: '正在运行', waiting: '等待你处理' } as const;

/**
 * A workspace's conversations in the navigation. The first page is drawn at once and the next as the end of the list comes into view; the
 * open conversation is always drawn, after the pages shown when it is beyond them.
 */
export function SessionList({
  activeScreen,
  activity,
  onSelectSession,
  selectedSessionId,
  workspace,
}: {
  activeScreen: AppScreen;
  activity: SessionActivity;
  onSelectSession: (workspaceId: string, sessionId: string) => void;
  selectedSessionId: string;
  workspace: WorkspaceNavigationItem;
}) {
  const open = workspace.sessions.findIndex((session) => session.id === selectedSessionId);
  const pages = useIncrementalList(workspace.sessions, undefined, open);
  const row = (session: (typeof workspace.sessions)[number]) => (
    <li key={session.id} className="session-nav-item">
      <button
        aria-current={
          activeScreen === 'workspace' && selectedSessionId === session.id ? 'page' : undefined
        }
        className={
          activeScreen === 'workspace' && selectedSessionId === session.id
            ? 'session-row is-active'
            : 'session-row'
        }
        onClick={() => onSelectSession(workspace.id, session.id)}
        onFocus={() => prefetchAgentSession(session.id)}
        onPointerEnter={() => rest(session.id)}
        onPointerLeave={leave}
        type="button"
      >
        <span
          className={`session-state state-${session.state}${
            activity.has(session.id) ? ` is-${activity.get(session.id)}` : ''
          }`}
          aria-hidden="true"
        />
        <AppIcon name="chat" size={15} />
        <span>{session.title || '新会话'}</span>
        {activity.has(session.id) ? (
          <span className="sr-only">{ACTIVITY_WORDS[activity.get(session.id)!]}</span>
        ) : null}
      </button>
      <SidebarItemMenu workspace={workspace} session={session} />
    </li>
  );
  return (
    <ul className="session-list">
      {pages.shown.map(row)}
      {pages.more ? <ListSentinel sentinel={pages.sentinel} /> : null}
      {pages.kept ? row(pages.kept) : null}
      {workspace.sessions.length === 0 ? <li className="session-empty">尚未创建会话</li> : null}
    </ul>
  );
}
