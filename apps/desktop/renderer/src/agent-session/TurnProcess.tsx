import { useTurnChoice } from './turn-choices';
import type { AgentToolActivityEntry, AgentTurnView } from '@aura/contracts/generated';
import { MessageText } from './MessageText';
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
import { EnableDebuggingOffer } from '../browser-settings/EnableDebuggingOffer';

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
  const [chosen, setChosen] = useTurnChoice<Record<string, boolean>>(`${turnId}:sections`, {});
  const [shown, setShown] = useTurnChoice<boolean | undefined>(`${turnId}:shown`, undefined);
  const terminal = terminalTurn(item.turn.state.state);
  const blocks = turnTimeline(item, calls, finalStepId);
  // The user's own messages from the turn are not its process: they stay in view.
  const collapsible =
    item.turn.state.state === 'completed' && blocks.some((block) => block.kind !== 'user');
  const open = !collapsible || (shown ?? false);
  const processId = `process-${item.turn.turn_task_id}`;
  // A call the debugging switch
  // stopped puts its button under the status line, in view whether the process is open or folded.
  const debuggingOffer = item.assistant_messages.some((message) =>
    message.tool_call_ids.some((id) => calls.get(id)?.display.offer === 'enable_browser_debugging'),
  );
  const newest = blocks.findLastIndex((block) => block.kind === 'process');
  const render = (block: TimelineBlock, index: number) => {
    if (block.kind === 'process') {
      const sectionOpen =
        chosen[block.key] ?? (sectionFailed(block.parts, calls) || (!terminal && index === newest));
      return (
        <ProcessSection
          key={block.key}
          item={item}
          parts={block.parts}
          calls={calls}
          open={sectionOpen}
          terminal={terminal}
          onToggle={() => setChosen({ ...chosen, [block.key]: !sectionOpen })}
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
    if (block.kind === 'reply') {
      const message = block.message;
      return (
        <div key={block.key} className="agent-interim-reply markdown-body">
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
      );
    }
    // An answer to the model's question reads as the questions and the replies.
    const answered = block.message.origin.origin === 'query_answer';
    const reply = answered ? parseQueryReply(block.message.text) : undefined;
    if (reply) return <QueryReplyCard key={block.key} view={reply} />;
    return (
      <article key={block.key} className="agent-user-message agent-steer-message">
        <small>
          {block.message.origin.origin === 'steer'
            ? '追加信息'
            : answered
              ? '你的回答'
              : '用户信息'}
        </small>
        <MessageText
          sessionId={item.turn.session_id}
          source={{ kind: 'user', event_id: block.message.event_id }}
          preview={block.message.text}
          totalBytes={block.message.total_bytes}
          sealedRevision={block.message.sealed_revision}
          role="user"
        />
        <UserCopy
          sessionId={item.turn.session_id}
          eventId={block.message.event_id}
          totalBytes={block.message.total_bytes}
        />
      </article>
    );
  };
  return (
    <div className="agent-process">
      <TurnStatus
        item={item}
        label={turnStatus(item, calls, awaitingContext)}
        toggle={
          collapsible ? { open, onToggle: () => setShown(!open), controls: processId } : undefined
        }
      />
      {debuggingOffer ? <EnableDebuggingOffer /> : null}
      <div id={processId} className="agent-process-body">
        {blocks.map((block, index) =>
          open || block.kind === 'user' ? render(block, index) : null,
        )}
      </div>
      <StepActivity item={item} />
    </div>
  );
}
