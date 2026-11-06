import type { PluginStatusEntry } from '../../../shared/plugin-ui-api';
import { STATUS_TONE } from './plugin-ui';
import './plugin-ui.css';

/** A plugin's status entry, in the plugin's row on the 插件 settings page. */
export function PluginStatusLine({ entry }: { entry: PluginStatusEntry }) {
  return (
    <p
      className={`plugin-status-line state-${STATUS_TONE[entry.tone]}`}
      title={`插件 ${entry.plugin}：${entry.text}`}
    >
      <span aria-hidden="true" />
      <span className="plugin-status-text">{entry.text}</span>
    </p>
  );
}
