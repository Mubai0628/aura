import { useCallback, useState } from 'react';

/**
 * What the user opened or
 * closed in a turn — the turn itself and each stretch of its process — is kept by the window, not
 * by one view of the conversation, so coming back shows the turn as it was left. The oldest
 * choices are forgotten first.
 */
const kept = new Map<string, unknown>();
const REMEMBERED = 500;

export function useTurnChoice<T>(key: string, initial: T): [T, (value: T) => void] {
  const [value, setValue] = useState<T>(() => (kept.has(key) ? (kept.get(key) as T) : initial));
  const choose = useCallback(
    (next: T) => {
      kept.delete(key);
      kept.set(key, next);
      if (kept.size > REMEMBERED) kept.delete(kept.keys().next().value!);
      setValue(next);
    },
    [key],
  );
  return [value, choose];
}

/** Tests start from a window that kept nothing. */
export function forgetTurnChoices(): void {
  kept.clear();
}
