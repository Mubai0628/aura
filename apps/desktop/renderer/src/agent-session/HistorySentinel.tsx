import type { RefObject } from 'react';
import { useEffect, useRef, useState } from 'react';

/** How far above the sentinel still counts as "the reader is arriving". */
const HISTORY_MARGIN = '600px';

/** The result of one page request.
 *
 *  - `true` — the history really moved, so there may be a next page.
 *  - `false` — the request ran and brought nothing: it failed, or the source
 *    answered with what was already on screen. Automatic attempts stop here.
 *  - `undefined` — nothing was requested (a read is already in flight, or the
 *    cursor disappeared). The sentinel stays armed instead of reporting an
 *    error the reader did not cause. */
export type HistoryPageLoad = () => Promise<boolean | undefined>;

/** Automatic paging for earlier history.
 *
 *  The sentinel sits where the older content will appear. When the viewport
 *  comes near it, that page is read without the reader pressing anything, and
 *  the caller keeps the scroll position stable while the page lands. Normal
 *  reading never needs a button: only a request that brought nothing stops the
 *  automatic attempts and offers a retry, so a failing source cannot loop. */
export function HistorySentinel({
  cursor,
  label,
  onLoad,
  root,
}: {
  /** Identity of the page that would be read now. A different value means the
   *  last page really arrived. */
  cursor: string;
  /** What the older content is, for the retry affordance and for tests. */
  label: string;
  onLoad: HistoryPageLoad;
  /** The scrolling element the sentinel is measured against. */
  root: RefObject<HTMLDivElement | null>;
}) {
  const element = useRef<HTMLDivElement | null>(null);
  const load = useRef(onLoad);
  const cursorNow = useRef(cursor);
  const attempt = useRef<() => void>(() => undefined);
  // The page that brought nothing, so the retry belongs to this cursor and a
  // real page clears the marker without an extra render pass.
  const [stalledAt, setStalledAt] = useState<string | null>(null);
  const busy = useRef(false);
  // A load is only allowed once per approach: the sentinel consumes the
  // intersection signal and waits until the reader leaves and comes back, so a
  // page that lands without moving the sentinel cannot chain-load the whole
  // session behind the reader's back.
  const armed = useRef(true);
  const stalled = stalledAt === cursor;

  useEffect(() => {
    load.current = onLoad;
  }, [onLoad]);

  useEffect(() => {
    // The cursor a page request belongs to. A new cursor makes any older stall
    // marker stale by comparison (`stalled` is a match, not a flag), so a page
    // that really arrived clears the retry affordance on its own.
    cursorNow.current = cursor;
  }, [cursor]);

  useEffect(() => {
    const target = element.current;
    if (!target) return;
    const run = async (): Promise<void> => {
      if (busy.current) return;
      busy.current = true;
      armed.current = false;
      // The page this attempt belongs to, so a stall is reported against the
      // cursor that failed and not against a page that arrived meanwhile.
      const requested = cursorNow.current;
      let moved: boolean | undefined;
      try {
        moved = await load.current();
      } catch {
        moved = false;
      } finally {
        busy.current = false;
      }
      if (moved === false) setStalledAt(requested);
      else if (moved === undefined) armed.current = true;
    };
    attempt.current = () => void run();
    if (typeof IntersectionObserver !== 'function') {
      // Without the observer there is no viewport information at all: ask once
      // for the next page so earlier history is still reachable.
      void run();
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries.at(-1);
        if (!entry) return;
        if (!entry.isIntersecting) {
          // The reader moved away from the sentinel: the next approach may ask
          // for the following page.
          armed.current = true;
          return;
        }
        if (armed.current) void run();
      },
      { root: root.current ?? null, rootMargin: HISTORY_MARGIN },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [root]);

  return (
    <div className="agent-history-sentinel" ref={element} data-history-cursor={cursor}>
      {stalled ? (
        <p role="status">
          更早的消息暂时无法加载。
          <button
            type="button"
            onClick={() => {
              setStalledAt(null);
              armed.current = true;
              attempt.current();
            }}
          >
            重试加载{label}
          </button>
        </p>
      ) : null}
    </div>
  );
}
