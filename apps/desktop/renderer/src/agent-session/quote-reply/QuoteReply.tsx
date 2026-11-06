import { useCallback, useContext, useEffect, useState, type RefObject } from 'react';
import { AppIcon } from '../../app-icons';
import { copyText } from '../../clipboard';
import { FileLinkContext } from '../../files/file-links';
import { ToolIcon } from '../../tool-icon';
import { useContextMenu } from '../../ui/useContextMenu';
import { Floating } from './Floating';
import { rangeBox } from './placement';
import { addQuote } from './quote-store';
import { ReplyCard } from './ReplyCard';
import { selectedPassage, useSelectedPassage, type Passage } from './selected-passage';
import './quote-reply.css';

/** The CSS highlight that keeps the quoted passage marked while the card holds the focus. */
const MARK = 'aura-quote-reply';

/**
 * Replying to a passage of the conversation: selected text offers 回复 beside it, and in its
 * right-click menu beside 复制. 回复 opens a card at the selection; a confirmed reply goes into the
 * composer of this conversation as a quote block, and the typing goes on there.
 */
export function QuoteReply({
  sessionId,
  rootRef,
  viewportRef,
}: {
  sessionId: string;
  /** The conversation's messages: only text selected inside them is offered. */
  rootRef: RefObject<HTMLElement | null>;
  viewportRef: RefObject<HTMLElement | null>;
}) {
  const [selected, forget] = useSelectedPassage(rootRef);
  const [replying, setReplying] = useState<Passage>();
  const { openAt, menu } = useContextMenu();
  const scope = useContext(FileLinkContext);
  useMarked(replying?.range);
  useEffect(() => {
    const opened = (event: MouseEvent) => {
      // A link's or a file's own menu has taken the right-click already.
      if (event.defaultPrevented || !(event.target instanceof Node)) return;
      const passage = selectedPassage(rootRef.current);
      if (!passage?.range.intersectsNode(event.target)) return;
      event.preventDefault();
      // The menu key gives no pointer: the menu opens under the selection.
      const under = rangeBox(passage.range);
      const pointer = event.clientX !== 0 || event.clientY !== 0;
      openAt(
        pointer ? event.clientX : (under?.left ?? 0),
        pointer ? event.clientY : (under?.bottom ?? 0),
        '所选内容的操作',
        [
          {
            label: '回复',
            icon: <AppIcon name="chat" size={14} />,
            run: () => setReplying(passage),
          },
          {
            label: '复制',
            icon: <ToolIcon name="copy" size={14} />,
            run: () =>
              void copyText(passage.text, '没能复制，请重试。').then(
                (refused) => refused && scope?.notify(refused),
              ),
          },
        ],
      );
    };
    document.addEventListener('contextmenu', opened);
    return () => document.removeEventListener('contextmenu', opened);
  }, [openAt, rootRef, scope]);
  const confirm = (reply: string) => {
    if (!replying) return;
    addQuote(sessionId, replying.quote, reply.trim());
    setReplying(undefined);
    forget();
    document.getSelection()?.removeAllRanges();
    // The words go on in the composer, above which the quote now waits.
    document.getElementById('aura-composer')?.focus();
  };
  const cancel = useCallback(
    (restoreSelection: boolean) => {
      setReplying(undefined);
      if (!restoreSelection || !replying?.range.startContainer.isConnected) return;
      const selection = document.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(replying.range);
    },
    [replying],
  );
  const offered = replying ? undefined : selected;
  return (
    <>
      {offered ? (
        <Floating
          align="center"
          className="quote-reply-offer"
          range={offered.range}
          viewportRef={viewportRef}
        >
          <button
            className="button is-small"
            onClick={() => setReplying(offered)}
            // Pressing keeps the selection and the focus where they are.
            onMouseDown={(event) => event.preventDefault()}
            type="button"
          >
            <AppIcon name="chat" size={14} />
            回复
          </button>
        </Floating>
      ) : null}
      {menu}
      {replying ? (
        <ReplyCard
          onCancel={cancel}
          onConfirm={confirm}
          passage={replying}
          viewportRef={viewportRef}
        />
      ) : null}
    </>
  );
}

/** Keeps the quoted passage marked as selected while the focus is in the card. */
function useMarked(range: Range | undefined) {
  useEffect(() => {
    if (!range || typeof Highlight !== 'function' || typeof CSS === 'undefined') return;
    if (!('highlights' in CSS)) return;
    CSS.highlights.set(MARK, new Highlight(range));
    return () => {
      CSS.highlights.delete(MARK);
    };
  }, [range]);
}
