import { useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import { AppIcon } from '../app-icons';
import './help-tip.css';

/**
 * A question mark beside a setting. Resting on it, or reaching it with the keyboard, shows `text`
 * in a bubble pointing at it, drawn outside the setting's own box so nothing clips it: in the
 * modal dialog the question mark is in, since a modal dialog is drawn above the rest of the page,
 * and over the page otherwise.
 */
export function HelpTip({ label, text }: { label: string; text: string }) {
  const [host, setHost] = useState<Element | null>(null);
  const anchor = useRef<HTMLSpanElement>(null);
  const bubble = useRef<HTMLSpanElement>(null);
  const id = useId();
  const show = () => setHost(anchor.current?.closest('dialog') ?? document.body);
  const hide = () => setHost(null);
  useLayoutEffect(() => {
    const mark = anchor.current?.getBoundingClientRect();
    const node = bubble.current;
    if (host === null || mark === undefined || node === null) return;
    const bounds = node.getBoundingClientRect();
    const center = mark.left + mark.width / 2;
    const left = Math.max(
      8,
      Math.min(center - bounds.width / 2, window.innerWidth - bounds.width - 8),
    );
    const below = mark.top - bounds.height - 8 < 8;
    node.dataset.side = below ? 'below' : 'above';
    node.style.left = `${left}px`;
    node.style.top = `${below ? mark.bottom + 8 : mark.top - bounds.height - 8}px`;
    node.style.setProperty('--help-tip-arrow', `${center - left}px`);
  }, [host]);
  return (
    <>
      <span
        aria-describedby={host === null ? undefined : id}
        aria-label={label}
        className="help-tip"
        onBlur={hide}
        onFocus={show}
        onMouseEnter={show}
        onMouseLeave={hide}
        ref={anchor}
        role="img"
        tabIndex={0}
      >
        <AppIcon name="help" size={14} />
      </span>
      {host === null
        ? null
        : createPortal(
            <span className="help-tip-bubble" id={id} ref={bubble} role="tooltip">
              {text}
            </span>,
            host,
          )}
    </>
  );
}
