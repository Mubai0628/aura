import { ChosenSkillsLine } from '../composer-skills/SkillChoice';
import { MessageAttachments } from '../composer-attachments/MessageAttachments';
import { userMessage } from './user-message';
import { agentDiagnosticIdentity } from './diagnostic-identity';
import type {
  AgentInboxProjection,
  AgentSessionBranchResult,
  AgentSessionProjection,
  AgentTurnView,
  AgentToolActivityEntry,
} from '@aura/contracts/generated';
import type { RefObject } from 'react';
import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { isUnconfirmedCommandOutcome } from '../../../shared/ipc-request-failure';
import { BranchDialog } from './BranchDialog';
import { BranchCopying } from './BranchCopying';
import { BranchOrigin } from './BranchOrigin';
import { HistorySentinel } from './HistorySentinel';
import { HookTurnSummary } from './HookTurnSummary';
import { MessageText } from './MessageText';
import { PlanCard } from './PlanCard';
import { ReplyActions, UserCopy } from './ReplyActions';
import { TurnProcess } from './TurnProcess';
import { PendingAdditions } from './PendingAdditions';
import { SendingMessage } from './SendingMessage';
import { endSending, shownIn, useSending } from './sending';
import { useConversationScroll } from './useConversationScroll';
import { useToolCallDirectory } from './useToolCallDirectory';
import { latestToolCompletion } from './tool-completion';
import { TurnSummary } from './TurnSummary';
import { AppIcon } from '../app-icons';
import { AutoContinuedNote, ContinueButton } from './TurnContinuation';
import { continuable } from './turn-continuation';
import { terminalTurn } from './turn-state';

