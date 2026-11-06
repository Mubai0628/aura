import type { QuoteBlock } from './quote-block';

/**
 * The most characters one quote keeps. The model already has the conversation, so a quote only
 * has to point at a passage: a longer selection keeps its start and ends with an ellipsis rather
 * than repeating a whole answer in the message.
 */
export const MAX_QUOTE_CHARS = 2_000;

/**
 * The selected text as a quote: line ends made `\n`, spaces at line ends dropped, a run of blank
 * lines made one, and at most `MAX_QUOTE_CHARS` characters, counted as code points so no character
 * is cut in half. Nothing when only spaces were selected.
 */
export function quoteFromSelection(selected: string): string | undefined {
  const lines: string[] = [];
  for (const line of selected.replace(/\r\n?/gu, '\n').split('\n')) {
    const kept = line.trimEnd();
    if (kept.trim()) lines.push(kept);
    else if (lines.length > 0 && lines.at(-1) !== '') lines.push('');
  }
  while (lines.at(-1) === '') lines.pop();
  const characters = Array.from(lines.join('\n'));
  if (characters.length === 0) return undefined;
  return characters.length > MAX_QUOTE_CHARS
    ? `${characters.slice(0, MAX_QUOTE_CHARS).join('').trimEnd()}…`
    : characters.join('');
}

/** A quote as Markdown: each line behind "> ", a blank line as ">" so the quote stays one. */
const markdownQuote = (quote: string) =>
  quote
    .split('\n')
    .map((line) => (line ? `> ${line}` : '>'))
    .join('\n');

/**
 * The text a send carries: each quoted passage as a Markdown quote with its reply under it, then
 * the typed words. Blank lines keep the parts apart, since a line right after a quote would
 * otherwise continue it. Without quotes the words go exactly as typed.
 */
export function quotedMessage(quotes: readonly QuoteBlock[], typed: string): string {
  if (quotes.length === 0) return typed;
  const parts = quotes.flatMap(({ quote, reply }) =>
    reply.trim() ? [markdownQuote(quote), reply.trim()] : [markdownQuote(quote)],
  );
  if (typed.trim()) parts.push(typed);
  return parts.join('\n\n');
}
