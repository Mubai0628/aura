import type { ReasoningEffort } from '@aura/contracts/generated';
import { useSyncExternalStore } from 'react';

/** How each thinking level reads. `xhigh` and `max` are two levels. */
export const REASONING_LABELS: Record<ReasoningEffort, string> = {
  off: '关闭',
  minimal: '最少',
  low: '低',
  medium: '中',
  high: '高',
  xhigh: '超高',
  max: '最高',
};

/**
 * The level a model is sent for a stored one, as Core decides: the level itself when the
 * model offers it; for `xhigh`, which older versions stored for a model whose top level is
 * named `max`, that `max`; otherwise none, and the model's own default applies.
 */
export function levelToSend(
  level: ReasoningEffort,
  offered: readonly ReasoningEffort[],
): ReasoningEffort | null {
  if (offered.includes(level)) return level;
  return level === 'xhigh' && offered.includes('max') ? 'max' : null;
}

/**
 * The thinking level each conversation
 * chose, per model. It goes with every message the conversation sends; a conversation (or a new
 * one) that chose none takes the model's default from the model settings. Remembered by this
 * window, like the composer drafts; a new conversation's choice moves from its draft to it.
 */
const KEY = 'aura.session-reasoning.v1';
/** Enough for every recent conversation; the oldest are forgotten first. */
const LIMIT = 300;
type Choices = Record<string, Record<string, ReasoningEffort>>;

let cached: Choices | undefined;
const listeners = new Set<() => void>();

function read(): Choices {
  if (cached) return cached;
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    cached = value && typeof value === 'object' && !Array.isArray(value) ? (value as Choices) : {};
  } catch {
    cached = {};
  }
  return cached;
}

function save(next: Choices): void {
  const sessions = Object.keys(next);
  for (const old of sessions.slice(0, Math.max(0, sessions.length - LIMIT))) delete next[old];
  cached = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    /* Kept in memory until the window reloads. */
  }
  for (const listener of listeners) listener();
}

/** The level this conversation chose for this model; none follows the model's default. */
export function sessionReasoning(sessionId: string, modelProfileId: string) {
  return read()[sessionId]?.[modelProfileId];
}

export function chooseSessionReasoning(
  sessionId: string,
  modelProfileId: string,
  level: ReasoningEffort | null,
): void {
  const all = { ...read() };
  const session = { ...all[sessionId] };
  if (level) session[modelProfileId] = level;
  else delete session[modelProfileId];
  delete all[sessionId];
  if (Object.keys(session).length) all[sessionId] = session;
  save(all);
}

/** A new conversation takes its draft's choices; a branch takes its source's. */
export function copySessionReasoning(from: string, to: string): void {
  const choices = read()[from];
  if (!choices || from === to) return;
  save({ ...read(), [to]: { ...choices } });
}

/** The conversation's choice for the model, kept current while the composer shows it. */
export function useSessionReasoning(sessionId: string, modelProfileId: string | undefined) {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => (modelProfileId ? sessionReasoning(sessionId, modelProfileId) : undefined),
  );
}

/** Tests start from what localStorage holds. */
export function forgetSessionReasoning(): void {
  cached = undefined;
}
