import { useMemo, useRef } from 'react';

/** The order of one source's reads: which answers are still news. */
export interface ReadOrder {
  /** A read starts: its number. */
  begin: () => number;
  /** A read answered: whether its answer may be shown, and if so it is now the newest shown. */
  accept: (read: number) => boolean;
  /** Something changed the source (an action finished): every read still out is stale. */
  settle: () => void;
}

/**
 * Orders the answers of every read of one source — the periodic one, a manual retry, the read
 * after an action. An answer to a read issued before the newest answer shown, or before the source
 * last changed, is dropped: a slow old answer cannot bring back a state that is already gone.
 */
export function useReadOrder(): ReadOrder {
  const order = useRef({ issued: 0, applied: 0 });
  return useMemo(
    () => ({
      begin: () => {
        order.current.issued += 1;
        return order.current.issued;
      },
      accept: (read) => {
        if (read <= order.current.applied) return false;
        order.current.applied = read;
        return true;
      },
      settle: () => {
        order.current.applied = order.current.issued;
      },
    }),
    [],
  );
}
