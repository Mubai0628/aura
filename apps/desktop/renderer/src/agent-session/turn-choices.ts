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

/**
 * What the user chose for one stretch of a turn's process, by the keys of its parts, first part
 * first. A stretch is named by its first part, and a long turn's earlier steps are read later, in
 * front of the stretch that was first: a choice kept under any of its parts is the stretch's.
 */
export function sectionChoice(
  chosen: Record<string, boolean>,
  keys: readonly string[],
): boolean | undefined {
  const key = keys.find((key) => Object.hasOwn(chosen, key));
  return key === undefined ? undefined : chosen[key];
}

/** The turn's choices with this stretch set to `open`, kept under its first part only. */
export function withSectionChoice(
  chosen: Record<string, boolean>,
  keys: readonly string[],
  open: boolean,
): Record<string, boolean> {
  const next = Object.fromEntries(Object.entries(chosen).filter(([key]) => !keys.includes(key)));
  next[keys[0]!] = open;
  return next;
}

/** Tests start from a window that kept nothing. */
export function forgetTurnChoices(): void {
  kept.clear();
}
