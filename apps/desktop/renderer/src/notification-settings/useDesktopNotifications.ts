import { useEffect, useRef } from 'react';

import { UPDATES_IN_FRONT, type NotificationTarget } from '../../../shared/notifications';

/** What is in front: the conversation shown, 关于与更新 while it shows, or nothing. */
export function frontOf(view: {
  screen: string;
  section: string;
  session: string | null;
}): string | null {
  if (view.screen === 'workspace') return view.session;
  return view.screen === 'settings' && view.section === 'about' ? UPDATES_IN_FRONT : null;
}

/**
 * Tells Main what is in front (see `frontOf`), so it says nothing about that and takes its red dot
 * away, and opens what a clicked notification was about.
 */
export function useDesktopNotifications(
  frontSession: string | null,
  open: (target: NotificationTarget) => void,
): void {
  const latest = useRef(open);
  useEffect(() => {
    latest.current = open;
  });
  useEffect(() => {
    window.aura?.v1?.notifications?.setForegroundSession(frontSession);
  }, [frontSession]);
  useEffect(
    () => window.aura?.v1?.notifications?.onNotificationOpen((target) => latest.current(target)),
    [],
  );
}
