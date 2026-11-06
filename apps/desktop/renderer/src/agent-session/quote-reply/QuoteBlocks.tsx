import { AppIcon } from '../../app-icons';
import type { QuoteBlock } from './quote-block';
import { removeQuotes } from './quote-store';
import './quote-blocks.css';

/** How many characters of a quote name its × for a screen reader. */
const NAMED = 16;

const named = (quote: string) => {
  const characters = Array.from(quote.replace(/\s+/gu, ' ').trim());
  return characters.length > NAMED
    ? `${characters.slice(0, NAMED).join('')}…`
    : characters.join('');
};

/**
 * The passages replied to with 回复, waiting above the composer's words: each quote with its bar,
 * the reply under it, and an × that takes the block out. Sending puts them before the words.
 */
export function QuoteBlocks({
  sessionId,
  quotes,
}: {
  sessionId: string;
  quotes: readonly QuoteBlock[];
}) {
  if (quotes.length === 0) return null;
  return (
    <ul aria-label="引用回复" className="composer-quotes">
      {quotes.map((block) => (
        <li className="composer-quote" key={block.id}>
          <blockquote title={block.quote}>{block.quote}</blockquote>
          {block.reply ? <p title={block.reply}>{block.reply}</p> : null}
          <button
            aria-label={`移除引用 ${named(block.quote)}`}
            onClick={() => removeQuotes(sessionId, [block.id])}
            type="button"
          >
            <AppIcon name="close" size={13} />
          </button>
        </li>
      ))}
    </ul>
  );
}
