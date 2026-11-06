/** A passage of the conversation replied to with 回复: the quote, and the reply typed for it. */
export interface QuoteBlock {
  id: string;
  quote: string;
  /** May be empty: the quote alone is sent. */
  reply: string;
}

/** Whether a stored value is a list of quote blocks, as the draft store reads it back. */
export const isQuoteBlocks = (value: unknown): value is QuoteBlock[] =>
  Array.isArray(value) &&
  value.every(
    (item) =>
      !!item &&
      typeof item === 'object' &&
      typeof item.id === 'string' &&
      typeof item.quote === 'string' &&
      typeof item.reply === 'string',
  );
