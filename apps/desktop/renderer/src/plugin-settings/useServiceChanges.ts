import { useEffect } from 'react';
import { pluginUiApi } from '../plugin-ui/plugin-ui';

/** Calls `changed` once a plugin's background service changed state, at most every half second. */
export function useServiceChanges(active: boolean, changed: () => void): void {
  useEffect(() => {
    const ui = pluginUiApi();
    if (!active || !ui) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stop = ui.onChange((change) => {
      if (change.kind !== 'service' || timer !== undefined) return;
      timer = setTimeout(() => {
        timer = undefined;
        changed();
      }, 500);
    });
    return () => {
      stop();
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [active, changed]);
}
