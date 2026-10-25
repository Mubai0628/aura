import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import './scroll-box.css';

/**
 * An expanded step's content keeps a bounded height. When it is longer than
 * that, the box shows a dashed frame and scrolls by itself, and it can be scrolled from the keyboard.
 */
export function ScrollBox({
  children,
  className,
  label,
}: {
  children: ReactNode;
  className?: string;
  /** What the content is, for a box that has to be scrolled. */
  label: string;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  useLayoutEffect(() => {
    const element = box.current;
    if (!element) return;
    const measure = () => setOverflowing(element.scrollHeight > element.clientHeight + 1);
    measure();
    // The box's own size changes with the window; its content changes as a call goes on.
    const resized = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(measure);
    resized?.observe(element);
    const changed = new MutationObserver(measure);
    changed.observe(element, { characterData: true, childList: true, subtree: true });
    return () => {
      resized?.disconnect();
      changed.disconnect();
    };
  }, []);
  return (
    <div
      aria-label={overflowing ? label : undefined}
      className={className ? `scroll-box ${className}` : 'scroll-box'}
      data-overflowing={overflowing ? 'true' : undefined}
      ref={box}
      role={overflowing ? 'region' : undefined}
      tabIndex={overflowing ? 0 : undefined}
    >
      {children}
    </div>
  );
}
