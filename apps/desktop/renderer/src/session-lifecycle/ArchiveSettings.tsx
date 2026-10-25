import { useState } from 'react';
import { userMessage } from '../agent-session/user-message';
import { useWorkspaceSessions } from '../workspace-session-store';
import { DeleteSessionDialog, DeleteWorkspaceDialog } from './LifecycleDialogs';
import './archive-settings.css';

type Confirm =
  { kind: 'session'; id: string; title: string } | { kind: 'workspace'; id: string; name: string };

/**
 * Settings → 归档: archived Workspaces and conversations,
 * each restored to the sidebar or deleted for good.
 */
export function ArchiveSettings() {
  const projection = useWorkspaceSessions().projection?.payload;
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();
  const [confirm, setConfirm] = useState<Confirm>();
  const workspaces = projection?.workspaces ?? [];
  const archivedWorkspaces = workspaces.filter((workspace) => workspace.state === 'archived');
  const archivedSessions = (projection?.sessions ?? []).filter(
    (session) => session.state === 'archived',
  );
  const sessionCount = (workspaceId: string) =>
    (projection?.sessions ?? []).filter((session) => session.workspace_id === workspaceId).length;
  const restore = async (key: string, run: () => Promise<{ ok: boolean; error?: unknown }>) => {
    setBusy(key);
    setError(undefined);
    try {
      const result = await run();
      if (!result.ok) throw result.error;
    } catch (reason) {
      setError(userMessage(reason, '恢复没有完成，请刷新后查看。'));
    } finally {
      setBusy(undefined);
    }
  };
  return (
    <div className="settings-page archive-settings">
      <div className="settings-content-heading">
        <div>
          <h2>归档</h2>
        </div>
      </div>
      {error ? <p role="alert">{error}</p> : null}
      <section aria-label="已归档的工作区" className="settings-surface">
        <h3>工作区</h3>
        {archivedWorkspaces.length === 0 ? (
          <p className="archive-empty">没有归档的工作区。</p>
        ) : (
          <ul className="archive-list">
            {archivedWorkspaces.map((workspace) => (
              <li key={workspace.workspace_id}>
                <div>
                  <strong>{workspace.display_name}</strong>
                  <code>{workspace.selected_root}</code>
                  <span>{`${sessionCount(workspace.workspace_id)} 个会话`}</span>
                </div>
                <div className="archive-actions">
                  <button
                    type="button"
                    className="button is-small"
                    disabled={busy !== undefined}
                    onClick={() =>
                      void restore(workspace.workspace_id, () =>
                        window.aura.v1.restoreWorkspace({ workspace_id: workspace.workspace_id }),
                      )
                    }
                  >
                    {busy === workspace.workspace_id ? '正在恢复…' : '恢复'}
                  </button>
                  <button
                    type="button"
                    className="button is-small is-danger"
                    disabled={busy !== undefined}
                    onClick={() =>
                      setConfirm({
                        kind: 'workspace',
                        id: workspace.workspace_id,
                        name: workspace.display_name,
                      })
                    }
                  >
                    删除
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
      <section aria-label="已归档的会话" className="settings-surface">
        <h3>会话</h3>
        {archivedSessions.length === 0 ? (
          <p className="archive-empty">没有归档的会话。</p>
        ) : (
          <ul className="archive-list">
            {archivedSessions.map((session) => (
              <li key={session.session_id}>
                <div>
                  <strong>{session.title || '新会话'}</strong>
                  <span>
                    {workspaces.find((w) => w.workspace_id === session.workspace_id)
                      ?.display_name ?? ''}
                  </span>
                </div>
                <div className="archive-actions">
                  <button
                    type="button"
                    className="button is-small"
                    disabled={busy !== undefined}
                    onClick={() =>
                      void restore(session.session_id, () =>
                        window.aura.v1.restoreSession({ session_id: session.session_id }),
                      )
                    }
                  >
                    {busy === session.session_id ? '正在恢复…' : '恢复'}
                  </button>
                  <button
                    type="button"
                    className="button is-small is-danger"
                    disabled={busy !== undefined}
                    onClick={() =>
                      setConfirm({ kind: 'session', id: session.session_id, title: session.title })
                    }
                  >
                    删除
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
      {confirm?.kind === 'session' ? (
        <DeleteSessionDialog session={confirm} onClose={() => setConfirm(undefined)} />
      ) : null}
      {confirm?.kind === 'workspace' ? (
        <DeleteWorkspaceDialog workspace={confirm} onClose={() => setConfirm(undefined)} />
      ) : null}
    </div>
  );
}
