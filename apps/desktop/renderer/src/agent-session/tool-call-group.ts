import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import {
  toolCallFinalStateKey,
  toolCallIdentity,
  toolCallState,
  toolCallTone,
  type ToolCallTone,
} from './tool-call-state';

/** One visible call as it appears inside a step, in real call order. */
export interface VisibleToolCall {
  callId: string;
  entry: AgentToolActivityEntry | undefined;
}

/** Adjacent calls of one step that share the exact Tool definition and final state. */
export interface ToolCallGroup {
  /** Stable member identities: never an array index. */
  key: string;
  callIds: string[];
  /** The exact Tool definition shared by the run; empty when not yet readable. */
  identity: string;
  /** The shared explicit final state; empty while the run is still open. */
  finalStateKey: string;
  /** `status:running` while consecutive open calls of one Tool may share a row. */
  openKey: string;
  displayName: string;
  /** The first member's program-derived title, e.g. 列出目录. */
  title: string;
  tone: ToolCallTone;
  state: string;
}

export const isGroupedRun = (group: ToolCallGroup): boolean => group.callIds.length > 1;

/** Group genuinely consecutive calls of one step.
 *
 * A finished run collapses only when every member shares the exact Tool
 * definition and the same explicit final state. Consecutive calls that are
 * still `running` may share one row so the header can show the live targets;
 * queued, waiting, unknown and missing projections stay separate, and a success
 * never shares a row with a failure. Each member keeps its own identity.
 */
export function groupConsecutiveToolCalls(calls: readonly VisibleToolCall[]): ToolCallGroup[] {
  const groups: ToolCallGroup[] = [];
  for (const call of calls) {
    const entry = call.entry;
    const previous = groups.at(-1);
    if (previous !== undefined && entry !== undefined && joinsRun(previous, entry)) {
      previous.callIds.push(call.callId);
      continue;
    }
    const finalStateKey = entry === undefined ? undefined : toolCallFinalStateKey(entry);
    groups.push({
      key: call.callId,
      callIds: [call.callId],
      identity: entry === undefined ? '' : toolCallIdentity(entry),
      finalStateKey: finalStateKey ?? '',
      openKey: entry === undefined ? '' : runningKey(entry),
      displayName: entry?.summary.display_name ?? '',
      title: entry?.display.title ?? '',
      tone: toolCallTone(entry),
      state: toolCallState(entry),
    });
  }
  return groups;
}

const runningKey = (entry: AgentToolActivityEntry): string =>
  entry.summary.side_effect_state !== 'unknown' &&
  (entry.summary.outcome === undefined || entry.summary.outcome === null) &&
  entry.summary.status === 'running'
    ? 'status:running'
    : '';

const joinsRun = (group: ToolCallGroup, entry: AgentToolActivityEntry): boolean => {
  if (group.identity === '' || toolCallIdentity(entry) !== group.identity) return false;
  if (group.finalStateKey !== '') {
    const finalStateKey = toolCallFinalStateKey(entry);
    return finalStateKey !== undefined && finalStateKey === group.finalStateKey;
  }
  const open = runningKey(entry);
  return open !== '' && open === group.openKey;
};
