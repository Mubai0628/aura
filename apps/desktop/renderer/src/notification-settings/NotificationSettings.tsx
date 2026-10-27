import { useEffect, useState } from 'react';

import type { NotificationPreferences } from '../../../shared/notifications';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import '../browser-settings/browser-settings.css';
import './notification-settings.css';

const SWITCHES: readonly { kind: keyof NotificationPreferences; label: string }[] = [
  { kind: 'updates', label: '有新版本' },
  { kind: 'approvals', label: '需要审批' },
  { kind: 'questions', label: '模型提问或等你决定' },
  { kind: 'completions', label: '任务完成' },
  { kind: 'failures', label: '任务出错或暂停' },
];

/**
 * 设置 → 通知: which Windows notifications Aura shows. A conversation's notifications come only
 * while it is not in front.
 */
export function NotificationSettings() {
  const api = window.aura?.v1?.notifications;
  const [preferences, setPreferences] = useState<NotificationPreferences>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    let alive = true;
    void api
      ?.notificationPreferences({ kind: 'get' })
      .then((read) => {
        if (alive) setPreferences(read);
      })
      .catch(() => {
        if (alive) setError('通知设置没有读到，请稍后再试。');
      });
    return () => {
      alive = false;
    };
  }, [api]);
  const save = async (next: NotificationPreferences) => {
    setError(undefined);
    setPreferences(next);
    try {
      setPreferences(await api!.notificationPreferences({ kind: 'set', preferences: next }));
    } catch {
      setError('通知设置没有保存，请重试。');
    }
  };
  return (
    <div className="settings-page notification-settings">
      <div className="settings-content-heading">
        <div>
          <h2>通知</h2>
        </div>
      </div>
      <section aria-label="通知" className="settings-surface notification-settings-list">
        {SWITCHES.map(({ kind, label }) => (
          <label className="browser-settings-toggle" key={kind}>
            <ToggleSwitch
              checked={preferences?.[kind] ?? false}
              disabled={preferences === undefined}
              label={label}
              onCheckedChange={(checked) =>
                preferences && void save({ ...preferences, [kind]: checked })
              }
            />
            <span>{label}</span>
          </label>
        ))}
      </section>
      {error === undefined ? null : (
        <p className="settings-status-line" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
