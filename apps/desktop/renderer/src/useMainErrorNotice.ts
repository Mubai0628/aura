import { useCallback, useEffect, useRef, useState } from 'react';
import type { MainErrorNoticeV1 } from '../../shared/main-errors-api';
import type { NotificationItem } from './notification-center-store';

/** One persistent notice: later errors replace it; dismissing survives stale snapshots. */
export function useMainErrorNotice() {
  const [notice, setNotice] = useState<MainErrorNoticeV1>();
  const seen = useRef(0);
  useEffect(() => {
    const api = window.aura.v1.mainErrors;
    if (!api) return;
    let alive = true;
    const receive = (next: MainErrorNoticeV1) => {
      const sequence = Number(next.id.slice('main_error_'.length));
      if (!alive || sequence <= seen.current) return;
      seen.current = sequence;
      setNotice(next);
    };
    const unsubscribe = api.onChange(receive);
    void api.snapshot().then((result) => {
      if (result.ok && result.value) receive(result.value);
    });
    return () => {
      alive = false;
      unsubscribe();
    };
  }, []);
  const dismiss = useCallback(
    (id: string) => setNotice((previous) => (previous?.id === id ? undefined : previous)),
    [],
  );
  const item: NotificationItem | undefined = notice
    ? {
        id: notice.id,
        dedupeKey: 'main-error',
        title: 'Aura 主进程发生错误',
        message: `${notice.logged ? '错误已写入日志。' : '日志写入失败，请检查日志目录权限或磁盘空间。'}请保存当前工作；若问题持续，请重启 Aura。日志：${notice.log_path}`,
        tone: 'danger',
      }
    : undefined;
  return { item, dismiss };
}
