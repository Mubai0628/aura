import { useEffect, useLayoutEffect, useRef, type RefObject } from 'react';
import './compact-row.css';

/** Room a row must gain past what its words need before they show again: no flicker at the edge. */
const SLACK = 8;

/** The row's `data-compact`: every word; all but those of what gives way first; icons alone. */
const STATES: readonly (string | undefined)[] = [undefined, 'first', 'true'];

/** What the row was last measured at: its width and what it held. */
interface Measured {
  width: number;
  content: string;
}

/**
 * "会被挤压到很小的文字按钮……压缩到无法横板一行内容纳不下文字的时候变为适配图标
 * 大小的按钮且不可再被压缩". A row of buttons that carry an icon and words: once any of those words
 * no longer fits on its line, every such button shows its icon alone, at a fixed size, and its
 * words stay for screen readers and in its tooltip. They come back when the row is wide enough for
 * all of them again.
 *
 * The row is measured at
 * the width its parts take with their words shown. Before, what ran past the row's edge counted:
 * the stop button's turning ring while a task ran, a menu or the context bar floating out of it;
 * and a width remembered from such a moment kept the icons until the row grew past it. Other
 * words or parts in the row, also when a part changes without this row rendering, are measured
 * again.
 *
 * A button takes part with `data-adaptive`, its words with `data-adaptive-label`; the row gets
 * `data-compact` (set here on the element, outside React's render, so measuring never renders).
 *
 * A button marked `data-adaptive-first` (the thinking level beside the model) gives way
 * first: while the row holds every other word without its words, only it shows its icon alone
 * (`data-compact="first"`), so it does not take the others' words away at a width that held them.
 */
export function useCompactRow<T extends HTMLElement>(): RefObject<T | null> {
  const ref = useRef<T>(null);
  const measured = useRef<Measured>({ width: -1, content: '' });
  // After every render: words may have changed (another model, another comparison).
  useLayoutEffect(() => {
    if (ref.current) fit(ref.current, measured.current);
  });
  useEffect(() => {
    const row = ref.current;
    if (!row) return;
    const resized = new ResizeObserver(() => fit(row, measured.current));
    resized.observe(row);
    // A part of the row that renders by itself (the model picker loading, a task's controls).
    const changed = new MutationObserver(() => fit(row, measured.current));
    changed.observe(row, { childList: true, subtree: true, characterData: true });
    return () => {
      resized.disconnect();
      changed.disconnect();
    };
  }, []);
  return ref;
}

/** What the row holds: its words and how many parts. */
function contentOf(row: HTMLElement): string {
  return `${row.getElementsByTagName('*').length}:${row.textContent ?? ''}`;
}

function fit(row: HTMLElement, measured: Measured): void {
  const width = row.getBoundingClientRect().width;
  const content = contentOf(row);
  if (Math.round(width) === measured.width && content === measured.content) return;
  measured.width = Math.round(width);
  measured.content = content;
  // Measured with the words shown, at the row's own natural width (compact-row.css), and then
  // without the words of what gives way first; set and taken back in one task, so it is never
  // painted. Showing more words than now needs room to spare.
  const was = Math.max(0, STATES.indexOf(row.dataset.compact));
  delete row.dataset.compact;
  row.dataset.measuring = 'true';
  const fits = (state: number) =>
    row.getBoundingClientRect().width <= width + 0.5 - (state < was ? SLACK : 0);
  let state = fits(0) ? 0 : 2;
  if (state === 2 && row.querySelector('[data-adaptive-first]')) {
    row.dataset.compact = 'first';
    if (fits(1)) state = 1;
    delete row.dataset.compact;
  }
  delete row.dataset.measuring;
  const compact = STATES[state];
  if (compact) row.dataset.compact = compact;
}
