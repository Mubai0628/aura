import { useEffect, useState } from 'react';
import type { McpRegistryView, McpServerView } from '@aura/contracts/generated';
import type { ResultEnvelope } from '../../../shared/aura-api';
import { McpPluginSecrets } from './McpPluginSecrets';
import { McpServerForm } from './McpServerForm';
import { McpServerPanel } from './McpServerPanel';
import { McpSignInRow } from './McpSignInRow';
import type { CSSProperties } from 'react';
import { AppIcon } from '../app-icons';
import {
  authSummary,
  mcpError,
  mcpStateLabel,
  mcpStateTone,
  transportSummary,
  trustSummary,
} from './mcp-labels';
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
  const [editing, setEditing] = useState<McpServerView | 'new'>();
  const [trusting, setTrusting] = useState<string>();
  const [removing, setRemoving] = useState<string>();
  const [open, setOpen] = useState<string>();
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
  const change = (server: McpServerView, action: 'enable' | 'disable' | 'untrust' | 'remove') =>
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
        case 'remove':
          setRemoving(undefined);
          return applied(await api.removeMcpServer(request), `已移除 ${name}`);
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
      setOpen(server.config.id);
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
    const locked = busy !== undefined;
    const needsTrust = server.state === 'not_trusted' || server.state === 'config_changed';
    // A plugin's server is edited, switched and removed with its plugin.
    const plugin = server.plugin ?? undefined;
    const tone = mcpStateTone(server.state);
    return (
      <li
        key={config.id}
        className="settings-card"
        data-state={server.state}
        data-tone={tone === 'success' || tone === 'active' ? undefined : tone}
        style={{ '--item': index } as CSSProperties}
      >
        <div className="settings-card-heading mcp-row-heading">
          <strong>{config.display_name}</strong>
          <code>{config.id}</code>
          {plugin ? <span className="settings-tag mcp-row-source">插件 {plugin}</span> : null}
          <span className="status-pill mcp-row-state" data-tone={tone}>
            {mcpStateLabel(server)}
          </span>
        </div>
        <p className="mcp-row-transport">
          {config.transport.type === 'stdio' ? '本机程序：' : 'HTTP：'}
          <code>{transportSummary(config)}</code>
        </p>
        {server.problems.length ? (
          <p className="settings-inline-alert" role="alert">
            {server.problems.join('；')}
          </p>
        ) : null}
        {server.last_error && server.state === 'failed' ? (
          <p className="settings-inline-alert" role="alert">
            {server.last_error}
          </p>
        ) : null}
        <McpSignInRow
          server={server}
          link={signInLink?.id === config.id ? signInLink.url : undefined}
          locked={locked}
          onSignIn={() => void signIn(server)}
          onSignOut={() => void signOut(server)}
        />
        <div className="settings-card-actions">
          {needsTrust && server.fingerprint ? (
            <button
              type="button"
              className="button is-primary is-small"
              disabled={locked}
              onClick={() => setTrusting(config.id)}
            >
              {server.state === 'config_changed' ? '查看并重新信任' : '查看并信任'}
            </button>
          ) : null}
          {!plugin &&
          (server.state === 'disabled' ||
            (config.enabled && !needsTrust && server.state !== 'invalid')) ? (
            <button
              type="button"
              className="button is-small"
              disabled={locked}
              onClick={() => void change(server, config.enabled ? 'disable' : 'enable')}
            >
              {config.enabled ? '停用' : '启用'}
            </button>
          ) : null}
          {server.state === 'ready' || server.state === 'failed' ? (
            <button
              type="button"
              className="button is-small"
              disabled={locked}
              aria-busy={busy === `refresh:${config.id}` ? true : undefined}
              onClick={() => void test(server)}
            >
              测试连接
            </button>
          ) : null}
          <button
            type="button"
            className="button is-ghost is-small"
            aria-expanded={open === config.id}
            onClick={() => setOpen(open === config.id ? undefined : config.id)}
          >
            {open === config.id ? '收起' : '工具与日志'}
          </button>
          {plugin ? null : (
            <>
              <button
                type="button"
                className="button is-small"
                disabled={locked}
                onClick={() => setEditing(server)}
              >
                编辑
              </button>
              {config.trusted_fingerprint ? (
                <button
                  type="button"
                  className="button is-small"
                  disabled={locked}
                  onClick={() => void change(server, 'untrust')}
                >
                  撤销信任
                </button>
              ) : null}
              <button
                type="button"
                className="button is-danger is-small mcp-remove"
                disabled={locked}
                onClick={() => setRemoving(config.id)}
              >
                移除
              </button>
            </>
          )}
        </div>
        {plugin ? (
          <>
            <McpPluginSecrets
              server={server}
              locked={locked}
              onSaved={(text, alert) => {
                setNotice({ alert, text });
                setReload((value) => value + 1);
              }}
            />
            <p className="mcp-row-note">随插件 {plugin} 信任与启用；编辑与移除请到 设置 → 插件。</p>
          </>
        ) : null}
        {trusting === config.id ? (
          <div
            role="alertdialog"
            aria-label={`信任 ${config.display_name}`}
            className="mcp-trust settings-panel is-accent"
          >
            <p>信任后 Aura 会{config.transport.type === 'stdio' ? '以你的身份运行' : '连接'}：</p>
            <pre>
              {server.resolved_command
                ? `${server.resolved_command.path}\nSHA-256 ${server.resolved_command.sha256}\n参数：${
                    config.transport.type === 'stdio'
                      ? (config.transport.arguments ?? []).join(' ')
                      : ''
                  }${authSummary(config) ? `\n${authSummary(config)}` : ''}`
                : trustSummary(config)}
            </pre>
            {/* One sentence, here and nowhere else on the page. */}
            <p>
              {config.transport.type === 'stdio'
                ? '它能做你能做的事；只信任你了解的服务器。'
                : config.transport.auth?.type === 'oauth'
                  ? '工具调用的参数会发到这个地址，信任后还要在浏览器中登录；只信任你了解的服务器。'
                  : '工具调用的参数会发到这个地址；只信任你了解的服务器。'}
            </p>
            <div className="settings-card-actions">
              <button
                type="button"
                className="button is-primary is-small"
                aria-busy={busy === `trust:${config.id}` ? true : undefined}
                onClick={() => void trust(server)}
              >
                信任并启用
              </button>
              <button
                type="button"
                className="button is-small"
                onClick={() => setTrusting(undefined)}
              >
                取消
              </button>
            </div>
          </div>
        ) : null}
        {removing === config.id ? (
          <div
            role="alertdialog"
            aria-label={`移除 ${config.display_name}`}
            className="settings-panel is-danger"
          >
            <p>移除后它的工具不再可用；程序文件不会被删除。</p>
            <div className="settings-card-actions">
              <button
                type="button"
                className="button is-danger is-solid is-small"
                onClick={() => void change(server, 'remove')}
              >
                移除
              </button>
              <button
                type="button"
                className="button is-small"
                onClick={() => setRemoving(undefined)}
              >
                取消
              </button>
            </div>
          </div>
        ) : null}
        {open === config.id ? <McpServerPanel server={server} refresh={refreshed} /> : null}
      </li>
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
            onClick={() => setEditing('new')}
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
      {editing ? (
        <McpServerForm
          initial={editing === 'new' ? undefined : editing.config}
          revision={revision}
          onCancel={() => setEditing(undefined)}
          onSaved={(saved, id) => {
            setRegistry(saved);
            setEditing(undefined);
            const state = saved.servers.find((server) => server.config.id === id)?.state;
            const waits = state === 'not_trusted' || state === 'config_changed';
            setNotice({
              alert: false,
              text: waits ? `已保存 ${id}；信任后才会启动` : `已保存 ${id}`,
            });
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
    </section>
  );
}
