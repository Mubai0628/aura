import { useState } from 'react';
import { AgentToolGroups } from './AgentToolGroups';
import { userMessage } from '../agent-session/user-message';
import type { ConfigurationOwner } from './useAgentConfiguration';
import { SkeletonRows } from '../ui/SkeletonRows';
import './agent-tools.css';

/**
 * The default-tool selection.
 *
 * The committed snapshot and its revision belong to `useAgentConfiguration`, so
 * this component only holds the draft selection the user is editing. Saving
 * therefore cannot submit a revision that the execution-strategy editor has
 * already moved past.
 *
 * The draft and the committed value it was built from are kept apart: a snapshot
 * published by the *other* editor is not this editor's business, so an unsaved
 * selection survives it. Only this editor's own successful save — or a snapshot
 * that arrived while nothing here was unsaved — moves the draft.
 */
export function AgentToolList({ configuration }: { configuration: ConfigurationOwner }) {
  const { view, loadError, busy, commit, reload } = configuration;
  // `undefined` means "nothing unsaved here": the selection on screen is then the
  // shared snapshot itself. A snapshot published by the *other* editor is not this
  // editor's business, so it cannot clear a selection being edited here; only this
  // editor's own save drops the draft. Deriving the shown value instead of copying
  // the snapshot into state from an effect is what keeps the two from racing.
  const [draft, setDraft] = useState<string[]>();
  const [filter, setFilter] = useState('');
  const [busyLocal, setBusyLocal] = useState(false);
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);

  const committed = view?.fixed_tool_ids ?? [];
  const fixed = draft ?? committed;
  const changed = view !== undefined && JSON.stringify(fixed) !== JSON.stringify(committed);
  const pending = busy || busyLocal;

  const save = async (restoreRecommended = false) => {
    if (view === undefined || pending) return;
    setBusyLocal(true);
    setSaved(false);
    setError(undefined);
    const outcome = await commit(
      restoreRecommended ? { use_recommended_fixed_tool_ids: true } : { fixed_tool_ids: fixed },
    );
    if (outcome.error !== undefined) setError(outcome.error);
    else {
      setSaved(true);
      // This editor's own save is what replaces its draft — including a restore,
      // where the value comes from Core rather than from what the user had typed.
      // The shared owner has already adopted the snapshot the save returned.
      setDraft(undefined);
    }
    setBusyLocal(false);
  };

  // A failed read must be visible and recoverable; an endless "loading" state
  // would hide the failure entirely.
  if (view === undefined) {
    return (
      <section className="agent-tool-settings" aria-label="会话工具配置">
        <div className="agent-tool-settings-heading">
          <div>
            <h3>会话工具</h3>
          </div>
        </div>
        {loadError === undefined ? (
          <SkeletonRows label="正在读取工具" />
        ) : (
          <>
            <p role="alert">{userMessage(loadError)}</p>
            <button type="button" className="button" onClick={() => void reload()}>
              重新读取工具配置
            </button>
          </>
        )}
      </section>
    );
  }

  const tools = view.tools.filter((tool) =>
    `${tool.name} ${tool.description}`.toLowerCase().includes(filter.toLowerCase()),
  );
  return (
    <section className="agent-tool-settings" aria-label="会话工具配置">
      <div className="agent-tool-settings-heading">
        <div>
          <h3>会话工具</h3>
        </div>
        <div className="settings-page-actions">
          <button
            type="button"
            className="button is-ghost"
            disabled={pending}
            onClick={() => void save(true)}
          >
            恢复推荐默认值
          </button>
          <button
            type="button"
            className="button is-primary"
            disabled={!changed || pending}
            onClick={() => void save()}
          >
            {pending ? '保存中…' : '保存配置'}
          </button>
        </div>
      </div>
      <div className="agent-tool-settings-controls">
        <label>
          搜索工具
          <input
            type="search"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="名称或用途"
          />
        </label>
      </div>
      {error ? (
        <p className="settings-status-line" role="alert">
          {userMessage(error)}
        </p>
      ) : null}
      {saved ? (
        <p className="settings-status-line is-done" role="status">
          工具配置已保存。
        </p>
      ) : null}
      <AgentToolGroups
        tools={tools}
        fixed={fixed}
        pending={pending}
        onToggle={(tool, checked) => {
          setSaved(false);
          setDraft(
            checked ? [...fixed, tool.tool_id].sort() : fixed.filter((id) => id !== tool.tool_id),
          );
        }}
      />
      {tools.length === 0 ? <p className="settings-status-line">没有匹配的工具。</p> : null}
    </section>
  );
}
