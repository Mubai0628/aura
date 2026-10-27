import { useEffect, useRef } from 'react';

import type { NotificationTarget } from '../../../shared/notifications';

/**
 * Tells Main which conversation is in front (none while settings are open), so it says nothing
 * about that one, and opens what a clicked notification was about.
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
