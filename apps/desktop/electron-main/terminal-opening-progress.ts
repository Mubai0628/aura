import type { TerminalOpeningProgressV1 } from '@aura/contracts/generated';
import { isTerminalOpeningProgress } from '../shared/terminal-opening-validation.js';
const observers = new WeakMap<object, Set<(value: TerminalOpeningProgressV1) => void>>();
export function subscribeTerminalOpeningProgress(
  owner: object,
  listener: (value: TerminalOpeningProgressV1) => void,
) {
  let set = observers.get(owner);
  if (!set) {
    set = new Set();
    observers.set(owner, set);
  }
  set.add(listener);
  return () => {
    set.delete(listener);
  };
}
export function terminalOpeningNotification(
  owner: object,
  method: string,
  value: unknown,
  runtime: string,
): boolean {
  if (method !== 'terminal.opening.v1') return false;
  if (!isTerminalOpeningProgress(value) || value.runtime_instance_id !== runtime) return true;
  for (const listener of observers.get(owner) ?? []) {
    try {
      listener(value);
    } catch {
      /* one observer cannot retire the Core channel */
    }
  }
  return true;
}
