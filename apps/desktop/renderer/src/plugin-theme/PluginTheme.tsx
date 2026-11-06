import { useUiSettings } from '../ui-settings-store';
import { usePluginTheme } from './usePluginTheme';

/** Applies the theme chosen in 设置 → 环境 to the whole window. */
export function PluginTheme() {
  usePluginTheme(useUiSettings().settings.plugin_theme);
  return null;
}
