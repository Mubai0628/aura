/** One part of a message: a quoted passage, or the words between. */
export interface QuotePart {
  quote: boolean;
  text: string;
}

const quoteLine = (line: string) => line === '>' || line.startsWith('> ');

/**
 * A message's quoted passages and its words, as a send with 回复 writes them: each quote a block
 * of "> " lines, the parts apart by blank lines. Nothing when the message quotes nothing.
 */
export function quoteParts(text: string): QuotePart[] | undefined {
  const parts = text.split(/\n{2,}/u).map((block) => {
    const lines = block.split('\n');
    return lines.every(quoteLine)
      ? { quote: true, text: lines.map((line) => line.slice(2)).join('\n') }
      : { quote: false, text: block };
  });
  return parts.some((part) => part.quote) ? parts : undefined;
}

/** The message with its quote marks left out, for a place that shows it as plain text. */
export function withoutQuoteMarks(text: string): string {
  const parts = quoteParts(text);
  return parts ? parts.map((part) => part.text).join('\n\n') : text;
}
