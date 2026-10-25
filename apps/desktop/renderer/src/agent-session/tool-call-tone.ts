import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { toolCallStateLabel } from '../tool-call-state-label';
import type { ToolIconTone } from '../tool-icon-map';
import { terminalReasonLabel } from './tool-call-state';

/** The model's question while it waits for the user. Core reports the call unfinished so
 *  the conversation reads it again until the question ends; it waits, nothing runs. */
const WAITING_FOR_USER = 'waiting_for_user';

/** The visual tone of a real Tool state; text always states the same thing. */
export const toneOf = (outcome: string | undefined, status: string): ToolIconTone => {
  if (outcome === WAITING_FOR_USER) return 'waiting';
  if (outcome === 'succeeded') return 'success';
  if (outcome === 'failed' || outcome === 'operation_rejected' || outcome === 'cancelled')
    return 'failed';
  if (outcome === 'not_executed' || outcome === 'indeterminate') return 'unknown';
  if (status === 'waiting_permission' || status === 'waiting_approval') return 'waiting';
  return 'running';
};

/** A call still under way — not finished and not waiting for the user —
 *  sweeps a dim light across its row. */
export function callUnderWay(entry: AgentToolActivityEntry | undefined): boolean {
  if (!entry) return false;
  const { outcome } = stateOf(entry);
  return toneOf(outcome, entry.summary.status) === 'running';
}

/** The explicit state text of one real call.
 *
 * It uses the same classification the group header, the chip and the detail view use:
 * the reason comes from the projection's error class and not-executed reason, never
 * from the broad outcome — four `fs.not_found` refusals were shown as 已拒绝 that way.
 */
export const stateOf = (
  entry: AgentToolActivityEntry,
): { outcome: string | undefined; text: string } => {
  const call = entry.summary;
  if (call.display_name === 'query.user_question' && !call.outcome)
    return { outcome: WAITING_FOR_USER, text: '等待你的回答' };
  const unknown = call.side_effect_state === 'unknown';
  const outcome = unknown ? 'indeterminate' : call.outcome?.type;
  return { outcome, text: terminalReasonLabel(call) ?? toolCallStateLabel(call) };
};
