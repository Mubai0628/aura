import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { endedBeforeStart, toolCallStateLabel } from '../tool-call-state-label';
import type { ToolIconTone } from '../tool-icon-map';
import { terminalReasonLabel } from './tool-call-state';

/** The model's question while it waits for the user. Core reports the call unfinished so
 *  the conversation reads it again until the question ends; it waits, nothing runs. */
const WAITING_FOR_USER = 'waiting_for_user';

/** A command call that exited with an error or that Aura stopped: it ran, but did not do what it
 *  was asked, so it is neither a success nor a failure of the call. */
const ENDED = 'ended';
const SHELL = new Set(['shell.powershell', 'shell.cmd']);
const ENDED_BADLY = /^(?:命令报错退出|已停止：)/u;
/** `/init` that wrote nothing because the user was editing the files. */
const NOT_WRITTEN = /^没有写入：/u;
/** A program list asked for while computer control is off: nothing was read. */
const CONTROL_OFF = /^电脑操控没有打开/u;

/** The visual tone of a real Tool state; text always states the same thing. */
export const toneOf = (outcome: string | undefined, status: string): ToolIconTone => {
  if (outcome === WAITING_FOR_USER) return 'waiting';
  if (outcome === ENDED) return 'unknown';
  if (outcome === 'succeeded') return 'success';
  if (outcome === 'failed' || outcome === 'operation_rejected' || outcome === 'cancelled')
    return 'failed';
  if (outcome === 'not_executed' || outcome === 'indeterminate') return 'unknown';
  // Its run ended before it had a result: nothing works on it any more.
  if (status === 'recovery_required') return 'unknown';
  // A call waiting for the user, or for another call to finish with its file, is not at work.
  if (
    status === 'waiting_permission' ||
    status === 'waiting_approval' ||
    status === 'waiting_resource'
  )
    return 'waiting';
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
  // A call its stopped turn ended before anything of it was sent did not run.
  const outcome = unknown
    ? 'indeterminate'
    : endedBeforeStart(call)
      ? 'not_executed'
      : call.outcome?.type;
  // A command that ran to its end succeeded as a call, but one that exited with an error or that
  // Aura stopped did not do what it was asked: it reads as ended, not as a success.
  if (
    outcome === 'succeeded' &&
    SHELL.has(call.display_name) &&
    ENDED_BADLY.test(entry.display.summary)
  )
    return { outcome: ENDED, text: '已结束' };
  if (
    outcome === 'succeeded' &&
    call.display_name === 'workspace_init.complete' &&
    NOT_WRITTEN.test(entry.display.summary)
  )
    return { outcome: ENDED, text: '未写入' };
  if (
    outcome === 'succeeded' &&
    call.display_name === 'computer.apps' &&
    CONTROL_OFF.test(entry.display.summary)
  )
    return { outcome: ENDED, text: '未读取' };
  return { outcome, text: terminalReasonLabel(call) ?? toolCallStateLabel(call) };
};