const uncertainRelated = new Set<string>();
export function Conversation({
  sessionId,
  projection,
  viewportRef,
  onChanged,
  loading,
  error,
  loadOlder,
  loadTurnOlder,
  onToolCompleted,
  title = '',
  onBranched,
  onOpenSession,
}: {
  /** The conversation shown, also while its projection loads. */
  sessionId?: string | undefined;
  projection: AgentSessionProjection | undefined;
  viewportRef: RefObject<HTMLDivElement | null>;
  onChanged: () => void;
  loading: boolean;
  error: string | undefined;
  loadOlder: () => Promise<boolean | undefined>;
  loadTurnOlder: (id: string) => Promise<boolean | undefined>;
  onToolCompleted?: ((key: string) => void) | undefined;
  /** This conversation's title, named in the branch dialog. */
  title?: string | undefined;
  /** A branch was made; the caller opens it. Without it no turn offers 新建分支. */
  onBranched?: ((made: AgentSessionBranchResult) => void) | undefined;
  /** Opens another conversation; `false` when it no longer exists. */
  onOpenSession?: ((sessionId: string) => boolean) | undefined;
}) {
  const [branchTurn, setBranchTurn] = useState<string>();
  // A message just sent shows here until the projection has it.
  const shownSession = sessionId ?? projection?.session_id;
  const sending = useSending(shownSession);
  const arrived =
    sending !== undefined &&
    projection?.session_id === shownSession &&
    shownIn(projection, sending);
  useEffect(() => {
    if (arrived && sending?.accepted && shownSession) endSending(shownSession);
  }, [arrived, sending, shownSession]);
  const last = projection?.turns.at(-1);
  const step = last?.assistant_messages.at(-1);
  // A message appended to the running task appears at its end too.
  const appended = (projection?.inbox?.messages ?? []).filter(
    (message) => message.destination.lane === 'addition',
  ).length;
  const contentVersion = `${last?.turn.turn_task_id}:${last?.user_messages.length}:${last?.turn.revision}:${step?.model_step_id}:${step?.stream_sequence}:${step?.partial}:${appended}:${sending && !arrived ? 'sending' : ''}`;
  // Resolve all visible calls, including earlier pages of a long task.
  const visibleCallIds = (projection?.turns ?? []).flatMap((item) =>
    item.assistant_messages.flatMap((message) => message.tool_call_ids),
  );
  const toolCalls = useToolCallDirectory(
    projection?.session_id,
    projection?.runtime_instance_id,
    visibleCallIds,
    `${last?.turn.turn_task_id ?? ''}:${last?.turn.revision ?? ''}:${last?.turn.state.state ?? ''}`,
    !projection?.active,
  );
  const completed = useMemo(() => latestToolCompletion(toolCalls), [toolCalls]);
  useEffect(() => {
    if (completed) onToolCompleted?.(completed);
  }, [completed, onToolCompleted]);
  const { stopFollowing, hasNew, away, follow } = useConversationScroll(
    viewportRef,
    projection?.session_id ?? '',
    projection?.runtime_instance_id,
    contentVersion,
  );
  // Older content is inserted above what the reader is looking at, so the page
  // is read and the scroll position is corrected before the browser paints it:
  // the text on screen never jumps while history is filled in.
  const anchored = (load: () => Promise<boolean | undefined>) => async () => {
    const viewport = viewportRef.current;
    const oldHeight = viewport?.scrollHeight ?? 0;
    stopFollowing();
    const moved = await load();
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => resolve());
    });
    if (viewport && viewport.scrollHeight !== oldHeight) {
      viewport.scrollTop += viewport.scrollHeight - oldHeight;
    }
    return moved;
  };
  return (
    <div
      className="agent-conversation"
      aria-label="会话消息"
      data-session-id={projection?.session_id}
    >
      {loading && !sending ? <p role="status">正在加载会话…</p> : null}
      {error ? <p role="alert">{userMessage(error)}</p> : null}
      {!loading && !error && !projection?.turns.length && !sending ? (
        <div className="agent-empty">
          <h3>开始一个新任务</h3>
          <p>选择模型，输入你的需求。</p>
        </div>
      ) : null}
      {projection?.branch_copying ? <BranchCopying copying={projection.branch_copying} /> : null}
      {projection?.next_before_turn_task_id ? (
        <HistorySentinel
          cursor={projection.next_before_turn_task_id}
          label="更早的消息"
          onLoad={anchored(loadOlder)}
          root={viewportRef}
        />
      ) : null}
      {projection?.turns.map((item, index) => (
        <Fragment key={item.turn.turn_task_id}>
          <Turn
            item={item}
            onBranch={onBranched ? () => setBranchTurn(item.turn.turn_task_id) : undefined}
            toolCalls={toolCalls}
            viewportRef={viewportRef}
            anchored={anchored}
            awaitingContext={
              projection.active?.run.state.state === 'awaiting_context_remediation' &&
              projection.active.run.agent_run_id === item.turn.agent_run_id
            }
            idle={!projection.active}
            last={index === projection.turns.length - 1}
            inbox={projection.inbox}
            onChanged={onChanged}
            loadTurnOlder={loadTurnOlder}
          />
          {/* The line between what a branch carried and its own turns. */}
          {item.imported && !projection.turns[index + 1]?.imported && projection.branch_origin ? (
            <BranchOrigin origin={projection.branch_origin} onOpenSource={onOpenSession} />
          ) : null}
        </Fragment>
      ))}
      {sending && !arrived ? <SendingMessage sending={sending} /> : null}
      {branchTurn && projection && onBranched ? (
        <BranchDialog
          sessionId={projection.session_id}
          title={title}
          turnTaskId={branchTurn}
          onBranched={(made) => {
            setBranchTurn(undefined);
            onBranched(made);
          }}
          onClose={() => setBranchTurn(undefined)}
        />
      ) : null}
      {/* No height of its own, and always there: offering the way back never changes how tall
          the conversation is, which would pull a reader near its end back to it. */}
      <div className="agent-follow-dock">
        {away || hasNew ? (
          <button
            aria-label="回到最新消息"
            className="agent-follow"
            data-new={hasNew ? 'true' : undefined}
            onClick={follow}
            title="回到最新消息"
            type="button"
          >
            <AppIcon name="arrow-down" />
          </button>
        ) : null}
      </div>
    </div>
  );
}

