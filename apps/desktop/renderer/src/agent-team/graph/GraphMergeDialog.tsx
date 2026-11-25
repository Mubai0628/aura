import type { AgentGraphView } from '@aura/contracts/generated';
import { useId } from 'react';
import { ConfirmationDialog } from '../../ConfirmationDialog';

export interface MergePreview {
  base: AgentGraphView;
  source: AgentGraphView;
  other: string;
}
export function GraphMergeDialog({
  preview,
  activeSessions,
  busy,
  onCancel,
  onConfirm,
}: {
  preview: MergePreview;
  activeSessions?: ReadonlySet<string> | undefined;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const title = useId();
  const groups = [preview.base, preview.source];
  return (
    <ConfirmationDialog
      title="合并编排画布"
      titleId={title}
      busy={busy}
      confirmLabel="合并画布"
      description="以下两张画布的全部会话将合并。"
      onCancel={onCancel}
      onConfirm={onConfirm}
      details={
        <div className="agent-graph-merge-scope">
          {groups.map((view, index) => {
            const shown = activeSessions
              ? view.profiles.filter((profile) => activeSessions.has(profile.session_id))
              : view.profiles;
            const archived = view.profiles.length - shown.length;
            return (
              <section key={index}>
                <h3>{index === 0 ? '当前画布' : '加入方画布'}</h3>
                <ul>
                  {shown.map((profile) => (
                    <li key={profile.session_id}>{profile.name}</li>
                  ))}
                  {archived > 0 ? <li>归档会话 {archived} 个</li> : null}
                </ul>
              </section>
            );
          })}
        </div>
      }
    />
  );
}
