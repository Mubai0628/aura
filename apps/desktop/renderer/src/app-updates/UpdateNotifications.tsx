import type { NotificationInput } from '../notification-center-store';
import { useEffect, useRef } from 'react';
import { useAppUpdates } from './useAppUpdates';
import { userMessage } from '../agent-session/user-message';

export function UpdateNotifications({
  onNotify,
  onWithdraw,
}: {
  onNotify: (notice: NotificationInput) => void;
  onWithdraw: (prefix: string) => void;
}) {
  const { state } = useAppUpdates();
  const sent = useRef(new Set<string>());
  const active = useRef<{ available: string | undefined; notice: string | undefined }>({
    available: undefined,
    notice: undefined,
  });
  useEffect(() => {
    if (!state) return;
    const publish = (
      kind: 'available' | 'notice',
      key: string | undefined,
      input: NotificationInput | undefined,
    ) => {
      if (active.current[kind] === key) return;
      if (active.current[kind]) onWithdraw(`app-update:${kind}:`);
      active.current[kind] = key;
      if (!key || !input || sent.current.has(key)) return;
      sent.current.add(key);
      onNotify(input);
    };
    const available = state.available && state.phase === 'available' ? state.latest : null;
    const availableKey = available ? `app-update:available:${available.version}` : undefined;
    publish(
      'available',
      availableKey,
      available && availableKey
        ? {
            dedupeKey: availableKey,
            title: '发现新版本',
            message: `Aura ${available.version} 已可更新，可前往设置 → 关于与更新查看。`,
            tone: 'info',
          }
        : undefined,
    );
    const notice = state.notice;
    const noticeKey = notice ? `app-update:notice:${JSON.stringify(notice)}` : undefined;
    publish(
      'notice',
      noticeKey,
      notice && noticeKey
        ? {
            dedupeKey: noticeKey,
            title: notice.going_back
              ? `回退失败，已返回 ${notice.kept_version}`
              : `更新失败，已自动退回 ${notice.kept_version}`,
            message: `${notice.going_back ? `未能回退到 Aura ${notice.version}。` : `Aura ${notice.version} 未能启动。`}\n${userMessage(notice.detail, '请在设置 → 关于与更新中查看当前版本后重试。')}`,
            tone: 'danger',
          }
        : undefined,
    );
  }, [state, onNotify, onWithdraw]);
  return null;
}
