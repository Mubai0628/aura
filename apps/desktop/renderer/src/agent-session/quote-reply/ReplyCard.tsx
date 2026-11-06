import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { ToolIcon } from '../../tool-icon';
import { Floating } from './Floating';
import { OWN_UI, type Passage } from './selected-passage';

/**
 * The card 回复 opens at the selection: the quote, and a box for the reply. Enter confirms the
 * reply, which may be empty; Shift+Enter starts a new line. Escape cancels and gives the selection
 * back; a press outside the card cancels and leaves the press its own aim.
 */
export function ReplyCard({
  passage,
  viewportRef,
  onConfirm,
  onCancel,
}: {
  passage: Passage;
  viewportRef: RefObject<HTMLElement | null>;
  onConfirm: (reply: string) => void;
  onCancel: (restoreSelection: boolean) => void;
}) {
  const [reply, setReply] = useState('');
  const box = useRef<HTMLTextAreaElement>(null);
  // The card is in the top layer by now (its layer opens before this effect runs), so the box can
  // take the focus.
  useLayoutEffect(() => box.current?.focus(), []);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      const card = box.current?.closest(OWN_UI);
      if (event.target instanceof Node && card?.contains(event.target)) return;
      onCancel(false);
    };
    document.addEventListener('pointerdown', outside, true);
    return () => document.removeEventListener('pointerdown', outside, true);
  }, [onCancel]);
  return (
    <Floating
      align="start"
      aria-label="引用回复"
      className="quote-reply-card"
      keep
      onKeyDown={(event) => {
        // While an input method composes, Escape is its own: it drops the composition.
        if (event.key !== 'Escape' || event.nativeEvent.isComposing) return;
        event.preventDefault();
        event.stopPropagation();
        onCancel(true);
      }}
      range={passage.range}
      role="dialog"
      viewportRef={viewportRef}
    >
      <blockquote className="quote-reply-quote">{passage.quote}</blockquote>
      <div className="quote-reply-row">
        <textarea
          aria-label="回复"
          onChange={(event) => setReply(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) return;
            event.preventDefault();
            onConfirm(reply);
          }}
          placeholder="回复…"
          ref={box}
          rows={1}
          value={reply}
        />
        <button
          aria-label="加入输入框"
          className="quote-reply-confirm"
          onClick={() => onConfirm(reply)}
          title="加入输入框"
          type="button"
        >
          <ToolIcon name="corner-down-right" size={14} />
        </button>
      </div>
    </Floating>
  );
}
