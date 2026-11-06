import { useEffect, useState } from 'react';

import type { PluginStatusEntry, PluginUiApiV1, PluginUiTone } from '../../../shared/plugin-ui-api';
import type { NotificationInput, NotificationTone } from '../notification-center-store';

/** The preload's bridge to plugins' notices and status entries, when this window has one. */
export const pluginUiApi = (): PluginUiApiV1 | undefined => window.aura?.v1.pluginUi;

/** The colour of each tone of a status entry. */
export const STATUS_TONE: Record<PluginUiTone, 'muted' | 'success' | 'warning' | 'danger'> = {
  info: 'muted',
  success: 'success',
  warning: 'warning',
  error: 'danger',
};

const NOTICE_TONE: Record<PluginUiTone, NotificationTone> = {
  info: 'info',
  success: 'success',
  warning: 'warning',
  error: 'danger',
};

/** The plugins' status entries shown now, kept up to date. */
export function usePluginStatuses(): PluginStatusEntry[] {
  const [statuses, setStatuses] = useState<PluginStatusEntry[]>([]);
  useEffect(() => {
    const api = pluginUiApi();
    if (api === undefined) return;
    let live = true;
    // A change that arrives first is newer than the entries asked for.
    let changed = false;
    const stop = api.onChange((change) => {
      if (change.kind !== 'statuses') return;
      changed = true;
      setStatuses(change.statuses);
    });
    void api.statuses().then((shown) => {
      if (live && !changed) setStatuses(shown);
    });
    return () => {
      live = false;
      stop();
    };
  }, []);
  return statuses;
}

/** Each notice a plugin shows goes to the notification center, under the plugin's name. */
export function usePluginNotices(publish: (input: NotificationInput) => void): void {
  useEffect(
    () =>
      pluginUiApi()?.onChange((change) => {
        if (change.kind !== 'notice') return;
        const { plugin, text, tone } = change.notice;
        publish({
          dedupeKey: `plugin:${plugin}:${text}`,
          title: `插件 ${plugin}`,
          message: text,
          tone: NOTICE_TONE[tone],
        });
      }),
    [publish],
  );
}
