import type { AgentToolActivityEntry, AgentTurnView } from '@aura/contracts/generated';
import { ExpandIcon } from '../tool-icon';
import { ReasoningTrace } from './reasoning-trace';
import { ActivitySession } from './activity-session';
import { ToolActivity } from './ToolActivity';
import { callUnderWay } from './tool-call-tone';
import { groupConsecutiveToolCalls, isGroupedRun } from './tool-call-group';
import { ToolCallRun } from './tool-run-row';
import { sectionCallIds, sectionLabel, type ProcessPart } from './turn-timeline';

/** One stretch of reasoning and calls between the turn's replies, closed to one line. */
export function ProcessSection({
  item,
  parts,
  calls,
  open,
  terminal,
  onToggle,
}: {
  item: AgentTurnView;
  parts: ProcessPart[];
  calls: ReadonlyMap<string, AgentToolActivityEntry>;
  open: boolean;
  terminal: boolean;
  onToggle: () => void;
}) {
  const id = `process-${item.turn.turn_task_id}-${parts[0]!.key}`;
  const count = sectionCallIds(parts).length;
  // Closed, the section still shows that something inside it is under way.
  const underWay =
    !open &&
    !terminal &&
    parts.some((part) =>
      part.kind === 'reasoning'
        ? part.message.partial
        : part.callIds.some((callId) => callUnderWay(calls.get(callId))),
    );
  return (
    <ActivitySession.Provider value={item.turn.session_id}>
      <section className="agent-process-section">
        <button
          type="button"
          className="agent-process-toggle"
          aria-expanded={open}
          aria-controls={id}
          data-running={underWay || undefined}
          onClick={onToggle}
        >
          <span className="agent-chevron" data-open={open} aria-hidden="true">
            <ExpandIcon open={open} />
          </span>
          <span className="agent-process-label">{sectionLabel(parts, calls)}</span>
          {count > 1 ? <span className="agent-process-count">{count} 次工具调用</span> : null}
        </button>
        <ol className="agent-steps" id={id} hidden={!open}>
          {parts.map((part) => (
            <li
              key={part.key}
              className="agent-process-step"
              data-step-id={part.message.model_step_id}
            >
              {part.kind === 'reasoning' ? (
                <ReasoningTrace
                  sessionId={item.turn.session_id}
                  modelStepId={part.message.model_step_id}
                  preview={part.message.reasoning}
                  totalBytes={part.message.reasoning_total_bytes}
                  partial={part.message.partial}
                  generating={part.message.partial && !terminal}
                  bodyId={part.message.stream_body_id ?? null}
                  sealedRevision={part.message.sealed_revision}
                />
              ) : (
                groupConsecutiveToolCalls(
                  part.callIds.map((callId) => ({ callId, entry: calls.get(callId) })),
                ).map((group) =>
                  isGroupedRun(group) ? (
                    <ToolCallRun key={group.key} group={group} calls={calls} />
                  ) : (
                    group.callIds.map((callId) => (
                      <ToolActivity key={callId} entry={calls.get(callId)} ended={terminal} />
                    ))
                  ),
                )
              )}
            </li>
          ))}
        </ol>
      </section>
    </ActivitySession.Provider>
  );
}
