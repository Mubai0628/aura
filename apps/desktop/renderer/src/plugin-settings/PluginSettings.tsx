import { useEffect, useState } from 'react';
import type {
  HookEntryView,
  McpServerView,
  PluginPreview,
  PluginRegistryView,
  PluginView,
} from '@aura/contracts/generated';
import type { ResultEnvelope } from '../../../shared/aura-api';
import { AppIcon } from '../app-icons';
import type { SettingsSection } from '../app-shell-types';
import { PluginCard, type PluginAction } from './PluginCard';
import { pluginError } from './plugin-labels';
import { ListSentinel, SkeletonRows } from '../ui/SkeletonRows';
import { useIncrementalList } from '../ui/useIncrementalList';
import './plugin-settings.css';

const NONE: readonly never[] = [];

type Notice = { alert: boolean; text: string };
type Running = { servers: McpServerView[]; hooks: HookEntryView[]; issue?: string };

/** Settings → 插件: the installed plugins, what each brings, and the user's choices. */
export function PluginSettings({
  coreReady,
  onOpenSection,
}: {
  coreReady: boolean;
  onOpenSection?: ((section: SettingsSection) => void) | undefined;
}) {
  const [registry, setRegistry] = useState<PluginRegistryView>();
  const [loadError, setLoadError] = useState<string>();
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState<string>();
  const [notice, setNotice] = useState<Notice>();
  const [preview, setPreview] = useState<PluginPreview>();
  const [uninstalling, setUninstalling] = useState<string>();
  const [running, setRunning] = useState<Running>({ servers: [], hooks: [] });
  const [poll, setPoll] = useState(0);
  useEffect(() => {
    if (!coreReady) return;
    let live = true;
    void window.aura.v1.listPlugins({}).then(
      (result) => {
        if (!live) return;
        if (result.ok) {
          setRegistry(result.value);
          setLoadError(undefined);
        } else setLoadError(pluginError(result.error.message, '插件列表读取失败，请重试。'));
      },
      (reason: unknown) => {
        if (live) setLoadError(pluginError(reason, '插件列表读取失败，请重试。'));
      },
    );
    return () => {
      live = false;
    };
  }, [coreReady, reload]);

  // How what the plugins bring is running: their servers and Hooks, read with the list and again
  // while a server is starting (the MCP page does the same).
  useEffect(() => {
    if (!coreReady || !registry) return;
    let live = true;
    void (async () => {
      try {
        const [servers, hooks] = await Promise.all([
          window.aura.v1.listMcpServers({}),
          window.aura.v1.listHooks({}),
        ]);
        if (!live) return;
        const issue = [servers, hooks]
          .map((result) => (result.ok ? undefined : pluginError(result.error.message)))
          .find(Boolean);
        setRunning({
          servers: servers.ok ? servers.value.servers.filter((server) => server.plugin) : [],
          hooks: hooks.ok ? hooks.value.entries.filter((hook) => hook.plugin) : [],
          ...(issue ? { issue } : {}),
        });
      } catch (reason) {
        if (live)
          setRunning({ servers: [], hooks: [], issue: pluginError(reason, '运行情况读取失败。') });
      }
    })();
    return () => {
      live = false;
    };
  }, [coreReady, registry, poll]);
  const starting = running.servers.some((server) => server.state === 'starting');
  useEffect(() => {
    if (!starting) return;
    const timer = setTimeout(() => setPoll((value) => value + 1), 1500);
    return () => clearTimeout(timer);
  }, [starting, running]);

  async function run(label: string, work: () => Promise<Notice | undefined>) {
    if (busy) return;
    setBusy(label);
    setNotice(undefined);
    try {
      const outcome = await work();
      if (outcome) setNotice(outcome);
      if (outcome?.alert) setReload((value) => value + 1);
    } catch (reason) {
      setNotice({ alert: true, text: pluginError(reason, '操作结果未确认，请刷新后核对。') });
      setReload((value) => value + 1);
    } finally {
      setBusy(undefined);
    }
  }
  const applied = (result: ResultEnvelope<PluginRegistryView>, done: string): Notice => {
    if (!result.ok) return { alert: true, text: pluginError(result.error.message) };
    setRegistry(result.value);
    return { alert: false, text: done };
  };
  const revision = registry?.revision ?? 0;
  const plugins = registry?.plugins ?? NONE;
  const pages = useIncrementalList(plugins);
  const change = { expected_revision: revision };

  const act = (plugin: PluginView, action: PluginAction) =>
    run(`${action}:${plugin.name}`, async () => {
      const api = window.aura.v1;
      const name = plugin.name;
      switch (action) {
        case 'preview':
        case 'preview_update': {
          const result = await api.previewPlugin({ name, pending: action === 'preview_update' });
          if (!result.ok) return { alert: true, text: pluginError(result.error.message) };
          setPreview(result.value);
          return undefined;
        }
        case 'enable':
        case 'disable':
          return applied(
            await api.setPluginEnabled({ ...change, name, enabled: action === 'enable' }),
            action === 'enable' ? `已启用 ${name}` : `已停用 ${name}；它带来的内容都已撤下`,
          );
        case 'untrust':
          return applied(
            await api.untrustPlugin({ ...change, name }),
            `已撤销对 ${name} 的信任；它带来的内容都已撤下`,
          );
        case 'discard_update':
          return applied(
            await api.discardPluginUpdate({ ...change, name }),
            `已放弃 ${name} 的新版本`,
          );
        case 'folder': {
          // Main opens the folder Core lists for this plugin; the page never sends a path.
          const result = await api.openPluginDirectory({ name, pending: false });
          if (!result.ok) return { alert: true, text: pluginError(result.error.message) };
          return { alert: !result.value.completed, text: result.value.message };
        }
        case 'uninstall':
          setUninstalling(undefined);
          if (preview?.plugin.name === name) setPreview(undefined);
          return applied(await api.uninstallPlugin({ ...change, name }), `已卸载 ${name}`);
      }
    });
  const trust = (shown: PluginPreview) =>
    run(`trust:${shown.plugin.name}`, async () => {
      const done = applied(
        await window.aura.v1.trustPlugin({
          ...change,
          name: shown.plugin.name,
          pending: shown.pending,
          expected_fingerprint: shown.fingerprint ?? '',
        }),
        shown.pending ? `已更新并信任 ${shown.plugin.name}` : `已信任并启用 ${shown.plugin.name}`,
      );
      if (!done.alert) setPreview(undefined);
      return done;
    });
  // Main shows the dialog; Core copies a folder or unpacks a ZIP file and checks it.
  const importFrom = (kind: 'folder' | 'archive') =>
    run(`import:${kind}`, async () => {
      const chosen =
        kind === 'folder'
          ? await window.aura.v1.selectPluginDirectory({})
          : await window.aura.v1.selectPluginArchive({});
      if (!chosen.ok) return { alert: true, text: pluginError(chosen.error.message) };
      if (!chosen.value.path) return undefined;
      return applied(
        await window.aura.v1.importPlugin({ path: chosen.value.path, expected_revision: revision }),
        '已导入；查看它带来的内容并信任后才会生效',
      );
    });

  const importButtons = (
    <>
      <button
        type="button"
        className="button is-primary"
        disabled={!coreReady || busy !== undefined}
        aria-busy={busy === 'import:folder' ? true : undefined}
        onClick={() => void importFrom('folder')}
      >
        <AppIcon name="folder" size={15} />
        导入文件夹…
      </button>
      <button
        type="button"
        className="button"
        disabled={!coreReady || busy !== undefined}
        aria-busy={busy === 'import:archive' ? true : undefined}
        onClick={() => void importFrom('archive')}
      >
        导入 ZIP…
      </button>
    </>
  );
  return (
    <section className="plugin-settings settings-page" aria-label="插件设置">
      <header className="settings-content-heading">
        <div>
          <h2>插件</h2>
        </div>
        <div className="settings-page-actions">
          {importButtons}
          <button
            type="button"
            className="button is-ghost"
            aria-label="刷新"
            title="刷新"
            onClick={() => setReload((value) => value + 1)}
          >
            <AppIcon name="refresh" size={15} />
          </button>
        </div>
      </header>
      {notice ? (
        <p
          className={notice.alert ? 'settings-status-line' : 'settings-status-line is-done'}
          role={notice.alert ? 'alert' : 'status'}
        >
          {notice.text}
        </p>
      ) : null}
      {registry?.issue ? (
        <p className="settings-status-line" role="alert">
          {registry.issue}
        </p>
      ) : null}
      {running.issue ? (
        <p className="settings-status-line">运行情况暂时读不到：{running.issue}</p>
      ) : null}
      {!coreReady ? (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，插件管理暂不可用。
        </p>
      ) : loadError ? (
        <p className="settings-status-line" role="alert">
          {loadError}{' '}
          <button
            type="button"
            className="button is-small"
            onClick={() => setReload((value) => value + 1)}
          >
            重试
          </button>
        </p>
      ) : !registry ? (
        <SkeletonRows label="正在读取插件" />
      ) : plugins.length ? (
        <ul className="plugin-list settings-card-list" aria-label="插件">
          {pages.shown.map((plugin, index) => (
            <PluginCard
              key={plugin.name}
              plugin={plugin}
              index={index}
              busy={busy !== undefined}
              working={
                busy?.endsWith(`:${plugin.name}`) ? busy.slice(0, busy.lastIndexOf(':')) : undefined
              }
              preview={preview?.plugin.name === plugin.name ? preview : undefined}
              servers={running.servers.filter((server) => server.plugin === plugin.name)}
              hooks={running.hooks.filter((hook) => hook.plugin === plugin.name)}
              confirmingUninstall={uninstalling === plugin.name}
              onAction={(action) => void act(plugin, action)}
              onTrust={(shown) => void trust(shown)}
              onClosePreview={() => setPreview(undefined)}
              onAskUninstall={() => setUninstalling(plugin.name)}
              onCancelUninstall={() => setUninstalling(undefined)}
              onOpenSection={onOpenSection}
            />
          ))}
          {pages.more ? <ListSentinel sentinel={pages.sentinel} /> : null}
        </ul>
      ) : (
        <div className="settings-empty plugin-empty">
          <span className="settings-empty-icon" aria-hidden="true">
            <AppIcon name="puzzle" size={22} />
          </span>
          <strong>还没有插件</strong>
          <p>
            导入一个含 aura-plugin.toml（或 Claude Code 插件的
            .claude-plugin/plugin.json）的文件夹或 ZIP 文件。
          </p>
        </div>
      )}
    </section>
  );
}
