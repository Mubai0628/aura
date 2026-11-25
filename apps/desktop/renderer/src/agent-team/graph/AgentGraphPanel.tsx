import type { AgentGraphView } from '@aura/contracts/generated';
import type { ReactNode } from 'react';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { AppIcon } from '../../app-icons';
import { ConfirmationDialog } from '../../ConfirmationDialog';
import { focusAgent } from '../team-focus';
import { AgentGraphCanvas } from './AgentGraphCanvas';
import { GraphMergeDialog, type MergePreview } from './GraphMergeDialog';
import { GraphTeamSection } from './GraphTeamSection';
import { AgentTeamAttention } from '../AgentTeamAttention';
import { mergePositions, visibleGraph } from './graph-layout';
import { graphRevision, useAgentGraph } from './useAgentGraph';
import './agent-graph.css';

type ScopeConfirmation =
  | { kind: 'remove'; session: string; base: AgentGraphView }
  | { kind: 'reset'; base: AgentGraphView };
export function AgentGraphPanel({
  sessionId,
  visible,
  shown,
  chosen,
  fallback,
  onOpenSession,
  onPicked,
  onEditProfile,
  activeSessions,
}: {
  sessionId: string;
  visible: boolean;
  shown: 'canvas' | 'list';
  chosen: string | undefined;
  fallback: ReactNode;
  onOpenSession?: ((session: string) => boolean) | undefined;
  onPicked: () => void;
  onEditProfile: (session: string) => void;
  activeSessions?: ReadonlySet<string> | undefined;
}) {
  const graph = useAgentGraph(sessionId, visible);
  const [merge, setMerge] = useState<MergePreview>();
  const [confirmation, setConfirmation] = useState<ScopeConfirmation>();
  const [preparing, setPreparing] = useState(false);
  const [problem, setProblem] = useState('');
  const alive = useRef(true),
    title = useId();
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const view = graph.view;
  const displayView = useMemo(
    () => (view && activeSessions?.has(sessionId) ? visibleGraph(view, activeSessions) : view),
    [view, activeSessions, sessionId],
  );
  const disabled = graph.busy || graph.uncertain || preparing;
  const add = async (other: string) => {
    if (!view || disabled || (activeSessions?.has(sessionId) && !activeSessions.has(other))) return;
    if (view.graph.sessions.includes(other)) {
      return;
    }
    setPreparing(true);
    setProblem('');
    try {
      const result = await window.aura.v1.readAgentGraph({ session_id: other });
      if (!alive.current) return;
      if (!result.ok || !result.value.graph.sessions.includes(other))
        throw new Error('graph_source');
      if (result.value.graph.workspace_id !== view.graph.workspace_id) {
        setProblem('只能加入同一工作区的普通会话，尚未合并。');
        return;
      }
      setMerge({ base: view, source: result.value, other });
    } catch {
      if (alive.current) setProblem('加入方画布暂时读不到，尚未合并；请重新读取后再试。');
    } finally {
      if (alive.current) setPreparing(false);
    }
  };
  const pick = (parent: string, agent?: string) => {
    if (parent !== sessionId && (!onOpenSession || !onOpenSession(parent))) {
      setProblem('暂时无法打开此会话。');
      return;
    }
    focusAgent(parent, agent);
    onPicked();
  };
  const remove = (session: string) => {
    if (view && !disabled) setConfirmation({ kind: 'remove', session, base: view });
  };
  const names = new Map(view?.profiles.map((profile) => [profile.session_id, profile.name]) ?? []);
  const attention = displayView ? (
    <div className="agent-graph-attention">
      {displayView.teams.map((entry) => (
        <AgentTeamAttention
          key={entry.parent_session_id}
          sessionId={entry.parent_session_id}
          members={entry.team.members}
          onPick={(agent) => pick(entry.parent_session_id, agent)}
        />
      ))}
    </div>
  ) : null;
  const groups = displayView ? (
    <div className="agent-graph-groups">
      {displayView.teams.map((entry) => (
        <GraphTeamSection
          key={entry.parent_session_id}
          entry={entry}
          profile={displayView.profiles.find(
            (profile) => profile.session_id === entry.parent_session_id,
          )}
          currentSession={sessionId}
          chosen={chosen}
          onPick={pick}
          onEditProfile={onEditProfile}
          showAttention={false}
        />
      ))}
    </div>
  ) : null;
  return (
    <section className="agent-graph-panel" aria-label="会话编排">
      {graph.problem || problem ? (
        <p role="alert" className="agent-team-problem">
          {graph.problem || problem}
          {graph.problem ? (
            <button className="button is-small" type="button" onClick={graph.refresh}>
              <AppIcon name="refresh" />
              重新读取
            </button>
          ) : null}
        </p>
      ) : null}
      {displayView && view ? (
        <>
          {attention}
          {shown === 'canvas' ? (
            <AgentGraphCanvas
              key={displayView.graph.graph_id ?? sessionId}
              view={displayView}
              sessionId={sessionId}
              chosen={chosen}
              disabled={disabled}
              change={graph.change}
              onPick={pick}
              onRemove={remove}
              onAdd={(other) => void add(other)}
              onReset={() => setConfirmation({ kind: 'reset', base: view })}
              onRefresh={graph.refresh}
              onEditProfile={onEditProfile}
            />
          ) : (
            groups
          )}
        </>
      ) : (
        fallback
      )}
      {merge ? (
        <GraphMergeDialog
          preview={merge}
          activeSessions={activeSessions}
          busy={graph.busy}
          onCancel={() => setMerge(undefined)}
          onConfirm={() => {
            const preview = merge;
            void graph
              .change(
                {
                  action: 'merge',
                  other_session_id: preview.other,
                  expected_other: graphRevision(preview.source),
                  confirmed: true,
                  positions: mergePositions(preview.base, preview.source),
                },
                graphRevision(preview.base),
              )
              .finally(() => {
                if (alive.current) setMerge(undefined);
              });
          }}
        />
      ) : null}
      {confirmation ? (
        <ConfirmationDialog
          title={confirmation.kind === 'remove' ? '移出会话' : '重新排版当前画布'}
          titleId={title}
          busy={graph.busy}
          confirmLabel={confirmation.kind === 'remove' ? '移出会话' : '重新排版'}
          description={
            confirmation.kind === 'remove'
              ? `将“${names.get(confirmation.session) ?? '这个会话'}”移到独立画布，并移除它的连线。`
              : '重置当前画布的节点位置。'
          }
          onCancel={() => setConfirmation(undefined)}
          onConfirm={() => {
            const current = confirmation;
            void graph
              .change(
                current.kind === 'remove'
                  ? { action: 'remove', session_id: current.session }
                  : { action: 'reset_layout' },
                graphRevision(current.base),
              )
              .finally(() => {
                if (alive.current) setConfirmation(undefined);
              });
          }}
        />
      ) : null}
    </section>
  );
}
