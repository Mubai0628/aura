import { useCallback, useState } from 'react';

/** How far beyond what is shown a thing is read, so a short scroll finds it already there. */
const AHEAD = '200px';

/**
 * Whether the element given the returned ref has come near the view; once near, it stays near.
 * Lists that hold hundreds of pictures read each one only when it is about to be seen. Where
 * there is no IntersectionObserver, everything counts as near.
 */
export function useNearView<T extends Element>(): [(element: T | null) => () => void, boolean] {
  const [near, setNear] = useState(() => typeof IntersectionObserver !== 'function');
  const ref = useCallback(
    (element: T | null) => {
      if (near || element === null) return () => {};
      const observer = new IntersectionObserver(
        (entries) => {
          if (!entries.some((entry) => entry.isIntersecting)) return;
          observer.disconnect();
          setNear(true);
        },
        // `scrollMargin` reaches into scrolled panes such as Settings; `rootMargin` covers the window.
        { rootMargin: AHEAD, scrollMargin: AHEAD } as IntersectionObserverInit,
      );
      observer.observe(element);
      return () => observer.disconnect();
    },
    [near],
  );
  return [ref, near];
}
