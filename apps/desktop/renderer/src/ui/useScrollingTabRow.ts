import { useEffect, useState, type RefObject } from 'react';

export interface TabRowOverflow {
  /** Tabs are hidden past the left edge. */
  left: boolean;
  /** Tabs are hidden past the right edge. */
  right: boolean;
  /** The tabs need more room than the whole strip (`frame`) has. */
  crowded: boolean;
}

/**
 * Shared with the browser's own row of pages: a row of tabs too wide
 * for its place scrolls sideways under the wheel, and says where tabs are out of view (its edges
 * fade there) and whether it is crowded. `signature` changes whenever a tab or its name does.
 */
export function useScrollingTabRow(
  row: RefObject<HTMLElement | null>,
  frame: RefObject<HTMLElement | null>,
  signature: string,
): TabRowOverflow {
  const [overflow, setOverflow] = useState<TabRowOverflow>({
    left: false,
    right: false,
    crowded: false,
  });
  useEffect(() => {
    const element = row.current;
    const outer = frame.current;
    if (!element || !outer) return;
    const measure = () => {
      const left = element.scrollLeft > 1;
      const right = element.scrollLeft + element.clientWidth < element.scrollWidth - 1;
      // A button that takes its room from the row comes only while the tabs cannot fit the whole
      // frame: once they fit without it, it goes and they have its room back.
      const crowded = element.scrollWidth > outer.clientWidth + 1;
      setOverflow((value) =>
        value.left === left && value.right === right && value.crowded === crowded
          ? value
          : { left, right, crowded },
      );
    };
    // The wheel scrolls the row sideways; the page itself has nothing to scroll here.
    const wheel = (event: WheelEvent) => {
      if (element.scrollWidth <= element.clientWidth) return;
      const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
      if (!delta) return;
      event.preventDefault();
      element.scrollLeft += delta;
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    observer.observe(outer);
    element.addEventListener('scroll', measure, { passive: true });
    element.addEventListener('wheel', wheel, { passive: false });
    return () => {
      observer.disconnect();
      element.removeEventListener('scroll', measure);
      element.removeEventListener('wheel', wheel);
    };
  }, [row, frame, signature]);
  return overflow;
}
