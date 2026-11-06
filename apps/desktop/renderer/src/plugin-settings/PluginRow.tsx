import type { PluginView } from '@aura/contracts/generated';
import type { PluginStatusEntry } from '../../../shared/plugin-ui-api';
import { AppIcon } from '../app-icons';
import { PluginStatusLine } from '../plugin-ui/PluginStatusLine';
import { SettingsRow, SettingsRowOpen } from '../ui/SettingsItem';
import type { PluginAction } from './PluginDetails';
import { pluginStateLabel, pluginStateTone, pluginTrust } from './plugin-labels';

/** A plugin's version, format and state, after its name in the row. */
export function PluginHeading({ plugin, named = true }: { plugin: PluginView; named?: boolean }) {
  return (
    <>
      {named ? (
        <>
          <span className="plugin-mark" aria-hidden="true">
            <AppIcon name="puzzle" size={16} />
          </span>
          <strong>{plugin.name}</strong>
        </>
      ) : null}
      {plugin.version ? <span className="plugin-version">{plugin.version}</span> : null}
      {plugin.layout === 'claude_code' ? (
        <span className="settings-tag">Claude Code 插件格式</span>
      ) : null}
      {plugin.development ? <span className="settings-tag">开发中</span> : null}
      {plugin.pending_update ? <span className="settings-tag">有新版本</span> : null}
      <span
        className="status-pill plugin-state"
        data-state={plugin.state}
        data-tone={pluginStateTone(plugin.state)}
      >
        {pluginStateLabel(plugin)}
      </span>
    </>
  );
}

/** One installed plugin in the list; the rest of it is in its dialog. */
export function PluginRow({
  plugin,
  index,
  busy,
  status,
  working,
  onOpen,
  onReview,
  onAction,
}: {
  plugin: PluginView;
  index: number;
  busy: boolean;
  /** The status entry the plugin shows now, if any. */
  status?: PluginStatusEntry | undefined;
  working?: string | undefined;
  onOpen: () => void;
  /** Opens the dialog with what the plugin brings, to be trusted there. */
  onReview: () => void;
  onAction: (action: PluginAction) => void;
}) {
  const { usable, needsTrust } = pluginTrust(plugin);
  const tone = pluginStateTone(plugin.state);
  const toggle = plugin.enabled ? 'disable' : 'enable';
  return (
    <SettingsRow
      className="plugin-card"
      state={plugin.state}
      tone={tone === 'success' ? undefined : tone}
      index={index}
      heading={<PluginHeading plugin={plugin} />}
      summary={plugin.description || undefined}
      alerts={
        <>
          {status ? <PluginStatusLine entry={status} /> : null}
          {plugin.problems.length ? (
            <p className="settings-status-line" role="alert">
              {plugin.problems.join('；')}
            </p>
          ) : null}
        </>
      }
      actions={
        <>
          {needsTrust ? (
            <button
              type="button"
              className="button is-primary is-small"
              disabled={busy}
              onClick={onReview}
            >
              {plugin.state === 'content_changed' ? '查看变化并重新信任' : '查看并信任'}
            </button>
          ) : null}
          {usable && !needsTrust ? (
            <button
              type="button"
              className="button is-small"
              disabled={busy}
              aria-busy={working === toggle ? true : undefined}
              onClick={() => onAction(toggle)}
            >
              {plugin.enabled ? '停用' : '启用'}
            </button>
          ) : null}
          <SettingsRowOpen name={plugin.name} onOpen={onOpen} />
        </>
      }
    />
  );
}
