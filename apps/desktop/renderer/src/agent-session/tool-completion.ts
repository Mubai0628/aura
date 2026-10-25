import type { AgentToolActivityEntry } from '@aura/contracts/generated';

/** Stable across streamed tokens. A set also catches an earlier parallel call
 * completing after a newer call, even on historical rows without a timestamp. */
export function latestToolCompletion(calls: ReadonlyMap<string, AgentToolActivityEntry>) {
  return [...calls]
    .filter(([, call]) => call.summary.outcome)
    .map(([id]) => id)
    .sort()
    .join(':');
}
