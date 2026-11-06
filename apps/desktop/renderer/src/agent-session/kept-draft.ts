import { useCallback, useState } from 'react';

/**
 * What a conversation's composer holds besides its words (the pictures and files added, the
 * references and Skills chosen), kept in memory while Aura runs. Leaving the conversation for
 * another one or for the settings unmounts the composer; coming back finds them as they were.
 * Nothing here is written to disk, and an emptied entry is dropped.
 */
const kept = new Map<string, unknown>();

type Update<T> = T | ((old: T) => T);

/** A composer value of one `kind` kept under the conversation's `key` (none: not kept). */
export function useKeptDraft<T extends readonly unknown[]>(
  kind: string,
  key: string | undefined,
  empty: T,
): [T, (next: Update<T>) => void] {
  const id = key === undefined ? undefined : `${kind}:${key}`;
  const read = useCallback(
    (): T => (id !== undefined && kept.has(id) ? (kept.get(id) as T) : empty),
    [id, empty],
  );
  const [state, setState] = useState(() => ({ id, value: read() }));
  // Another conversation's composer reads its own.
  const value = state.id === id ? state.value : read();
  const set = useCallback(
    (next: Update<T>) =>
      setState((old) => {
        const base = old.id === id ? old.value : read();
        const value = typeof next === 'function' ? (next as (old: T) => T)(base) : next;
        if (id !== undefined) {
          if (value.length === 0) kept.delete(id);
          else kept.set(id, value);
        }
        return { id, value };
      }),
    [id, read],
  );
  return [value, set];
}

/** Forgets every kept value; for tests. */
export function resetKeptDraftsForTests(): void {
  kept.clear();
}