function Turn({
  item,
  onBranch,
  toolCalls,
  viewportRef,
  anchored,
  awaitingContext,
  idle,
  last,
  inbox,
  onChanged,
  loadTurnOlder,
}: {
  item: AgentTurnView;
  onBranch?: (() => void) | undefined;
  toolCalls: ReadonlyMap<string, AgentToolActivityEntry>;
  viewportRef: RefObject<HTMLDivElement | null>;
  anchored: (load: () => Promise<boolean | undefined>) => () => Promise<boolean | undefined>;
  awaitingContext: boolean;
  idle: boolean;
  last: boolean;
  inbox?: AgentInboxProjection | undefined;
  onChanged: () => void;
  loadTurnOlder: (id: string) => Promise<boolean | undefined>;
}) {
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const sending = useRef(false);
  const [needsReview, setNeedsReview] = useState(() =>
    uncertainRelated.has(item.turn.turn_task_id),
  );
  const uncertain = () => {
    uncertainRelated.add(item.turn.turn_task_id);
    setNeedsReview(true);
  };
  const implementPlan = async () => {
    if (sending.current || needsReview) return;
    sending.current = true;
    setBusy(true);
    try {
      const result = await window.aura.v1.startRelatedAgentTurn({
        session_id: item.turn.session_id,
        source_turn_task_id: item.turn.turn_task_id,
        expected_revision: item.turn.revision,
        kind: 'implement_plan',
        goal_budget: null,
      });
      if (!result.ok) {
        setError(result.error.message);
        if (isUnconfirmedCommandOutcome(result.error)) uncertain();
      } else setError(undefined);
      onChanged();
    } catch {
      setError('操作结果未确认，请核对历史。');
      uncertain();
      onChanged();
    } finally {
      sending.current = false;
      setBusy(false);
    }
  };
  const rootMessage = item.user_messages.find((m) => m.event_id === item.turn.root_user_event_id);
  // A natural completion answers with its own ModelStep response; the projection
  // names that step, so the text is rendered from one place only — never by
  // comparing report strings, which never matched a model-rewritten answer.
  const finalSource = item.final_text_source;
  const finalStep =
    finalSource?.kind === 'assistant_step'
      ? item.assistant_messages.find(
          (message) => message.model_step_id === finalSource.model_step_id,
        )
      : undefined;
  // What 复制 under the turn copies: the answer, once the turn has one.
  const answer =
    item.turn.state.state !== 'completed' || item.plan
      ? undefined
      : finalStep
        ? {
            source: { kind: 'assistant' as const, model_step_id: finalStep.model_step_id },
            totalBytes: finalStep.text_total_bytes,
          }
        : {
            source: { kind: 'completion' as const, turn_task_id: item.turn.turn_task_id },
            totalBytes:
              item.completion?.total_bytes ??
              new TextEncoder().encode(item.turn.state.final_report).length,
          };
  return (
    <section className="agent-turn" data-turn-id={item.turn.turn_task_id}>
      {item.next_messages ? (
        <HistorySentinel
          cursor={`${item.next_messages.before_user_event_id ?? ''}:${item.next_messages.before_model_step_id ?? ''}`}
          label="此任务的更早消息"
          onLoad={anchored(() => loadTurnOlder(item.turn.turn_task_id))}
          root={viewportRef}
        />
      ) : null}
      {item.auto_continued ? <AutoContinuedNote view={item.auto_continued} /> : null}
      {rootMessage ? (
        <article className="agent-user-message">
          {rootMessage.total_bytes > 0 || !rootMessage.attachments?.length ? (
            <MessageText
              sessionId={item.turn.session_id}
              source={{ kind: 'user', event_id: rootMessage.event_id }}
              preview={rootMessage.text}
              totalBytes={rootMessage.total_bytes}
              sealedRevision={rootMessage.sealed_revision}
              role="user"
            />
          ) : null}
          <MessageAttachments
            sessionId={item.turn.session_id}
            attachments={rootMessage.attachments ?? []}
          />
          <ChosenSkillsLine
            skills={item.user_messages.flatMap((message) => message.skills ?? [])}
          />
          <UserCopy
            sessionId={item.turn.session_id}
            eventId={rootMessage.event_id}
            totalBytes={rootMessage.total_bytes}
          />
        </article>
      ) : null}
      <TurnProcess
        item={item}
        calls={toolCalls}
        awaitingContext={awaitingContext}
        finalStepId={finalSource?.kind === 'assistant_step' ? finalSource.model_step_id : undefined}
      />
      {inbox ? (
        <PendingAdditions inbox={inbox} turnTaskId={item.turn.turn_task_id} onChanged={onChanged} />
      ) : null}
      {item.plan ? (
        <PlanCard
          plan={item.plan}
          carried={item.imported === true}
          disabled={!idle || busy || needsReview}
          onImplement={() => void implementPlan()}
        />
      ) : null}
      {/* A running Goal is shown by its card in 近期活动, not under the answer. */}
      {item.turn.state.state === 'completed' && !item.plan ? (
        finalStep ? (
          // The final answer comes from the answering ModelStep itself, so it is
          // rendered here once through the existing complete-text route.
          <div className="agent-final-report markdown-body">
            <MessageText
              sessionId={item.turn.session_id}
              source={{ kind: 'assistant', model_step_id: finalStep.model_step_id }}
              preview={finalStep.text}
              totalBytes={finalStep.text_total_bytes}
              sealedRevision={finalStep.sealed_revision}
              role="final"
            />
          </div>
        ) : (
          <div className="agent-final-report markdown-body">
            <MessageText
              sessionId={item.turn.session_id}
              source={{ kind: 'completion', turn_task_id: item.turn.turn_task_id }}
              preview={item.completion?.preview ?? item.turn.state.final_report}
              sealedRevision={item.completion?.digest}
              totalBytes={
                item.completion?.total_bytes ??
                new TextEncoder().encode(item.turn.state.final_report).length
              }
              role="final"
            />
          </div>
        )
      ) : null}
      {item.diagnostic || 'reason' in item.turn.state ? (
        <p role="alert">
          {userMessage(
            item.diagnostic ?? ('reason' in item.turn.state ? item.turn.state.reason : ''),
            '任务暂时中断。过程输出仍可阅读，请在输入框说明下一步。',
          )}
          {(() => {
            const identity = agentDiagnosticIdentity(item.diagnostic);
            return identity ? (
              <small className="agent-diagnostic-identity">{identity}</small>
            ) : null;
          })()}
          {continuable(item, last, idle) ? (
            <ContinueButton item={item} onChanged={onChanged} />
          ) : null}
        </p>
      ) : null}
      <HookTurnSummary item={item} />
      {item.source?.kind === 'implement_plan' ? (
        <small>
          关联计划：{item.source.source_turn_task_id} · 修订 {item.source.expected_revision}
        </small>
      ) : null}
      {needsReview ? (
        <p role="status">
          上次操作结果未确认，请先核对关联任务。
          <button
            type="button"
            onClick={() => {
              uncertainRelated.delete(item.turn.turn_task_id);
              setNeedsReview(false);
              setError(undefined);
            }}
          >
            已核对，允许再次操作
          </button>
        </p>
      ) : null}
      {error ? <p role="alert">{userMessage(error)}</p> : null}
      {answer || terminalTurn(item.turn.state.state) ? (
        // The answer's actions sit
        // just under the line that ends the turn; what the turn took sits at its right.
        <footer className="agent-turn-footer">
          {answer ? (
            <ReplyActions sessionId={item.turn.session_id} {...answer} onBranch={onBranch} />
          ) : null}
          {terminalTurn(item.turn.state.state) ? <TurnSummary item={item} /> : null}
        </footer>
      ) : null}
    </section>
  );
}
