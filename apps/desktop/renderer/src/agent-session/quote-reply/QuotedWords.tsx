import type { ReactNode } from 'react';
import { quoteParts } from './quote-parts';
import './quoted-words.css';

/**
 * A message the conversation does not show yet, with its quotes drawn as quotes as the
 * conversation will draw them; `words` draws the rest as the place drew it before.
 */
export function QuotedWords({
  text,
  className,
  words,
}: {
  text: string;
  className: string;
  words: (text: string) => ReactNode;
}) {
  const parts = quoteParts(text);
  if (!parts) return <p className={className}>{words(text)}</p>;
  return (
    <div className={`${className} quoted-words`}>
      {parts.map((part, index) =>
        part.quote ? (
          <blockquote key={index}>{part.text}</blockquote>
        ) : (
          <p key={index}>{words(part.text)}</p>
        ),
      )}
    </div>
  );
}
