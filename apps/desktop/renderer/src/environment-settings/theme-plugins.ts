import type { PluginView } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';

export interface ThemeOption {
  /** The plugin's name; none is Aura's own. */
  name: string | null;
  title: string;
}

/** The ready plugins whose theme can be used, with its title. */
export function themePlugins(plugins: readonly PluginView[]): ThemeOption[] {
  return plugins.flatMap((view) =>
    view.state === 'ready' && typeof view.theme === 'string' && view.theme !== ''
      ? [{ name: view.name, title: view.theme }]
      : [],
  );
}

/** Read each time the page opens: plugins are installed, trusted and changed while Aura runs. */
export function useThemePlugins(): ThemeOption[] | undefined {
  const [themes, setThemes] = useState<ThemeOption[]>();
  useEffect(() => {
    const list = window.aura?.v1.listPlugins;
    if (typeof list !== 'function') return;
    let live = true;
    list({}).then(
      (result) => {
        if (live && result.ok) setThemes(themePlugins(result.value.plugins));
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  return themes;
}
