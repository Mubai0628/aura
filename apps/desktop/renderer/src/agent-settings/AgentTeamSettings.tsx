import type {
  AgentTeamLimits,
  AgentTeamPermissions,
  AgentTeamSettingsView,
} from '@aura/contracts/generated';
import { useEffect, useState } from 'react';
import { userMessage } from '../agent-session/user-message';
import { AgentTeamPermissionsForm } from './AgentTeamPermissionsForm';
import { AgentTeamLimitsForm } from './AgentTeamLimitsForm';
import { PeerCollaborationSettings } from './PeerCollaborationSettings';
import { toDraft, type Draft } from './team-limits-form';
import './agent-team-settings.css';

const UNREADABLE = '读不到多 Agent 的设置，请重试。';
const NOT_SAVED = '多 Agent 的设置没有保存，请重试。';
const UNCONFIRMED = '保存结果尚未确认，请先重新读取核对，不要重复保存。';
type Read = { view: AgentTeamSettingsView } | { problem: string };
const readSettings = async (): Promise<Read> => {
  try {
    const result = await window.aura.v1.readAgentTeamSettings();
    return result.ok ? { view: result.value } : { problem: userMessage(result.error, UNREADABLE) };
  } catch {
    return { problem: UNREADABLE };
  }
};

export function AgentTeamSettings({ coreReady }: { coreReady: boolean }) {
  const [view, setView] = useState<AgentTeamSettingsView>();
  const [draft, setDraft] = useState<Draft>();
  const [problem, setProblem] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const show = (next: AgentTeamSettingsView, keepLimits = false) => {
    setView(next);
    if (!keepLimits) setDraft(toDraft(next.limits));
  };
  useEffect(() => {
    if (!coreReady) return;
    let live = true;
    void readSettings().then((read) => {
      if (!live) return;
      if ('view' in read) {
        setView(read.view);
        setDraft(toDraft(read.view.limits));
        setProblem(undefined);
        setUncertain(false);
      } else setProblem(read.problem);
    });
    return () => {
      live = false;
    };
  }, [coreReady]);
  const retry = async () => {
    const read = await readSettings();
    if ('view' in read) {
      show(read.view);
      setProblem(undefined);
      setUncertain(false);
    } else setProblem(read.problem);
  };
  const save = async (limits: AgentTeamLimits, permissions?: AgentTeamPermissions) => {
    if (view === undefined || busy || uncertain || !coreReady) return;
    setBusy(true);
    try {
      const result = await window.aura.v1.setAgentTeamSettings({
        limits,
        ...(permissions === undefined ? {} : { permissions }),
        expected_revision: view.revision,
      });
      if (result.ok) {
        show(result.value, permissions !== undefined);
        setProblem(undefined);
        return;
      }
      setProblem(userMessage(result.error, NOT_SAVED));
      if (result.error.message.includes('agents.limits_changed')) {
        const again = await readSettings();
        if ('view' in again) show(again.view);
      }
    } catch {
      setProblem(UNCONFIRMED);
      setUncertain(true);
    } finally {
      setBusy(false);
    }
  };
  const ready = coreReady && view !== undefined && draft !== undefined && !busy && !uncertain;
  return (
    <div className="settings-page agent-team-settings">
      <div className="settings-content-heading">
        <div>
          <h2>多 Agent</h2>
        </div>
      </div>
      {coreReady ? null : (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，多 Agent 设置暂不可用。
        </p>
      )}
      <AgentTeamLimitsForm
        view={view}
        draft={draft}
        disabled={!ready}
        onDraft={setDraft}
        onSave={(limits) => void save(limits)}
        onDefault={() => {
          if (view) void save(view.defaults);
        }}
      />
      <PeerCollaborationSettings coreReady={coreReady} />
      <AgentTeamPermissionsForm
        defaults={view?.permission_defaults}
        disabled={!ready}
        onSave={(permissions) => {
          if (view) void save(view.limits, permissions);
        }}
        permissions={view?.permissions}
      />
      {problem ? (
        <p className="settings-status-line" role="alert">
          {problem}
          {view === undefined || uncertain ? (
            <button
              className="button is-small"
              disabled={busy}
              onClick={() => void retry()}
              type="button"
            >
              {uncertain ? '重新读取' : '重试'}
            </button>
          ) : null}
        </p>
      ) : null}
    </div>
  );
}
