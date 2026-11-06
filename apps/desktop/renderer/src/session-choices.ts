import { useSyncExternalStore } from 'react';

/** Enough for every recent conversation; the oldest are forgotten first. */
const LIMIT = 300;

/**
 * What each conversation chose in the composer for each model, such as its thinking level or
 * speed mode, kept under `key`. It goes with every message the conversation sends. Remembered by
 * this window, like the composer drafts; a new conversation's choice moves from its draft to it,
 * and a branch takes its source's.
 */
export interface SessionChoices<T extends string> {
  /** The conversation's choice for the model; none is the model's own. */
  get(sessionId: string, modelProfileId: string): T | undefined;
  choose(sessionId: string, modelProfileId: string, value: T | null): void;
  copy(from: string, to: string): void;
  subscribe(listener: () => void): () => void;
  /** Tests start from what localStorage holds. */
  forget(): void;
}

export function sessionChoices<T extends string>(key: string): SessionChoices<T> {
  type Choices = Record<string, Record<string, T>>;
  let cached: Choices | undefined;
  const listeners = new Set<() => void>();
  const read = (): Choices => {
    if (cached) return cached;
    try {
      const value: unknown = JSON.parse(localStorage.getItem(key) ?? '{}');
      cached =
        value && typeof value === 'object' && !Array.isArray(value) ? (value as Choices) : {};
    } catch {
      cached = {};
    }
    return cached;
  };
  const save = (next: Choices): void => {
    const sessions = Object.keys(next);
    for (const old of sessions.slice(0, Math.max(0, sessions.length - LIMIT))) delete next[old];
    cached = next;
    try {
      localStorage.setItem(key, JSON.stringify(next));
    } catch {
      /* Kept in memory until the window reloads. */
    }
    for (const listener of listeners) listener();
  };
  return {
    get: (sessionId, modelProfileId) => read()[sessionId]?.[modelProfileId],
    choose: (sessionId, modelProfileId, value) => {
      const all = { ...read() };
      const session = { ...all[sessionId] };
      if (value) session[modelProfileId] = value;
      else delete session[modelProfileId];
      delete all[sessionId];
      if (Object.keys(session).length) all[sessionId] = session;
      save(all);
    },
    copy: (from, to) => {
      const choices = read()[from];
      if (!choices || from === to) return;
      save({ ...read(), [to]: { ...choices } });
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    forget: () => {
      cached = undefined;
    },
  };
}

/** The conversation's choice for the model, kept current while the composer shows it. */
export function useSessionChoice<T extends string>(
  choices: SessionChoices<T>,
  sessionId: string,
  modelProfileId: string | undefined,
): T | undefined {
  return useSyncExternalStore(choices.subscribe, () =>
    modelProfileId ? choices.get(sessionId, modelProfileId) : undefined,
  );
}
