import { useRef, type SyntheticEvent } from 'react';
import { ExpandIcon, ToolIcon } from '../tool-icon';
import { MessageText } from './MessageText';
import { useDisclosureOpen } from './disclosure-open';
import { useReasoningTail } from './reasoning-tail';

/** Model reasoning stays plain text: it is not a product answer and is never
 *  rendered as Markdown. Only the disclosure direction is shared.
 *
 * A long thought was hard to close — its title had scrolled far away. Open,
 * the title stays at the top of the conversation as the thought scrolls by, and the thought ends
 * with 收起思考过程. Closed either way, its title is brought into view. While it is written, its
 * title sweeps a dim light. */
export function ReasoningTrace({
  sessionId,
  modelStepId,
  preview,
  totalBytes,
  partial,
  generating,
  bodyId,
  sealedRevision,
}: {
  sessionId: string;
  modelStepId: string;
  preview: string;
  totalBytes: number;
  partial: boolean;
  /** False when the turn has stopped but the stored reasoning can still be paged. */
  generating?: boolean;
  bodyId: string | null;
  sealedRevision?: string | null | undefined;
}) {
  const { onToggle, open } = useDisclosureOpen();
  const details = useRef<HTMLDetailsElement>(null);
  const newest = useReasoningTail({ sessionId, modelStepId, preview, totalBytes, partial, open });
  const toggled = (event: SyntheticEvent<HTMLDetailsElement>) => {
    onToggle(event);
    if (!event.currentTarget.open)
      event.currentTarget.querySelector('summary')?.scrollIntoView({ block: 'nearest' });
  };
  return (
    <details className="agent-reasoning-trace" onToggle={toggled} ref={details}>
      <summary data-running={generating || undefined}>
        <ToolIcon className="agent-thinking-mark" name="sparkles" size={14} />
        <span className="agent-thinking-label">思考过程</span>{' '}
        <span className="agent-chevron" aria-hidden="true">
          <ExpandIcon open={open} />
        </span>
        {/* Closed, a line says what the thought is about; its newest
            line, following it while it is written. */}
        {open ? null : <span className="agent-reasoning-preview">{newest}</span>}
      </summary>
      <MessageText
        sessionId={sessionId}
        source={{ kind: 'reasoning', model_step_id: modelStepId }}
        preview={preview}
        totalBytes={totalBytes}
        partial={partial}
        bodyId={bodyId}
        sealedRevision={sealedRevision}
        role="reasoning"
        className="agent-reasoning"
      />
      {open ? (
        <button
          className="agent-reasoning-collapse"
          onClick={() => {
            if (details.current) details.current.open = false;
          }}
          type="button"
        >
          <ToolIcon name="chevrons-down-up" size={13} />
          收起思考过程
        </button>
      ) : null}
    </details>
  );
}
