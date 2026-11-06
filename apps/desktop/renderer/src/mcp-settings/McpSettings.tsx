import { useEffect, useState } from 'react';
import type { McpRegistryView, McpServerView } from '@aura/contracts/generated';
import type { ResultEnvelope } from '../../../shared/aura-api';
import { McpAlerts, McpDetails, McpHeading, type McpChange } from './McpDetails';
import { McpServerForm } from './McpServerForm';
import { AppIcon } from '../app-icons';
import { mcpError, mcpNeedsTrust, mcpStateTone, mcpSwitchable, mcpTransport } from './mcp-labels';
import { SettingsItemDialog, SettingsRow, SettingsRowOpen } from '../ui/SettingsItem';
import { ListSentinel, SkeletonRows } from '../ui/SkeletonRows';
import { useIncrementalList } from '../ui/useIncrementalList';
import './mcp-settings.css';

const NONE: readonly never[] = [];

type Notice = { alert: boolean; text: string };

/** Settings → MCP: add servers, trust what they run, switch them and see their tools. */
export function McpSettings({ coreReady }: { coreReady: boolean }) {
  const [registry, setRegistry] = useState<McpRegistryView>();
  const [loadError, setLoadError] = useState<string>();
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState<string>();
  const [notice, setNotice] = useState<Notice>();
  const [adding, setAdding] = useState(false);
  const [opened, setOpened] = useState<string>();
  const [editing, setEditing] = useState<string>();
  const [trusting, setTrusting] = useState<string>();
  const [removing, setRemoving] = useState<string>();
  const [tools, setTools] = useState<string>();
  const [refreshed, setRefreshed] = useState(0);
  const [signInLink, setSignInLink] = useState<{ id: string; url: string }>();
  useEffect(() => {
    if (!coreReady) return;
    let live = true;
    void window.aura.v1.listMcpServers({}).then(
      (result) => {
        if (!live) return;
        if (result.ok) {
          setRegistry(result.value);
          setLoadError(undefined);
        } else setLoadError(mcpError(result.error.message, 'MCP 服务器列表读取失败，请重试。'));
      },
      (reason: unknown) => {
        if (live) setLoadError(mcpError(reason, 'MCP 服务器列表读取失败，请重试。'));
      },
    );
    return () => {
      live = false;
    };
  }, [coreReady, reload]);
  // A server that is starting, or a sign-in that waits for the browser, is read again until
  // it settles.
  const starting =
    registry?.servers.some(
      (server) => server.state === 'starting' || server.sign_in?.pending === true,
    ) ?? false;
  useEffect(() => {
    if (!starting) return;
    const timer = setTimeout(() => setReload((value) => value + 1), 1500);
    return () => clearTimeout(timer);
  }, [starting, registry]);

  async function run(label: string, work: () => Promise<Notice | undefined>) {
    if (busy) return;
    setBusy(label);
    setNotice(undefined);
    try {
      const outcome = await work();
      if (outcome) setNotice(outcome);
      if (outcome?.alert) setReload((value) => value + 1);
    } catch (reason) {
      setNotice({ alert: true, text: mcpError(reason, '操作结果未确认，请刷新后核对。') });
      setReload((value) => value + 1);
    } finally {
      setBusy(undefined);
    }
  }
  const applied = (result: ResultEnvelope<McpRegistryView>, done: string): Notice => {
    if (!result.ok) return { alert: true, text: mcpError(result.error.message) };
    setRegistry(result.value);
    return { alert: false, text: done };
  };
  const revision = registry?.revision ?? 0;
  const servers = registry?.servers ?? NONE;
  const pages = useIncrementalList(servers);
  // A server that leaves the list, removed here or no longer found, takes its dialog with it.
  const shown =
    opened === undefined ? undefined : servers.find((server) => server.config.id === opened);
  // A dialog shows what is done in it, never the outcome of an earlier action on another item.
  const openItem = (id: string) => {
    setNotice(undefined);
    setOpened(id);
  };
  const close = () => {
    setOpened(undefined);
    setEditing(undefined);
    setTrusting(undefined);
    setRemoving(undefined);
    setTools(undefined);
  };
  const saved = (next: McpRegistryView, id: string) => {
    setRegistry(next);
    const state = next.servers.find((server) => server.config.id === id)?.state;
    const waits = state === 'not_trusted' || state === 'config_changed';
    setNotice({ alert: false, text: waits ? `已保存 ${id}；信任后才会启动` : `已保存 ${id}` });
  };

  const trust = (server: McpServerView) =>
    run(`trust:${server.config.id}`, async () => {
      setTrusting(undefined);
      return applied(
        await window.aura.v1.trustMcpServer({
          id: server.config.id,
          expected_fingerprint: server.fingerprint ?? '',
          expected_revision: revision,
        }),
        `已信任并启用 ${server.config.display_name}，正在列出工具`,
      );
    });
  const change = (server: McpServerView, action: McpChange) =>
    run(`${action}:${server.config.id}`, async () => {
      const request = { id: server.config.id, expected_revision: revision };
      const api = window.aura.v1;
      const name = server.config.display_name;
      switch (action) {
        case 'enable':
        case 'disable':
          return applied(
            await api.setMcpServerEnabled({ ...request, enabled: action === 'enable' }),
            action === 'enable' ? `已启用 ${name}` : `已停用 ${name}，正在运行的实例已停止`,
          );
        case 'untrust':
          return applied(await api.untrustMcpServer(request), `已撤销对 ${name} 的信任`);
        case 'remove': {
          setRemoving(undefined);
          const done = applied(await api.removeMcpServer(request), `已移除 ${name}`);
          // A server added again later under the same ID does not reopen this dialog.
          if (!done.alert) close();
          return done;
        }
        case 'restore':
          return applied(await api.restoreMcpPluginDefault(request), `已恢复 ${name} 的插件配置`);
      }
    });
  const signIn = (server: McpServerView) =>
    run(`sign-in:${server.config.id}`, async () => {
      const result = await window.aura.v1.signInMcpServer({ id: server.config.id });
      setReload((value) => value + 1);
      if (!result.ok) return { alert: true, text: mcpError(result.error.message) };
      setSignInLink({ id: server.config.id, url: result.value.authorization_url });
      return {
        alert: false,
        text: `已在浏览器中打开 ${result.value.authorization_server} 的登录页面；完成后这里会自动更新`,
      };
    });
  const signOut = (server: McpServerView) =>
    run(`sign-out:${server.config.id}`, async () =>
      applied(
        await window.aura.v1.signOutMcpServer({ id: server.config.id }),
        `已退出 ${server.config.display_name} 的登录`,
      ),
    );
  const test = (server: McpServerView) =>
    run(`refresh:${server.config.id}`, async () => {
      const result = await window.aura.v1.refreshMcpServer({ id: server.config.id });
      setReload((value) => value + 1);
      setRefreshed((value) => value + 1);
      setTools(server.config.id);
      if (!result.ok) return { alert: true, text: mcpError(result.error.message) };
      return result.value.server.state === 'ready'
        ? {
            alert: false,
            text: `${server.config.display_name} 连接成功，列出 ${result.value.tools.length} 个工具`,
          }
        : {
            alert: true,
            text: `${server.config.display_name} 没有连接成功：${result.value.server.last_error ?? '请查看错误输出'}`,
          };
    });

  const row = (server: McpServerView, index: number) => {
    const { config } = server;
    const tone = mcpStateTone(server.state);
    return (
      <SettingsRow
        key={config.id}
        state={server.state}
        tone={tone === 'success' || tone === 'active' ? undefined : tone}
        index={index}
        headingClassName="mcp-row-heading"
        heading={<McpHeading server={server} />}
        summary={mcpTransport(server)}
        alerts={<McpAlerts server={server} />}
        actions={
          <>
            {mcpNeedsTrust(server) && server.fingerprint ? (
              <button
                type="button"
                className="button is-primary is-small"
                disabled={busy !== undefined}
                onClick={() => {
                  openItem(config.id);
                  setTrusting(config.id);
                }}
              >
                {server.state === 'config_changed' ? '查看并重新信任' : '查看并信任'}
              </button>
            ) : null}
            {mcpSwitchable(server) ? (
              <button
                type="button"
                className="button is-small"
                disabled={busy !== undefined}
                onClick={() => void change(server, config.enabled ? 'disable' : 'enable')}
              >
                {config.enabled ? '停用' : '启用'}
              </button>
            ) : null}
            <SettingsRowOpen name={config.display_name} onOpen={() => openItem(config.id)} />
          </>
        }
      />
    );
  };

  return (
    <section className="mcp-settings settings-page" aria-label="MCP 设置">
      <header className="settings-content-heading">
        <div>
          <h2>MCP</h2>
        </div>
        <div className="settings-page-actions">
          <button
            type="button"
            className="button is-primary"
            disabled={!coreReady || busy !== undefined}
            onClick={() => setAdding(true)}
          >
            <AppIcon name="plus" size={15} />
            添加服务器
          </button>
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
      {adding ? (
        <McpServerForm
          revision={revision}
          onCancel={() => setAdding(false)}
          onSaved={(next, id) => {
            setAdding(false);
            saved(next, id);
          }}
        />
      ) : null}
      {!coreReady ? (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，MCP 管理暂不可用。
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
        <SkeletonRows label="正在读取 MCP 服务器" />
      ) : servers.length ? (
        <ul className="mcp-list settings-card-list" aria-label="MCP 服务器">
          {pages.shown.map(row)}
          {pages.more ? <ListSentinel sentinel={pages.sentinel} /> : null}
        </ul>
      ) : (
        <div className="settings-empty">
          <span className="settings-empty-icon" aria-hidden="true">
            <AppIcon name="code" size={22} />
          </span>
          <strong>还没有 MCP 服务器</strong>
          <p>点“添加服务器”填写命令或地址。</p>
        </div>
      )}
      {shown ? (
        <SettingsItemDialog
          title={shown.config.display_name}
          heading={<McpHeading server={shown} named={false} />}
          headingClassName="mcp-row-heading"
          notice={notice}
          onClose={close}
        >
          {editing === shown.config.id ? (
            <McpServerForm
              initial={shown.config}
              revision={revision}
              onCancel={() => setEditing(undefined)}
              onSaved={(next, id) => {
                setEditing(undefined);
                saved(next, id);
              }}
            />
          ) : (
            <McpDetails
              server={shown}
              busy={busy}
              link={signInLink?.id === shown.config.id ? signInLink.url : undefined}
              trusting={trusting === shown.config.id}
              removing={removing === shown.config.id}
              toolsOpen={tools === shown.config.id}
              refreshed={refreshed}
              onAskTrust={() => setTrusting(shown.config.id)}
              onCancelTrust={() => setTrusting(undefined)}
              onTrust={() => void trust(shown)}
              onChange={(action) => void change(shown, action)}
              onTest={() => void test(shown)}
              onToggleTools={() =>
                setTools(tools === shown.config.id ? undefined : shown.config.id)
              }
              onEdit={() => {
                setNotice(undefined);
                setEditing(shown.config.id);
              }}
              onAskRemove={() => setRemoving(shown.config.id)}
              onCancelRemove={() => setRemoving(undefined)}
              onSignIn={() => void signIn(shown)}
              onSignOut={() => void signOut(shown)}
              onSecretSaved={(text, alert) => {
                setNotice({ alert, text });
                setReload((value) => value + 1);
              }}
            />
          )}
        </SettingsItemDialog>
      ) : null}
    </section>
  );
}
