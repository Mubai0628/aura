import type {
  AgentPromptInspection,
  AgentSessionProjection,
  ContextUsageSummaryView,
} from '@aura/contracts/generated';

/** Where the composer meter's numbers come from. */
export type InspectionSource =
  | { kind: 'current' }
  | { kind: 'recent'; modelStepId: string }
  | { kind: 'loading' }
  | { kind: 'none' }
  | { kind: 'failed' };

export interface LatestInspection {
  source: InspectionSource;
  /** Kept for callers that still pass a full inspection. The ring does not fill it. */
  view: AgentPromptInspection | undefined;
  /** Frozen meter from the session projection. Opening details still loads the inspector. */
  usage: ContextUsageSummaryView | undefined;
  compactionRunning: boolean;
  /** The step whose real request the meter shows, when there is one. */
  modelStepId: string | undefined;
  contextSnapshotId: string | undefined;
  /** That step's request has ended; its cache use is known only then. */
  finished: boolean;
}

/**
 * The newest assistant message may have no Context snapshot of its own: a step
 * that failed, was interrupted, or has not reached its request yet. Dropping
 * the meter in that case hid data that is still durably available, so the
 * composer follows the most recent *real* request of this Session instead and
 * reports which step produced it.
 *
 * The selection never crosses Sessions or Runtime instances, and an older
 * request is never presented as the current step's own usage.
 */
export function selectInspectionStep(
  projection: AgentSessionProjection | undefined,
): { modelStepId: string; current: boolean } | undefined {
  if (!projection) return undefined;
  const newest = projection.turns.at(-1)?.assistant_messages.at(-1);
  for (const turn of [...projection.turns].reverse()) {
    for (const message of [...turn.assistant_messages].reverse()) {
      if (message.context_snapshot_id) {
        return {
          modelStepId: message.model_step_id,
          current: newest?.model_step_id === message.model_step_id,
        };
      }
    }
  }
  return undefined;
}

const meterFor = (
  projection: AgentSessionProjection,
  modelStepId: string,
): {
  usage: ContextUsageSummaryView | undefined;
  compactionRunning: boolean;
  contextSnapshotId: string | undefined;
  finished: boolean;
} => {
  for (const turn of projection.turns) {
    for (const message of turn.assistant_messages) {
      if (message.model_step_id !== modelStepId) continue;
      return {
        usage: message.context_usage ?? undefined,
        compactionRunning: message.context_compaction_running === true,
        contextSnapshotId: message.context_snapshot_id ?? undefined,
        finished: message.partial !== true,
      };
    }
  }
  return {
    usage: undefined,
    compactionRunning: false,
    contextSnapshotId: undefined,
    finished: false,
  };
};

export function useLatestInspection(
  projection: AgentSessionProjection | undefined,
): LatestInspection {
  const selected = selectInspectionStep(projection);
  if (!projection || !selected) {
    return {
      source: { kind: 'none' },
      view: undefined,
      usage: undefined,
      compactionRunning: false,
      modelStepId: undefined,
      contextSnapshotId: undefined,
      finished: false,
    };
  }
  const meter = meterFor(projection, selected.modelStepId);
  if (!meter.usage) {
    return {
      source: { kind: 'failed' },
      view: undefined,
      usage: undefined,
      compactionRunning: meter.compactionRunning,
      modelStepId: selected.modelStepId,
      contextSnapshotId: meter.contextSnapshotId,
      finished: meter.finished,
    };
  }
  return {
    source: selected.current
      ? { kind: 'current' }
      : { kind: 'recent', modelStepId: selected.modelStepId },
    view: undefined,
    usage: meter.usage,
    compactionRunning: meter.compactionRunning,
    modelStepId: selected.modelStepId,
    contextSnapshotId: meter.contextSnapshotId,
    finished: meter.finished,
  };
}
