import { useSyncExternalStore } from 'react';

/** The sub-agent whose grants the user opened, from its record or its card on the canvas. */
export interface SubagentGrantsTarget {
  parentSessionId: string;
  agent: string;
  name: string;
  sessionId: string;
  workspaceId: string;
}

let target: SubagentGrantsTarget | undefined;
const listeners = new Set<() => void>();
const changed = () => {
  for (const listener of [...listeners]) listener();
};

export function openSubagentGrants(next: SubagentGrantsTarget) {
  target = next;
  changed();
}

export function closeSubagentGrants() {
  target = undefined;
  changed();
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

/** The sub-agent whose grants are shown, if any. */
export function useSubagentGrants(): SubagentGrantsTarget | undefined {
  return useSyncExternalStore(subscribe, () => target);
}
