import { getComposerDraft, updateComposerDraft, useComposerDrafts } from '../composer-drafts';
import type { QuoteBlock } from './quote-block';

const NONE: readonly QuoteBlock[] = [];

/**
 * The passages quoted in a conversation's composer. They are kept with the conversation's draft,
 * so they stay with that conversation, survive a restart and go when the draft is removed.
 */
export const quotesOf = (sessionId: string): readonly QuoteBlock[] =>
  getComposerDraft(sessionId)?.quotes ?? NONE;

export function useQuotes(sessionId: string): readonly QuoteBlock[] {
  return useComposerDrafts().find((draft) => draft.id === sessionId)?.quotes ?? NONE;
}

export function addQuote(sessionId: string, quote: string, reply: string): void {
  updateComposerDraft(sessionId, {
    quotes: [...quotesOf(sessionId), { id: crypto.randomUUID(), quote, reply }],
  });
}

/** Takes the named blocks out: one the user removed, or the ones a sent message carried. */
export function removeQuotes(sessionId: string, ids: readonly string[]): void {
  const quotes = quotesOf(sessionId);
  const kept = quotes.filter((block) => !ids.includes(block.id));
  if (kept.length !== quotes.length) updateComposerDraft(sessionId, { quotes: kept });
}
