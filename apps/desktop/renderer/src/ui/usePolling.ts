import { useEffect } from 'react';

/**
 * Reads now, then `ms` after each read settles: never two at once, so a slow answer is not followed
 * by a queue of requests behind it. `read` is told whether its answer still belongs to the current
 * effect; a new `read` (its inputs changed) or an unmount ends the old one. A failed read waits the
 * same time before the next.
 */
export function usePolling(
  read: ((live: () => boolean) => Promise<unknown>) | undefined,
  ms: number,
): void {
  useEffect(() => {
    if (read === undefined) return;
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = () => {
      void Promise.resolve()
        .then(() => read(() => live))
        .catch(() => undefined)
        .finally(() => {
          if (live) timer = setTimeout(tick, ms);
        });
    };
    tick();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [read, ms]);
}
