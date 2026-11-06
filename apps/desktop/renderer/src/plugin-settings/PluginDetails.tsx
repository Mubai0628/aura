import type {
  HookEntryView,
  McpServerView,
  PluginConfigureRequest,
  PluginOption,
  PluginPreview,
  PluginView,
} from '@aura/contracts/generated';
import { AppIcon } from '../app-icons';
import type { SettingsSection } from '../app-shell-types';
import { PluginOptions } from './PluginOptions';
import { PluginPreviewPanel } from './PluginPreviewPanel';
import { PluginRemote } from './PluginRemote';
import { PluginRuntime } from './PluginRuntime';
import { PluginService } from './PluginService';
import { countsLabel, limitsLabel, pluginTrust } from './plugin-labels';

export type PluginAction =
  | 'preview'
  | 'preview_update'
  | 'enable'
  | 'disable'
  | 'untrust'
  | 'discard_update'
  | 'folder'
  | 'export'
  | 'uninstall';

/** What the plugin page saves for a plugin, and what is said once it is saved. */
export type PluginChange = Omit<PluginConfigureRequest, 'name' | 'expected_revision'>;

/** One installed plugin in its dialog: what it brings, how that runs, and the user's choices. */
export function PluginDetails({
  plugin,
  busy,
  working,
  preview,
  servers,
  hooks,
  confirmingUninstall,
  onAction,
  onTrust,
  onClosePreview,
  onAskUninstall,
  onCancelUninstall,
  onOpenSection,
  onConfigure,
  onSetSecret,
}: {
  plugin: PluginView;
  busy: boolean;
  /** The action under way on this plugin, shown on its button. */
  working?: string | undefined;
  preview: PluginPreview | undefined;
  servers: McpServerView[];
  hooks: HookEntryView[];
  confirmingUninstall: boolean;
  onAction: (action: PluginAction) => void;
  onTrust: (preview: PluginPreview) => void;
  onClosePreview: () => void;
  onAskUninstall: () => void;
  onCancelUninstall: () => void;
  onOpenSection?: ((section: SettingsSection) => void) | undefined;
  onConfigure: (change: PluginChange, done: string) => void;
  onSetSecret: (option: PluginOption, bytes: Uint8Array) => void;
}) {
  const { usable, needsTrust } = pluginTrust(plugin);
  const update = plugin.pending_update;
  const busyOn = (action: PluginAction) => (working === action ? true : undefined);
  // The button that shows content (the current version or the waiting update) hides it.
  const showsCurrent = preview !== undefined && !preview.pending;
  const showsUpdate = preview?.pending === true;
  return (
    <div className="plugin-details" data-state={plugin.state}>
      {plugin.description ? <p className="plugin-description">{plugin.description}</p> : null}
      <p className="settings-card-meta">
        <span>{countsLabel(plugin.counts)}</span>
        {plugin.author ? <span>作者 {plugin.author}</span> : null}
        <span>{limitsLabel(plugin)}</span>
      </p>
      {plugin.development ? (
        <p className="settings-card-meta plugin-folder" title={plugin.development}>
          文件夹 {plugin.development}
        </p>
      ) : null}
      {plugin.problems.length ? (
        <p className="settings-status-line" role="alert">
          {plugin.problems.join('；')}
        </p>
      ) : null}
      {plugin.warnings.length ? (
        <p className="plugin-warning">{plugin.warnings.join('；')}</p>
      ) : null}
      {plugin.unsupported.length ? (
        <p className="plugin-unsupported">Aura 不使用：{plugin.unsupported.join('；')}</p>
      ) : null}
      {update ? (
        <div className="plugin-update settings-panel is-accent" role="status">
          <span>
            新版本 {update.version ?? ''} 已导入，等你查看并信任；现在仍在用{' '}
            {plugin.version ?? '当前版本'}。
          </span>
          {update.problems.length ? <p role="alert">{update.problems.join('；')}</p> : null}
          {update.changed_servers?.length ? (
            <p className="plugin-warning">
              你改过配置、新版本也改了的服务器：{update.changed_servers.join('、')}
            </p>
          ) : null}
          <div className="settings-card-actions">
            {update.fingerprint ? (
              <button
                type="button"
                className={showsUpdate ? 'button is-small' : 'button is-primary is-small'}
                disabled={busy}
                aria-busy={busyOn('preview_update')}
                aria-expanded={showsUpdate}
                onClick={() => (showsUpdate ? onClosePreview() : onAction('preview_update'))}
              >
                {showsUpdate ? '收起' : '查看新版本'}
              </button>
            ) : null}
            <button
              type="button"
              className="button is-small"
              disabled={busy}
              aria-busy={busyOn('discard_update')}
              onClick={() => onAction('discard_update')}
            >
              放弃新版本
            </button>
          </div>
        </div>
      ) : null}
      <PluginService
        plugin={plugin}
        busy={busy}
        onSwitch={(on) =>
          onConfigure(
            { service: on },
            on ? `已开启 ${plugin.name} 的后台服务` : `已关闭 ${plugin.name} 的后台服务`,
          )
        }
      />
      <PluginOptions
        plugin={plugin}
        busy={busy}
        onSaveText={(text) => onConfigure({ text }, `已保存 ${plugin.name} 的设置`)}
        onSetSecret={onSetSecret}
        onClearSecret={(option) =>
          onConfigure({ forget_secrets: [option.key] }, `已清除 ${option.title}`)
        }
      />
      <PluginRemote plugin={plugin} busy={busy} onConfigure={onConfigure} />
      <PluginRuntime servers={servers} hooks={hooks} onOpenSection={onOpenSection} />
      <div className="settings-card-actions">
        {usable ? (
          <button
            type="button"
            className={
              needsTrust && !showsCurrent ? 'button is-primary is-small' : 'button is-small'
            }
            disabled={busy}
            aria-busy={busyOn('preview')}
            aria-expanded={showsCurrent}
            onClick={() => (showsCurrent ? onClosePreview() : onAction('preview'))}
          >
            {showsCurrent
              ? '收起'
              : needsTrust
                ? plugin.state === 'content_changed'
                  ? '查看变化并重新信任'
                  : '查看并信任'
                : '查看内容'}
          </button>
        ) : null}
        {usable && !needsTrust ? (
          <button
            type="button"
            className="button is-small"
            disabled={busy}
            aria-busy={busyOn(plugin.enabled ? 'disable' : 'enable')}
            onClick={() => onAction(plugin.enabled ? 'disable' : 'enable')}
          >
            {plugin.enabled ? '停用' : '启用'}
          </button>
        ) : null}
        {plugin.trusted_fingerprint ? (
          <button
            type="button"
            className="button is-small"
            disabled={busy}
            aria-busy={busyOn('untrust')}
            onClick={() => onAction('untrust')}
          >
            撤销信任
          </button>
        ) : null}
        <button
          type="button"
          className="button is-ghost is-small"
          disabled={busy}
          onClick={() => onAction('folder')}
        >
          <AppIcon name="folder" size={14} />
          打开文件夹
        </button>
        {usable ? (
          <button
            type="button"
            className="button is-ghost is-small"
            disabled={busy}
            aria-busy={busyOn('export')}
            onClick={() => onAction('export')}
          >
            导出…
          </button>
        ) : null}
        <button
          type="button"
          className="button is-danger is-small plugin-uninstall"
          disabled={busy}
          onClick={onAskUninstall}
        >
          卸载
        </button>
      </div>
      {confirmingUninstall ? (
        <div
          className="plugin-confirm settings-panel is-danger"
          role="alertdialog"
          aria-label={`卸载 ${plugin.name}`}
        >
          <p>
            卸载会删除 Aura
            里的这份插件、它带来的全部内容，以及它的服务器的密钥与登录；你当初导入的原文件夹或 ZIP
            文件不受影响。
          </p>
          <div className="settings-card-actions">
            <button
              type="button"
              className="button is-danger is-solid is-small"
              aria-busy={busyOn('uninstall')}
              onClick={() => onAction('uninstall')}
            >
              卸载
            </button>
            <button type="button" className="button is-small" onClick={onCancelUninstall}>
              取消
            </button>
          </div>
        </div>
      ) : null}
      {preview ? (
        <PluginPreviewPanel
          preview={preview}
          busy={busy}
          onTrust={preview.pending || needsTrust ? () => onTrust(preview) : undefined}
        />
      ) : null}
    </div>
  );
}
