import { useId, useRef, useState } from 'react';
import type { McpPreview, McpRegistryView, McpServerConfig } from '@aura/contracts/generated';
import { formatPairs, mcpError, parsePairs } from './mcp-labels';
import type { McpSecretInputHandle } from './McpSecretInput';
import { HttpSignIn, SecretEnvironment } from './McpSignIn';
import {
  httpAuth,
  initialClientId,
  initialHeader,
  initialMode,
  initialRows,
  initialScopes,
  oauthAuth,
  savedAuthSecret,
  secretEnvironment,
  writeSecret,
} from './mcp-sign-in-config';

type Kind = 'stdio' | 'http';

/** Add or change one server. Saving never trusts or switches it on; that is a separate step. */
export function McpServerForm({
  initial,
  revision,
  onSaved,
  onCancel,
}: {
  initial?: McpServerConfig | undefined;
  revision: number;
  onSaved: (registry: McpRegistryView, id: string) => void;
  onCancel: () => void;
}) {
  const stdio = initial?.transport.type === 'stdio' ? initial.transport : undefined;
  const http = initial?.transport.type === 'http' ? initial.transport : undefined;
  const [id, setId] = useState(initial?.id ?? '');
  const [displayName, setDisplayName] = useState(initial?.display_name ?? '');
  const [kind, setKind] = useState<Kind>(http ? 'http' : 'stdio');
  const [command, setCommand] = useState(stdio?.command ?? '');
  const [args, setArgs] = useState((stdio?.arguments ?? []).join('\n'));
  const [directory, setDirectory] = useState(stdio?.working_directory ?? '');
  const [environment, setEnvironment] = useState(formatPairs(stdio?.environment, '='));
  const [url, setUrl] = useState(http?.url ?? '');
  const [headers, setHeaders] = useState(formatPairs(http?.headers, ':'));
  const [startup, setStartup] = useState(String((initial?.startup_timeout_ms ?? 30_000) / 1000));
  const [call, setCall] = useState(String((initial?.call_timeout_ms ?? 120_000) / 1000));
  const [mode, setMode] = useState(initialMode(initial));
  const [authHeader, setAuthHeader] = useState(initialHeader(initial));
  const [clientId, setClientId] = useState(initialClientId(initial));
  const [scopes, setScopes] = useState(initialScopes(initial));
  // A secret written in an earlier attempt is kept, so retrying never writes another one.
  const [authSecretId, setAuthSecretId] = useState(savedAuthSecret(initial));
  const authSecret = useRef<McpSecretInputHandle>(null);
  const [secretRows, setSecretRows] = useState(initialRows(initial));
  const secretInputs = useRef(new Map<number, McpSecretInputHandle | null>());
  const [preview, setPreview] = useState<McpPreview>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const idHint = useId();

  const config = (authId = authSecretId, rows = secretRows): McpServerConfig => ({
    id: id.trim(),
    display_name: displayName.trim() || id.trim(),
    transport:
      kind === 'stdio'
        ? {
            type: 'stdio',
            command: command.trim(),
            arguments: args
              .split(/\r?\n/u)
              .map((line) => line.trim())
              .filter(Boolean),
            working_directory: directory.trim() || null,
            environment: parsePairs(environment, '='),
            ...(rows.length ? { secret_environment: secretEnvironment(rows) } : {}),
          }
        : {
            type: 'http',
            url: url.trim(),
            headers: parsePairs(headers, ':'),
            ...(mode === 'none'
              ? {}
              : {
                  auth:
                    mode === 'oauth'
                      ? oauthAuth(clientId, scopes)
                      : httpAuth(mode, authHeader, authId),
                }),
          },
    enabled: initial?.enabled ?? false,
    trusted_fingerprint: initial?.trusted_fingerprint ?? null,
    startup_timeout_ms: Math.round(Number(startup) * 1000),
    call_timeout_ms: Math.round(Number(call) * 1000),
    available_for_subagents: initial?.available_for_subagents ?? false,
  });

  async function work(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      await action();
    } catch (reason) {
      setError(mcpError(reason, '操作结果未确认，请刷新后核对。'));
    } finally {
      setBusy(false);
    }
  }
  const check = () =>
    work(async () => {
      const result = await window.aura.v1.previewMcpServer({ server: config() });
      if (result.ok) setPreview(result.value);
      else setError(mcpError(result.error.message));
    });
  const save = () =>
    work(async () => {
      // Secrets go first, each on the secret channel; the configuration names their IDs.
      const serverId = id.trim();
      let authId = authSecretId;
      if (kind === 'http' && (mode === 'bearer' || mode === 'header')) {
        authId = await writeSecret(serverId, authSecret.current, authSecretId, '认证密钥');
        setAuthSecretId(authId);
      }
      let rows = secretRows;
      if (kind === 'stdio') {
        rows = [];
        for (const row of secretRows) {
          const label = `环境变量 ${row.name.trim() || '（未命名）'} 的值`;
          const input = secretInputs.current.get(row.key);
          rows.push({ ...row, secretId: await writeSecret(serverId, input, row.secretId, label) });
        }
        setSecretRows(rows);
      }
      const server = config(authId, rows);
      const result = await window.aura.v1.saveMcpServer({
        server,
        replaces: initial?.id ?? null,
        expected_revision: revision,
      });
      if (result.ok) onSaved(result.value, server.id);
      else setError(mcpError(result.error.message));
    });

  return (
    <form
      className="mcp-form"
      aria-label={initial ? `编辑 ${initial.display_name}` : '添加 MCP 服务器'}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <label>
        服务器 ID
        <input
          value={id}
          disabled={initial !== undefined}
          placeholder="files"
          aria-describedby={idHint}
          onChange={(event) => setId(event.target.value)}
        />
      </label>
      <small id={idHint}>小写字母、数字和 -，保存后不能修改。</small>
      <label>
        显示名
        <input value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
      </label>
      <fieldset>
        <legend>连接方式</legend>
        <div className="mcp-choices">
          <label>
            <input
              type="radio"
              checked={kind === 'stdio'}
              onChange={() => setKind('stdio')}
              name="mcp-kind"
            />
            本机程序（stdio）
          </label>
          <label>
            <input
              type="radio"
              checked={kind === 'http'}
              onChange={() => setKind('http')}
              name="mcp-kind"
            />
            HTTP 地址（Streamable HTTP）
          </label>
        </div>
      </fieldset>
      {kind === 'stdio' ? (
        <>
          <label>
            命令
            <input
              value={command}
              placeholder="C:\Tools\server.exe 或 npx"
              onChange={(event) => setCommand(event.target.value)}
            />
          </label>
          <label>
            参数（每行一个）
            <textarea rows={3} value={args} onChange={(event) => setArgs(event.target.value)} />
          </label>
          <label>
            工作目录（可选）
            <input value={directory} onChange={(event) => setDirectory(event.target.value)} />
          </label>
          <label>
            环境变量（每行 名字=值，不要写密钥）
            <textarea
              rows={2}
              value={environment}
              onChange={(event) => setEnvironment(event.target.value)}
            />
          </label>
          <SecretEnvironment
            rows={secretRows}
            onRows={setSecretRows}
            inputs={secretInputs}
            disabled={busy}
          />
        </>
      ) : (
        <>
          <label>
            地址
            <input
              value={url}
              placeholder="https://example.com/mcp"
              onChange={(event) => setUrl(event.target.value)}
            />
          </label>
          <small>公网地址需要 https；本机或局域网的服务器写 IP 地址或 localhost。</small>
          <label>
            请求头（每行 名字: 值，不要写密钥）
            <textarea
              rows={2}
              value={headers}
              onChange={(event) => setHeaders(event.target.value)}
            />
          </label>
          <HttpSignIn
            mode={mode}
            onMode={setMode}
            header={authHeader}
            onHeader={setAuthHeader}
            clientId={clientId}
            onClientId={setClientId}
            scopes={scopes}
            onScopes={setScopes}
            saved={authSecretId !== undefined}
            secret={authSecret}
            disabled={busy}
          />
        </>
      )}
      <div className="mcp-form-row">
        <label>
          启动时限（秒）
          <input
            inputMode="numeric"
            value={startup}
            onChange={(event) => setStartup(event.target.value)}
          />
        </label>
        <label>
          调用时限（秒）
          <input
            inputMode="numeric"
            value={call}
            onChange={(event) => setCall(event.target.value)}
          />
        </label>
      </div>
      {preview ? (
        <div className="mcp-preview" role="status" aria-label="检查结果">
          {preview.problems.length ? (
            <ul>
              {preview.problems.map((problem) => (
                <li key={problem}>{problem}</li>
              ))}
            </ul>
          ) : (
            <p>配置可以使用。</p>
          )}
          {preview.resolved_command ? (
            <p>
              将运行：<code>{preview.resolved_command.path}</code>
            </p>
          ) : preview.problems.length === 0 && kind === 'http' ? (
            <p>
              将连接：<code>{url.trim()}</code>
            </p>
          ) : null}
        </div>
      ) : null}
      {error ? (
        <p className="settings-inline-alert" role="alert">
          {error}
        </p>
      ) : null}
      <div className="settings-card-actions">
        <button type="submit" className="button is-primary" disabled={busy}>
          保存
        </button>
        <button type="button" className="button" disabled={busy} onClick={() => void check()}>
          检查
        </button>
        <button type="button" className="button is-ghost" disabled={busy} onClick={onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}
