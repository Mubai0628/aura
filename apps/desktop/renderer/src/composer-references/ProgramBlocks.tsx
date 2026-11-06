import { Fragment, useLayoutEffect, useRef, type RefObject } from 'react';

import type { ProgramToken } from './program-tokens';
import { ProgramIcon } from './ProgramIcon';
import './program-blocks.css';

type Piece = { text: string } | { token: ProgramToken };

/** The words cut at each program block. A last zero-width space keeps a final empty line. */
function pieces(value: string, tokens: readonly ProgramToken[]): Piece[] {
  const out: Piece[] = [];
  let at = 0;
  for (const token of tokens) {
    if (token.start > at) out.push({ text: value.slice(at, token.start) });
    out.push({ token });
    at = token.end;
  }
  out.push({ text: `${value.slice(at)}\u200b` });
  return out;
}

/**
 * The programs chosen with "@" drawn as blocks over the composer's words. The words stay the
 * textarea's own, so typing, selecting and Chinese input work as before: a copy laid out the same
 * way behind them paints each block, and one in front of them puts the program's icon over the
 * `@"` and covers the closing quote. Both copies follow the textarea's size and scrolling.
 */
export function ProgramBlocks({
  textarea,
  value,
  tokens,
}: {
  textarea: RefObject<HTMLTextAreaElement | null>;
  value: string;
  tokens: readonly ProgramToken[];
}) {
  const back = useRef<HTMLDivElement>(null);
  const front = useRef<HTMLDivElement>(null);
  const shown = tokens.length > 0;
  useLayoutEffect(() => {
    const area = textarea.current;
    if (!shown || !area) return;
    const follow = () => {
      for (const layer of [back.current, front.current]) {
        if (!layer) continue;
        layer.style.width = `${area.clientWidth}px`;
        layer.style.height = `${area.clientHeight}px`;
        layer.scrollTop = area.scrollTop;
      }
    };
    follow();
    area.addEventListener('scroll', follow);
    const resized = new ResizeObserver(follow);
    resized.observe(area);
    return () => {
      area.removeEventListener('scroll', follow);
      resized.disconnect();
    };
  }, [textarea, shown, value]);
  if (!shown) return null;
  const parts = pieces(value, tokens);
  return (
    <>
      <div ref={back} className="composer-program-layer" data-layer="back" aria-hidden="true">
        {parts.map((part, index) =>
          'text' in part ? (
            <Fragment key={index}>{part.text}</Fragment>
          ) : (
            <span key={index} className="composer-program-block">
              {value.slice(part.token.start, part.token.end)}
            </span>
          ),
        )}
      </div>
      <div ref={front} className="composer-program-layer" data-layer="front" aria-hidden="true">
        {parts.map((part, index) =>
          'text' in part ? (
            <Fragment key={index}>{part.text}</Fragment>
          ) : (
            <Fragment key={index}>
              <span className="composer-program-mark" data-program={part.token.name}>
                <ProgramIcon appId={part.token.appId} size={14} />
                {'@"'}
              </span>
              {part.token.name}
              <span className="composer-program-end">"</span>
            </Fragment>
          ),
        )}
      </div>
    </>
  );
}
