import type { PluginPanelsApiV1 } from '../../../shared/plugin-panel-api';

/** The preload's plugin panel bridge, when this window has one. */
export const pluginPanelsApi = (): PluginPanelsApiV1 | undefined => window.aura?.v1.pluginPanels;
