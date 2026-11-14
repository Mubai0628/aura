import type { AgentTeamChangesView, WorktreeChangeKind } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';
import { changesFailure } from './failure-words';
import { size } from '../storage-settings/size';

const KIND: Record<WorktreeChangeKind, string> = {
  added: '新增',
  modified: '修改',
  deleted: '删除',
};

/**
 * What a worktree changed since it started, with the checks and what is left from the newest
 * final report: a sub-agent's (`agent`), or the conversation's own.
 */
export function WorktreeChanges({
  sessionId,
  agent,
}: {
  sessionId: string;
  agent?: string | undefined;
}) {
  const [view, setView] = useState<AgentTeamChangesView>();
  const [problem, setProblem] = useState<string>();
  useEffect(() => {
    let active = true;
    window.aura.v1
      .readAgentTeamChanges({ session_id: sessionId, ...(agent ? { agent } : {}) })
      .then((result) => {
        if (!active) return;
        if (result.ok) {
          setView(result.value);
          setProblem(undefined);
        } else setProblem(changesFailure(result.error));
      })
      .catch((error: unknown) => {
        if (active) setProblem(changesFailure(error, true));
      });
    return () => {
      active = false;
    };
  }, [sessionId, agent]);
  if (problem)
    return (
      <p className="agent-team-problem" role="alert">
        {problem}
      </p>
    );
  if (!view) return <p className="worktree-changes is-loading">正在读改动…</p>;
  return (
    <dl aria-label="工作副本的改动" className="worktree-changes">
      <dt>分支</dt>
      <dd>{view.branch}</dd>
      <dt>改动</dt>
      <dd>
        {view.files.length === 0 ? (
          '没有改动'
        ) : (
          <ul>
            {view.files.map((file) => (
              <li key={file.path}>
                <span className="worktree-change-kind" data-kind={file.kind}>
                  {KIND[file.kind]}
                </span>
                <span className="worktree-change-path">{file.path}</span>
                {file.added != null ? <span className="is-added">+{file.added}</span> : null}
                {file.removed != null ? <span className="is-removed">-{file.removed}</span> : null}
              </li>
            ))}
          </ul>
        )}
      </dd>
      {view.checks?.length ? (
        <>
          <dt>检查</dt>
          <dd>
            <ul>
              {view.checks.map((check) => (
                <li key={check}>{check}</li>
              ))}
            </ul>
          </dd>
        </>
      ) : null}
      {view.remaining ? (
        <>
          <dt>还没做</dt>
          <dd>{view.remaining}</dd>
        </>
      ) : null}
      {view.size_bytes != null ? (
        <>
          <dt>占用</dt>
          <dd>{size(view.size_bytes)}</dd>
        </>
      ) : null}
    </dl>
  );
}
