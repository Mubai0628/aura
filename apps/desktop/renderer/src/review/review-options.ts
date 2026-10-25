import { useSyncExternalStore } from 'react';

/** The review page's view options, as the ⋮ menu sets them. */
export interface ReviewOptions {
  fileList: boolean;
  byFolder: boolean;
  separate: boolean;
  split: boolean;
  wrap: boolean;
  words: boolean;
  ignoreWhitespace: boolean;
}

export const DEFAULT_REVIEW_OPTIONS: ReviewOptions = {
  fileList: true,
  byFolder: false,
  separate: false,
  split: false,
  wrap: false,
  words: true,
  ignoreWhitespace: false,
};

/** This window's own choice, kept across restarts as a convenience. */
const KEY = 'aura.ui.v1.review-options';

function stored(): ReviewOptions {
  try {
    const value = JSON.parse(window.localStorage.getItem(KEY) ?? '{}') as Record<string, unknown>;
    const options = { ...DEFAULT_REVIEW_OPTIONS };
    for (const name of Object.keys(options) as (keyof ReviewOptions)[])
      if (typeof value[name] === 'boolean') options[name] = value[name];
    return options;
  } catch {
    return { ...DEFAULT_REVIEW_OPTIONS };
  }
}

let current: ReviewOptions | undefined;
const listeners = new Set<() => void>();

const snapshot = () => (current ??= stored());

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

export function setReviewOptions(change: Partial<ReviewOptions>): void {
  current = { ...snapshot(), ...change };
  try {
    window.localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // The choice still holds for this window.
  }
  for (const listener of listeners) listener();
}

export function useReviewOptions(): ReviewOptions {
  return useSyncExternalStore(subscribe, snapshot);
}

export function resetReviewOptionsForTests(): void {
  current = { ...DEFAULT_REVIEW_OPTIONS };
  try {
    window.localStorage.removeItem(KEY);
  } catch {
    // Nothing kept.
  }
}
