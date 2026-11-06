import { useEffect } from 'react';

import { applyThemeSheet, themeCss } from './theme-css';

/** Core's refusal names the plugin's theme: it cannot be used now. */
const REFUSED = /^plugins\./u;

/**
 * The theme chosen in 设置 → 环境, while its plugin can give it. It is read again each time the
 * window comes back to the front, so a theme being written shows its changes; when it cannot be
 * used, Aura's own shows.
 */
export function usePluginTheme(name: string | null | undefined): void {
  useEffect(() => {
    const read = window.aura?.v1.getPluginTheme;
    if (!name || typeof read !== 'function') {
      applyThemeSheet(undefined);
      return;
    }
    let live = true;
    const load = () => {
      read({ name }).then(
        (result) => {
          if (!live) return;
          if (result.ok) applyThemeSheet(themeCss(result.value));
          else if (REFUSED.test(result.error.message)) applyThemeSheet(undefined);
        },
        // Core out of reach says nothing about the theme; what is shown stays.
        () => undefined,
      );
    };
    load();
    window.addEventListener('focus', load);
    return () => {
      live = false;
      window.removeEventListener('focus', load);
    };
  }, [name]);
}
