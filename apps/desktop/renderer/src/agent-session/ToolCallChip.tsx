import type { ToolCallActivitySummaryView } from '@aura/contracts/generated';
import { toolCallStateLabel } from '../tool-call-state-label';
import { ToolIcon } from '../tool-icon';
import { toolStateIcon, toolTypeIcon } from '../tool-icon-map';

/** Which visual state a Tool call is in, derived from its real projection. */
type ToolChipTone = 'success' | 'failed' | 'waiting' | 'running' | 'unknown';

const toolChipTone = (call: ToolCallActivitySummaryView | undefined): ToolChipTone => {
  if (!call) return 'unknown';
  if (call.side_effect_state === 'unknown') return 'unknown';
  if (call.outcome !== undefined && call.outcome !== null) {
    switch (call.outcome.type) {
      case 'succeeded':
        return 'success';
      case 'operation_rejected':
        return 'failed';
      case 'failed':
        return 'failed';
      case 'cancelled':
        return 'failed';
      case 'not_executed':
        return 'unknown';
      case 'indeterminate':
        return 'unknown';
    }
  }
  switch (call.status) {
    case 'running':
    case 'queued':
    case 'cancelling':
      return 'running';
    case 'waiting_permission':
    case 'waiting_approval':
    case 'waiting_dependency':
    case 'waiting_resource':
    case 'pending_admission':
      return 'waiting';
    case 'recovery_required':
      return 'unknown';
    case 'finalized':
      return 'success';
  }
};

/** Compact Tool chip: a readable Tool name with its real state, never a
 * random result id as the title. The identity stays available in the body. */
export function ToolCallChip({
  call,
  callId,
}: {
  call: ToolCallActivitySummaryView | undefined;
  callId: string;
}) {
  const tone = toolChipTone(call);
  const state = call ? toolCallStateLabel(call) : '状态未在本页记录';
  return (
    <>
      <span className={`tool-chip tool-chip-${tone}`}>
        <ToolIcon className="tool-chip-kind" name={toolTypeIcon(call?.display_name)} size={14} />
        <ToolIcon className="tool-chip-dot" name={toolStateIcon(tone)} size={14} />
        <span className="tool-chip-name">{call?.display_name ?? '工具调用'}</span>
        <span className="tool-chip-state">{state}</span>
      </span>
      {call ? null : <span className="tool-chip-identity">{callId.slice(-8)}</span>}
    </>
  );
}
