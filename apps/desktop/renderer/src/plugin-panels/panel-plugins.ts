import type { PluginView } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';

/** A plugin whose panel can be opened now, and the panel's title. */
export interface PanelPlugin {
  plugin: string;
  title: string;
}

/** The ready plugins with a panel that can be used. */
export function panelPlugins(plugins: readonly PluginView[]): PanelPlugin[] {
  return plugins.flatMap((view) =>
    view.state === 'ready' && typeof view.panel === 'string' && view.panel !== ''
      ? [{ plugin: view.name, title: view.panel }]
      : [],
  );
}

/**
 * The panels to offer, read again each time `asking` turns true: plugins are installed, trusted
 * and changed while Aura runs, so a list kept from before would offer what is gone.
 */
export function usePanelPlugins(asking: boolean): PanelPlugin[] {
  const [panels, setPanels] = useState<PanelPlugin[]>([]);
  useEffect(() => {
    const list = window.aura?.v1.listPlugins;
    if (!asking || typeof list !== 'function') return;
    let live = true;
    list({}).then(
      (result) => {
        if (live && result.ok) setPanels(panelPlugins(result.value.plugins));
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [asking]);
  return panels;
}
