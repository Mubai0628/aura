import { useCallback, useEffect, useRef, useState, type FocusEvent } from 'react';
import type {
  AgentWaitActivity,
  AgentWaitActivityView,
  AgentWaitTarget,
} from '@aura/contracts/generated';

const HEARTBEAT_MS = 4_000;

function visible(element: HTMLElement | null): boolean {
  return Boolean(
    element?.isConnected &&
    document.visibilityState !== 'hidden' &&
    element.checkVisibility?.({ visibilityProperty: true }) !== false,
  );
}

export function useHumanWait<Element extends HTMLElement = HTMLElement>(
  sessionId: string,
  target: AgentWaitTarget,
  enabled = true,
) {
  const id = target.kind === 'query' ? target.query_id : target.approval_id;
  const [viewer] = useState(() => crypto.randomUUID());
  const [state, setState] = useState<{ id: string; value: AgentWaitActivityView }>();
  const [held, setHeld] = useState(false);
  const active = useRef(false);
  const [root, mount] = useState<Element | null>(null);
  const inView = useRef(false);
  const sequence = useRef(0);
  const applied = useRef(0);
  const send = useCallback(
    (activity: AgentWaitActivity) => {
      if (!enabled || typeof window.aura?.v1.updateAgentWait !== 'function') return;
      const serial = ++sequence.current;
      void window.aura.v1
        .updateAgentWait({ session_id: sessionId, target, viewer, activity })
        .then((result) => {
          if (result.ok && serial >= applied.current) {
            applied.current = serial;
            setState({ id, value: result.value });
          }
        })
        .catch(() => undefined);
    },
    [enabled, id, sessionId, target, viewer],
  );
  const change = useCallback(
    (focused: boolean) => {
      if (!enabled || active.current === focused) return;
      active.current = focused;
      setHeld(focused);
      send(focused ? 'focus' : 'blur');
    },
    [enabled, send],
  );
  useEffect(() => {
    if (!enabled || !root) return;
    const refresh = () => {
      const shown = inView.current && visible(root);
      const focused = shown && document.hasFocus() && root.contains(document.activeElement);
      if (focused !== active.current) change(focused);
      else if (shown) send(focused ? 'focus' : 'present');
    };
    let watching = true;
    const observer =
      typeof IntersectionObserver === 'function'
        ? new IntersectionObserver(
            (entries) => {
              if (!watching) return;
              const entry = entries.findLast((item) => item.target === root);
              if (!entry) return;
              inView.current =
                entry.isIntersecting &&
                entry.intersectionRect.width > 0 &&
                entry.intersectionRect.height > 0;
              refresh();
            },
            { threshold: [0, 0.001] },
          )
        : undefined;
    inView.current = observer === undefined;
    if (observer) observer.observe(root);
    else refresh();
    const release = () => change(false);
    const timer = window.setInterval(refresh, HEARTBEAT_MS);
    window.addEventListener('focus', refresh);
    window.addEventListener('blur', release);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      watching = false;
      observer?.disconnect();
      inView.current = false;
      window.clearInterval(timer);
      window.removeEventListener('focus', refresh);
      window.removeEventListener('blur', release);
      document.removeEventListener('visibilitychange', refresh);
      if (active.current) release();
    };
  }, [change, enabled, root, send]);
  const value = state?.id === id ? state.value : undefined;
  return {
    mount,
    deadlineAt: value?.deadline_at,
    focusedUntil: value?.focused_until,
    held,
    focus: () => {
      if (inView.current && visible(root) && document.hasFocus()) change(true);
    },
    blur: (event: FocusEvent<HTMLElement>) => {
      if (!(event.relatedTarget instanceof Node) || !root?.contains(event.relatedTarget))
        change(false);
    },
    touch: () => {
      if (!active.current && inView.current && visible(root)) send('touch');
    },
  };
}
