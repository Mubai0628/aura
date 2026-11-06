import type { McpServerView } from '@aura/contracts/generated';
import { McpPluginSecrets } from './McpPluginSecrets';
import { McpServerPanel } from './McpServerPanel';
import { McpSignInRow } from './McpSignInRow';
import {
  authSummary,
  mcpNeedsTrust,
  mcpStateLabel,
  mcpStateTone,
  mcpSwitchable,
  transportSummary,
  trustSummary,
} from './mcp-labels';

export type McpChange = 'enable' | 'disable' | 'untrust' | 'remove' | 'restore';

/** A server's name, ID, plugin and state. */
export function McpHeading({ server, named = true }: { server: McpServerView; named?: boolean }) {
  const { config } = server;
  return (
    <>
      {named ? <strong>{config.display_name}</strong> : null}
      <code>{config.id}</code>
      {server.plugin ? (
        <span className="settings-tag mcp-row-source">插件 {server.plugin}</span>
      ) : null}
      {server.plugin_changed ? <span className="settings-tag">已修改</span> : null}
      <span className="status-pill mcp-row-state" data-tone={mcpStateTone(server.state)}>
        {mcpStateLabel(server)}
      </span>
    </>
  );
}

/** What keeps a server from running: its configuration's problems, or why it did not start. */
export function McpAlerts({ server }: { server: McpServerView }) {
  return (
    <>
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
    </>
  );
}

/** One server in its dialog: how it connects, its sign-in, its tools and the user's choices. */
export function McpDetails({
  server,
  busy,
  link,
  trusting,
  removing,
  toolsOpen,
  refreshed,
  onAskTrust,
  onCancelTrust,
  onTrust,
  onChange,
  onTest,
  onToggleTools,
  onEdit,
  onAskRemove,
  onCancelRemove,
  onSignIn,
  onSignOut,
  onSecretSaved,
}: {
  server: McpServerView;
  busy: string | undefined;
  link: string | undefined;
  trusting: boolean;
  removing: boolean;
  toolsOpen: boolean;
  refreshed: number;
  onAskTrust: () => void;
  onCancelTrust: () => void;
  onTrust: () => void;
  onChange: (action: McpChange) => void;
  onTest: () => void;
  onToggleTools: () => void;
  onEdit: () => void;
  onAskRemove: () => void;
  onCancelRemove: () => void;
  onSignIn: () => void;
  onSignOut: () => void;
  onSecretSaved: (text: string, alert: boolean) => void;
}) {
  const { config } = server;
  const locked = busy !== undefined;
  // A plugin's server is trusted and removed with its plugin.
  const plugin = server.plugin ?? undefined;
  return (
    <div className="mcp-details">
      <p className="mcp-row-transport">
        {config.transport.type === 'stdio' ? '本机程序：' : 'HTTP：'}
        <code>{transportSummary(config)}</code>
      </p>
      <McpAlerts server={server} />
      <McpSignInRow
        server={server}
        link={link}
        locked={locked}
        onSignIn={onSignIn}
        onSignOut={onSignOut}
      />
      <div className="settings-card-actions">
        {mcpNeedsTrust(server) && server.fingerprint ? (
          <button
            type="button"
            className="button is-primary is-small"
            disabled={locked}
            onClick={onAskTrust}
          >
            {server.state === 'config_changed' ? '查看并重新信任' : '查看并信任'}
          </button>
        ) : null}
        {mcpSwitchable(server) ? (
          <button
            type="button"
            className="button is-small"
            disabled={locked}
            onClick={() => onChange(config.enabled ? 'disable' : 'enable')}
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
            onClick={onTest}
          >
            测试连接
          </button>
        ) : null}
        <button
          type="button"
          className="button is-ghost is-small"
          aria-expanded={toolsOpen}
          onClick={onToggleTools}
        >
          {toolsOpen ? '收起' : '工具与日志'}
        </button>
        <button type="button" className="button is-small" disabled={locked} onClick={onEdit}>
          编辑
        </button>
        {plugin && server.plugin_changed ? (
          <button
            type="button"
            className="button is-small"
            disabled={locked}
            onClick={() => onChange('restore')}
          >
            恢复插件默认
          </button>
        ) : null}
        {plugin ? null : (
          <>
            {config.trusted_fingerprint ? (
              <button
                type="button"
                className="button is-small"
                disabled={locked}
                onClick={() => onChange('untrust')}
              >
                撤销信任
              </button>
            ) : null}
            <button
              type="button"
              className="button is-danger is-small mcp-remove"
              disabled={locked}
              onClick={onAskRemove}
            >
              移除
            </button>
          </>
        )}
      </div>
      {plugin ? <McpPluginSecrets server={server} locked={locked} onSaved={onSecretSaved} /> : null}
      {trusting ? (
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
              onClick={onTrust}
            >
              信任并启用
            </button>
            <button type="button" className="button is-small" onClick={onCancelTrust}>
              取消
            </button>
          </div>
        </div>
      ) : null}
      {removing ? (
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
              onClick={() => onChange('remove')}
            >
              移除
            </button>
            <button type="button" className="button is-small" onClick={onCancelRemove}>
              取消
            </button>
          </div>
        </div>
      ) : null}
      {toolsOpen ? <McpServerPanel server={server} refresh={refreshed} /> : null}
    </div>
  );
}
