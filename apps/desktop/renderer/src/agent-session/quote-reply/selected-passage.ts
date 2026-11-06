import { useCallback, useEffect, useState, type RefObject } from 'react';
import { quoteFromSelection } from './quote-text';

/** A passage selected in the conversation: the quote it makes, its text as selected, and where. */
export interface Passage {
  quote: string;
  text: string;
  range: Range;
}

/** Marks this feature's own button and card, whose text and presses are not the conversation's. */
export const OWN_UI = '[data-quote-reply]';

/** Text that is not the conversation's messages: what can be typed in, dialogs, menus, this UI. */
const NOT_MESSAGES = [
  'input',
  'textarea',
  'select',
  '[contenteditable]:not([contenteditable="false"])',
  'dialog',
  '[role="dialog"]',
  '[role="menu"]',
  OWN_UI,
].join(', ');

const elementOf = (node: Node) => (node instanceof Element ? node : node.parentElement);

/** The passage selected inside `root`, when the whole selection lies in its messages. */
export function selectedPassage(root: HTMLElement | null): Passage | undefined {
  const selection = document.getSelection();
  if (!root || !selection || selection.isCollapsed || selection.rangeCount === 0) return undefined;
  const range = selection.getRangeAt(0);
  if (!root.contains(range.commonAncestorContainer)) return undefined;
  const excluded = (node: Node) => {
    const found = elementOf(node)?.closest(NOT_MESSAGES);
    return found != null && root.contains(found);
  };
  if ([range.startContainer, range.endContainer].some(excluded)) return undefined;
  const text = selection.toString();
  const quote = quoteFromSelection(text);
  return quote ? { quote, text, range: range.cloneRange() } : undefined;
}

/**
 * The passage selected in the conversation `root`, read once the mouse that selects it is let go,
 * so nothing follows the pointer while it drags; nothing once the selection collapses or leaves the
 * messages. `forget` drops it before the selection's own event says so.
 */
export function useSelectedPassage(root: RefObject<HTMLElement | null>) {
  const [passage, setPassage] = useState<Passage>();
  useEffect(() => {
    let pressed = false;
    let pending: number | undefined;
    const read = () => setPassage(selectedPassage(root.current));
    const down = (event: PointerEvent) => {
      // A right-click keeps the selection for its menu, and this feature's own buttons keep it.
      if (event.button !== 0) return;
      if (event.target instanceof Element && event.target.closest(OWN_UI)) return;
      pressed = true;
      setPassage(undefined);
    };
    const up = () => {
      if (!pressed) return;
      pressed = false;
      // A click inside a selection collapses it only after the release.
      window.clearTimeout(pending);
      pending = window.setTimeout(read);
    };
    const changed = () => {
      if (!pressed) read();
    };
    document.addEventListener('pointerdown', down, true);
    document.addEventListener('pointerup', up, true);
    document.addEventListener('selectionchange', changed);
    return () => {
      window.clearTimeout(pending);
      document.removeEventListener('pointerdown', down, true);
      document.removeEventListener('pointerup', up, true);
      document.removeEventListener('selectionchange', changed);
    };
  }, [root]);
  const forget = useCallback(() => setPassage(undefined), []);
  return [passage, forget] as const;
}
