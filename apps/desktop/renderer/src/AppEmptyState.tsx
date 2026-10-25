import { AppIcon } from './app-icons';
import { OceanBackdrop } from './ocean/OceanBackdrop';
import { ProjectionCompatibilityNotice } from './ProjectionCompatibilityNotice';
import { useAddWorkspace } from './workspace-add';
import './app-empty-state.css';

export interface RuntimeNotice {
  title: string;
  message: string;
  tone: 'danger' | 'muted' | 'success' | 'warning';
}

/** "正在启动 Aura Core" with "正在启动 Aura Core…": the second adds nothing, so it is not repeated. */
const repeats = (title: string, message: string) => {
  const plain = (text: string) => text.replace(/[…。.\s]+$/u, '');
  return plain(message) === plain(title);
};

/**
 * The main area when there is no conversation to show, laid out as the new conversation's page
 * without its message box: the one thing to do next — choose a folder as the first workspace,
 * or start a session in the selected one — sits where its title is. Core's state comes first
 * when Core is not ready, with its own words.
 */
export function AppEmptyState({
  loading,
  starting,
  workspaceName,
  error,
  runtimeNotice,
  onCreated,
  onNewSession,
}: {
  loading: boolean;
  /** What Core is doing while it starts; nothing can be chosen until it is ready. */
  starting?: string | undefined;
  workspaceName?: string | undefined;
  error?: string | undefined;
  runtimeNotice?: RuntimeNotice | undefined;
  onCreated: (workspace: string) => void;
  onNewSession?: (() => void) | undefined;
}) {
  const adding = useAddWorkspace(onCreated);
  const ready = runtimeNotice === undefined;
  // While
  // Core starts there is nothing to choose yet, so the page is the sea and what Core is doing.
  if (starting)
    return (
      <section className="app-empty is-starting" aria-label="会话状态">
        <OceanBackdrop />
        <p className="app-starting" role="status">
          {starting}
        </p>
      </section>
    );
  return (
    <section className="conversation-workspace new-session-workspace" aria-label="会话状态">
      <header className="conversation-header">
        <h2>{loading ? '正在加载工作区…' : workspaceName ? '还没有会话' : '欢迎使用 Aura'}</h2>
      </header>
      <div className="new-session-body">
        <OceanBackdrop />
        <div className="app-empty-notices">
          <ProjectionCompatibilityNotice surface="workspace" />
          {runtimeNotice ? (
            <p className="app-empty-notice" data-tone={runtimeNotice.tone} role="alert">
              <strong>{runtimeNotice.title}</strong>
              {repeats(runtimeNotice.title, runtimeNotice.message) ? null : (
                <span> {runtimeNotice.message}</span>
              )}
            </p>
          ) : null}
        </div>
        <div className="new-session-welcome">
          <span>AURA</span>
          <h1>从一个想法开始</h1>
          {error ? <p className="app-empty-lead">{error}</p> : null}
          {loading || error ? null : workspaceName ? (
            onNewSession ? (
              <button
                type="button"
                className="button is-primary app-empty-action"
                disabled={!ready}
                onClick={onNewSession}
              >
                <AppIcon name="plus" size={15} />
                新建会话
              </button>
            ) : null
          ) : (
            <button
              type="button"
              className="button is-primary app-empty-action"
              disabled={!ready || adding.busy}
              aria-busy={adding.busy ? true : undefined}
              onClick={() => void adding.add()}
            >
              <AppIcon name="folder" size={15} />
              选择文件夹创建工作区
            </button>
          )}
          {adding.notice ? (
            <p className="app-empty-result" role={adding.failed ? 'alert' : 'status'}>
              {adding.notice}
            </p>
          ) : null}
        </div>
      </div>
    </section>
  );
}
