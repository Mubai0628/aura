import { useCallback, useEffect, useRef, useState } from 'react';

export type NotificationTone = 'danger' | 'info' | 'success' | 'warning';

export interface NotificationInput {
  dedupeKey: string;
  message: string;
  title: string;
  tone: NotificationTone;
}

export interface NotificationItem extends NotificationInput {
  id: string;
}

export function useNotificationCenter(autoDismissMs = 4_000) {
  const [items, setItems] = useState<NotificationItem[]>([]);
  const nextId = useRef(0);
  const activeKeys = useRef(new Set<string>());
  const timers = useRef(new Map<string, number>());

  const dismiss = useCallback((id: string) => {
    const timer = timers.current.get(id);
    if (timer !== undefined) window.clearTimeout(timer);
    timers.current.delete(id);
    setItems((current) => {
      const removed = current.find((item) => item.id === id);
      if (removed !== undefined) activeKeys.current.delete(removed.dedupeKey);
      return current.filter((item) => item.id !== id);
    });
  }, []);

  const publish = useCallback(
    (input: NotificationInput) => {
      if (activeKeys.current.has(input.dedupeKey)) return;
      activeKeys.current.add(input.dedupeKey);
      nextId.current += 1;
      const id = `ui_notice_${nextId.current}`;
      setItems((current) => [...current, { ...input, id }]);
      // A failure says what went wrong and what to do next, so it
      // stays until it is closed; every other notice leaves by itself.
      if (input.tone !== 'danger') {
        timers.current.set(
          id,
          window.setTimeout(() => dismiss(id), autoDismissMs),
        );
      }
    },
    [autoDismissMs, dismiss],
  );

  /**
   * Withdraws the notices a state raised (their keys start with `prefix`) once that state is fine
   * again: a failure stays until it is closed, but not after the screen shows it was resolved.
   */
  const withdraw = useCallback((prefix: string) => {
    setItems((current) => {
      const resolved = current.filter((item) => item.dedupeKey.startsWith(prefix));
      if (resolved.length === 0) return current;
      for (const item of resolved) {
        activeKeys.current.delete(item.dedupeKey);
        const timer = timers.current.get(item.id);
        if (timer !== undefined) window.clearTimeout(timer);
        timers.current.delete(item.id);
      }
      return current.filter((item) => !item.dedupeKey.startsWith(prefix));
    });
  }, []);

  useEffect(
    () => () => {
      for (const timer of timers.current.values()) window.clearTimeout(timer);
      timers.current.clear();
      activeKeys.current.clear();
    },
    [],
  );

  return { dismiss, items, publish, withdraw };
}
