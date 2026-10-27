import { useEffect, useRef, useState } from 'react';

/** Rows drawn at first, and added each time the end of the list comes into view. */
export const LIST_PAGE = 40;

/**
 * A long list draws its
 * first page at once and the next page as its end comes into view, so a page with thousands of
 * Skills, tools or servers opens as fast as one with a few. A list of another length (a search,
 * an item added or removed) starts again from its first page; the same list given again, as after
 * acting on one of its rows, keeps the rows already drawn.
 */
export function useIncrementalList<T>(items: readonly T[], page = LIST_PAGE, keep = -1) {
  const [limit, setLimit] = useState(page);
  const [length, setLength] = useState(items.length);
  if (length !== items.length) {
    setLength(items.length);
    setLimit(page);
  }
  const sentinel = useRef<HTMLLIElement | null>(null);
  const drawn = limit;
  // Nothing can tell when the end comes into view without IntersectionObserver: all rows are drawn.
  const more = typeof IntersectionObserver === 'function' && items.length > drawn;
  // The row at `keep` (the open conversation, say) beyond the pages shown so far is drawn on its
  // own after them: drawing every row before it made opening the 5,000th conversation draw 5,000.
  const kept = more && keep >= drawn ? items[keep] : undefined;
  useEffect(() => {
    const element = sentinel.current;
    if (!more || !element) return;
    // A new observer for each page reports at once whether the end is still in view.
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) setLimit(drawn + page);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [more, drawn, page]);
  return { shown: more ? items.slice(0, drawn) : items, more, sentinel, total: items.length, kept };
}
