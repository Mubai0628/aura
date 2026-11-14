import type { AgentApprovalReview, AgentTeamMemberView } from '@aura/contracts/generated';
import { useRef, useState } from 'react';
import { ApprovalPanel } from '../agent-session/ApprovalPanel';
import { Conversation } from '../agent-session/Conversation';
import { useAgentSession } from '../agent-session/useAgentSession';
import { size } from '../storage-settings/size';
import { PHASE_WORDS, memberPhase } from './agent-team-words';
import { focusAgent } from './team-focus';
import { refreshAgentTeam, showAgentTeam } from './team-store';
import { WorktreeChanges } from './WorktreeChanges';
import './agent-team.css';
import { stopFailure } from './failure-words';

const CLOSED = { merged: '已合回', discarded: '已丢弃' } as const;

/** The answers the main agent or the settings gave this many of its latest questions. */
const REVIEWS_SHOWN = 3;

const ANSWER_WORDS = {
  allow: '允许',
  allow_workspace: '本工作区允许',
  deny: '拒绝',
  ask_user: '交给你',
} as const;

/** Who answered one of its questions, how and why. */
function reviewWords(review: AgentApprovalReview): string | undefined {
  const answered = review.answered;
  if (!answered) return undefined;
  const who = answered.by === 'settings' ? '设置' : '主 Agent 代批';
  return `${who}：${ANSWER_WORDS[answered.answer]} · ${answered.reason}`;
}

/**
 * A sub-agent picked on the 多 Agent page, shown in the main area over its conversation: the way
 * back to the conversation, its state, its whole record in the conversation's own components —
 * its task sheet, what it thought and said, its tool calls and results, its to-do list and its
 * reports — and the approvals it waits for. Read only: the user can stop it, not write to it.
 */
export function AgentRecord({
  parentSessionId,
  parentTitle,
  member,
}: {
  parentSessionId: string;
  parentTitle?: string | undefined;
  member: AgentTeamMemberView;
}) {
  const agent = useAgentSession(member.session_id);
  const viewportRef = useRef<HTMLDivElement>(null);
  const [stopping, setStopping] = useState(false);
  const [problem, setProblem] = useState<string>();
  const [changes, setChanges] = useState(false);
  const worktree = member.worktree;
  const phase = memberPhase(member);
  const reviews = (member.reviews ?? [])
    .map(reviewWords)
    .filter((words) => words !== undefined)
    .slice(-REVIEWS_SHOWN);
  const stop = async () => {
    if (stopping) return;
    setStopping(true);
    setProblem(undefined);
    try {
      const result = await window.aura.v1.stopAgentTeamMember({
        session_id: parentSessionId,
        agent: member.agent,
      });
      if (result.ok) showAgentTeam(parentSessionId, result.value);
      else setProblem(stopFailure(result.error));
    } catch (error) {
      setProblem(stopFailure(error, true));
    } finally {
      setStopping(false);
      refreshAgentTeam(parentSessionId);
      agent.refresh();
    }
  };
  return (
    <section aria-label={`子 Agent 记录：${member.name}`} className="agent-team-record">
      <header className="agent-team-record-head">
        <nav aria-label="会话路径" className="agent-team-record-path">
          <button
            className="agent-team-record-up"
            onClick={() => focusAgent(parentSessionId, undefined)}
            type="button"
          >
            {parentTitle || '新会话'}
          </button>
          <span aria-hidden="true">/</span>
          <h2>{member.name}</h2>
          <small>{member.agent}</small>
        </nav>
        <span className="agent-team-state" data-phase={phase}>
          {PHASE_WORDS[phase]}
        </span>
        {worktree ? (
          <small className="agent-team-branch">
            {worktree.branch}
            {worktree.state === 'active' ? '' : ` · ${CLOSED[worktree.state]}`}
            {worktree.size_bytes != null ? ` · ${size(worktree.size_bytes)}` : ''}
          </small>
        ) : null}
        {worktree?.state === 'active' ? (
          <button
            aria-expanded={changes}
            className="button is-small"
            onClick={() => setChanges((shown) => !shown)}
            type="button"
          >
            看改动
          </button>
        ) : null}
        {member.state === 'stopped' ? null : (
          <button
            className="button is-danger is-small"
            disabled={stopping}
            onClick={() => void stop()}
            type="button"
          >
            {stopping ? '正在停止…' : '停止'}
          </button>
        )}
      </header>
      {problem ? (
        <p className="agent-team-problem" role="alert">
          {problem}
        </p>
      ) : null}
      {changes && worktree?.state === 'active' ? (
        <div className="agent-team-record-changes">
          <WorktreeChanges agent={member.agent} sessionId={parentSessionId} />
        </div>
      ) : null}
      <div className="agent-team-record-scroll" ref={viewportRef}>
        <div className="conversation-column">
          <Conversation
            readOnly
            sessionId={member.session_id}
            projection={agent.projection}
            viewportRef={viewportRef}
            onChanged={agent.refresh}
            loading={agent.loading}
            error={agent.error}
            loadOlder={agent.loadOlder}
            loadTurnOlder={agent.loadTurnOlder}
          />
        </div>
      </div>
      {reviews.length ? (
        <ul aria-label="代批记录" className="agent-team-reviews">
          {reviews.map((words, index) => (
            <li key={`${index}:${words}`}>{words}</li>
          ))}
        </ul>
      ) : null}
      {member.approvals.length ? (
        <div className="agent-team-record-dock">
          {member.approvals.map((id) => (
            <ApprovalPanel
              key={id}
              sessionId={member.session_id}
              approvalId={id}
              from={`子 Agent · ${member.name}`}
              onChanged={() => {
                refreshAgentTeam(parentSessionId);
                agent.refresh();
              }}
            />
          ))}
        </div>
      ) : null}
    </section>
  );
}
