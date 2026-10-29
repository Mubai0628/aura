import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { endedAgo, endedAtText } from './ended-ago';

/**
 * How long ago the turn ended, counted again every minute. Resting on the text shows when it
 * ended in a bubble just above it, drawn over the page so the conversation does not clip it.
 */
export function EndedAgo({ endedAt }: { endedAt: number }) {
  const [now, setNow] = useState(Date.now);
  const [open, setOpen] = useState(false);
  const text = useRef<HTMLSpanElement>(null);
  const bubble = useRef<HTMLSpanElement>(null);
  const id = useId();
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, []);
  useLayoutEffect(() => {
    const anchor = text.current?.getBoundingClientRect();
    const node = bubble.current;
    if (!open || anchor === undefined || node === null) return;
    const bounds = node.getBoundingClientRect();
    const left = anchor.left + anchor.width / 2 - bounds.width / 2;
    node.style.left = `${Math.max(8, Math.min(left, window.innerWidth - bounds.width - 8))}px`;
    node.style.top = `${anchor.top - bounds.height - 6 < 8 ? anchor.bottom + 6 : anchor.top - bounds.height - 6}px`;
  }, [open]);
  return (
    <>
      <span
        aria-describedby={open ? id : undefined}
        className="agent-ended-ago"
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        ref={text}
      >
        {endedAgo(endedAt, now)}
      </span>
      {open
        ? createPortal(
            <span className="agent-ended-bubble" id={id} ref={bubble} role="tooltip">
              {endedAtText(endedAt)}
            </span>,
            document.body,
          )
        : null}
    </>
  );
}
