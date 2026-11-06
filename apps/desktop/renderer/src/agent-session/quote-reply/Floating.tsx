import { useLayoutEffect, useRef, type HTMLAttributes, type RefObject } from 'react';
import { placeBeside, rangeBox, visibleArea, type Box } from './placement';

/**
 * Something shown beside a selection in the conversation. It sits in the top layer, so no
 * scrolling pane clips it and the composer's dock does not cover it, and follows the selection as
 * the conversation scrolls or the window changes size. Placed above the selection when there is
 * room in what can be seen of the conversation, else below; out of sight it hides, unless `keep`
 * holds it at the nearest edge.
 */
export function Floating({
  range,
  viewportRef,
  align,
  keep = false,
  ...rest
}: {
  range: Range;
  viewportRef: RefObject<HTMLElement | null>;
  align: 'center' | 'start';
  keep?: boolean;
} & Omit<HTMLAttributes<HTMLDivElement>, 'popover'>) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    if (typeof element.showPopover === 'function') element.showPopover();
    else element.removeAttribute('popover');
  }, []);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    // A passage the conversation re-rendered away leaves a kept card at its last place, or at the
    // top of what can be seen when it never had one.
    let last: Box | undefined;
    const place = () => {
      const area = visibleArea(viewportRef.current);
      let anchor = rangeBox(range);
      if (anchor) last = anchor;
      else if (keep) anchor = last ?? { ...area, bottom: area.top };
      const size = element.getBoundingClientRect();
      const spot =
        anchor &&
        placeBeside(anchor, { width: size.width, height: size.height }, area, align, keep);
      element.style.visibility = spot ? '' : 'hidden';
      if (!spot) return;
      element.style.left = `${spot.left}px`;
      element.style.top = `${spot.top}px`;
    };
    place();
    // A card grows with the lines typed into it; above the selection it grows upward.
    const resized = new ResizeObserver(place);
    resized.observe(element);
    document.addEventListener('scroll', place, { capture: true, passive: true });
    window.addEventListener('resize', place);
    return () => {
      resized.disconnect();
      document.removeEventListener('scroll', place, true);
      window.removeEventListener('resize', place);
    };
  }, [range, viewportRef, align, keep]);
  return <div {...rest} data-quote-reply="" popover="manual" ref={ref} />;
}
