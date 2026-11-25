import { useContext } from 'react';
import { sectionChoice, useTurnChoice, withSectionChoice } from './turn-choices';
import type { AgentToolActivityEntry, AgentTurnView } from '@aura/contracts/generated';
import { StoppingRunContext } from './stopping';
import { MessageText } from './MessageText';
import { MarkdownBlocks } from '../markdown-body';
import { MessageAttachments } from '../composer-attachments/MessageAttachments';
import { MessageReferencesLine } from '../composer-references/MessageReferences';
import { ChosenSkillsLine } from '../composer-skills/SkillChoice';
import { ProcessSection } from './ProcessSection';
import { UserCopy } from './ReplyActions';
import { StepActivity } from './StepActivity';
import { TurnStatus } from './TurnStatus';
import { sectionFailed, turnTimeline, type TimelineBlock } from './turn-timeline';
import { terminalTurn, turnStatus } from './turn-state';
import './turn-process.css';
import { parseQueryReply } from './query/query-reply';
import { QueryReplyCard } from './query/QueryReplyCard';
import { BackupConnectionNote, ContextCleared, ContextCompacted } from './ContextCompacted';

/**
 * A turn in order: one status line; the model's replies between its tools in the
 * conversation; reasoning and calls in sections closed to one line, the newest open while the
 * turn runs and a failed one always open. No step inspection here. Once the turn has completed, its process
 * closes behind the status line, which opens and closes it again; what the user added during the
 * turn stays in view.
 */
export function TurnProcess({
  item,
  calls,
  awaitingContext,
  finalStepId,
}: {
  item: AgentTurnView;
  calls: ReadonlyMap<string, AgentToolActivityEntry>;
  awaitingContext: boolean;
  /** The ModelStep whose response is the turn's final answer, when the projection
   *  names one: its visible text belongs to the final area, not to the timeline. */
  finalStepId?: string | undefined;
}) {
  const turnId = item.turn.turn_task_id;
  const stoppingRun = useContext(StoppingRunContext);
  const stopping = stoppingRun !== undefined && stoppingRun === item.turn.agent_run_id;
  const [chosen, setChosen] = useTurnChoice<Record<string, boolean>>(`${turnId}:sections`, {});
  const [shown, setShown] = useTurnChoice<boolean | undefined>(`${turnId}:shown`, undefined);
  const terminal = terminalTurn(item.turn.state.state);
  const blocks = turnTimeline(item, calls, finalStepId);
  const collapsible = item.turn.state.state === 'completed' && blocks.some(foldsWithProcess);
  const open = !collapsible || (shown ?? false);
  const processId = `process-${item.turn.turn_task_id}`;
  const newest = blocks.findLastIndex((block) => block.kind === 'process');
  const render = (block: TimelineBlock, index: number) => {
    if (block.kind === 'process') {
      const keys = block.parts.map((part) => part.key);
      const sectionOpen =
        sectionChoice(chosen, keys) ??
        (sectionFailed(block.parts, calls) || (!terminal && index === newest));
      return (
        <ProcessSection
          key={block.key}
          item={item}
          parts={block.parts}
          calls={calls}
          open={sectionOpen}
          terminal={terminal}
          onToggle={() => setChosen(withSectionChoice(chosen, keys, !sectionOpen))}
        />
      );
    }
    if (block.kind === 'connection') {
      return block.message.backup_connection ? (
        <BackupConnectionNote key={block.key} name={block.message.backup_connection} />
      ) : null;
    }
    if (block.kind === 'compacted') {
      const { context_compacted: compacted, context_cleared: cleared } = block.message;
      if (compacted) return <ContextCompacted key={block.key} compacted={compacted} />;
      return cleared ? <ContextCleared key={block.key} cleared={cleared} /> : null;
    }
    if (block.kind === 'interim_reply') {
      return (
        <div key={block.key} className="agent-interim-reply" role="group" aria-label="过程回复">
          <MarkdownBlocks className="agent-interim-text" text={block.message.text} />
        </div>
      );
    }
    if (block.kind === 'reply') {
      const message = block.message;
      return (
        <div key={block.key} className="agent-interim-reply" role="group" aria-label="过程回复">
          <div className="agent-interim-text markdown-body">
            <MessageText
              sessionId={item.turn.session_id}
              source={{ kind: 'assistant', model_step_id: message.model_step_id }}
              preview={message.text}
              totalBytes={message.text_total_bytes}
              partial={message.partial}
              bodyId={message.stream_body_id ?? null}
              sealedRevision={message.sealed_revision}
              role="final"
            />
          </div>
        </div>
      );
    }
    // An answer to the model's question reads as the questions and the replies.
    const answered = block.message.origin.origin === 'query_answer';
    const reply = answered ? parseQueryReply(block.message.text) : undefined;
    if (reply) return <QueryReplyCard key={block.key} view={reply} />;
    const { message } = block;
    const label =
      message.origin.origin === 'steer' ? '追加信息' : answered ? '你的回答' : '用户信息';
    return (
      <article
        key={block.key}
        aria-label={label}
        className="agent-user-message agent-steer-message"
      >
        {answered ? <small>{label}</small> : null}
        {message.total_bytes > 0 ? (
          <MessageText
            sessionId={item.turn.session_id}
            source={{ kind: 'user', event_id: message.event_id }}
            preview={message.text}
            totalBytes={message.total_bytes}
            sealedRevision={message.sealed_revision}
            role="user"
          />
        ) : null}
        <MessageAttachments
          sessionId={item.turn.session_id}
          attachments={message.attachments ?? []}
        />
        <MessageReferencesLine references={message.references} />
        <ChosenSkillsLine skills={message.skills} />
        {message.total_bytes > 0 ? (
          <UserCopy
            sessionId={item.turn.session_id}
            eventId={message.event_id}
            totalBytes={message.total_bytes}
          />
        ) : null}
      </article>
    );
  };
  return (
    <div className="agent-process">
      <TurnStatus
        item={item}
        label={turnStatus(item, calls, awaitingContext, stopping)}
        toggle={
          collapsible ? { open, onToggle: () => setShown(!open), controls: processId } : undefined
        }
      />
      <div id={processId} className="agent-process-body">
        {blocks.map((block, index) =>
          open || !foldsWithProcess(block) ? render(block, index) : null,
        )}
      </div>
      <StepActivity item={item} />
    </div>
  );
}

function foldsWithProcess(block: TimelineBlock): boolean {
  return block.kind !== 'user' && block.kind !== 'interim_reply';
}
