import type { MutableRefObject, Ref } from 'react';
import { McpSecretInput, type McpSecretInputHandle } from './McpSecretInput';
import type { HttpMode, SecretRow } from './mcp-sign-in-config';

/** How an HTTP server signs in: nothing, a bearer secret or a secret in a named header. */
export function HttpSignIn({
  mode,
  onMode,
  header,
  onHeader,
  clientId,
  onClientId,
  scopes,
  onScopes,
  saved,
  secret,
  disabled,
}: {
  mode: HttpMode;
  onMode: (mode: HttpMode) => void;
  header: string;
  onHeader: (header: string) => void;
  clientId: string;
  onClientId: (clientId: string) => void;
  scopes: string;
  onScopes: (scopes: string) => void;
  saved: boolean;
  secret: Ref<McpSecretInputHandle>;
  disabled: boolean;
}) {
  const choice = (value: HttpMode, label: string) => (
    <label>
      <input type="radio" name="mcp-auth" checked={mode === value} onChange={() => onMode(value)} />
      {label}
    </label>
  );
  // The ways to sign in are one row of choices; what the chosen way needs is rows of its own
  // below it.
  return (
    <fieldset>
      <legend>认证</legend>
      <div className="mcp-choices">
        {choice('none', '不需要')}
        {choice('bearer', '密钥（Authorization: Bearer）')}
        {choice('header', '密钥（放在指定的请求头）')}
        {choice('oauth', 'OAuth 登录（在浏览器中登录）')}
      </div>
      {mode === 'header' ? (
        <div className="mcp-auth-fields">
          <label>
            放密钥的请求头
            <input
              value={header}
              placeholder="X-Api-Key"
              onChange={(event) => onHeader(event.target.value)}
            />
          </label>
        </div>
      ) : null}
      {mode === 'oauth' ? (
        <div className="mcp-auth-fields">
          <label>
            客户端 ID（可选）
            <input
              value={clientId}
              placeholder="留空时自动注册"
              onChange={(event) => onClientId(event.target.value)}
            />
          </label>
          <label>
            权限（可选，空格分隔）
            <input
              value={scopes}
              placeholder="留空时按服务器的要求"
              onChange={(event) => onScopes(event.target.value)}
            />
          </label>
          <small>保存并信任后，在服务器列表里点“在浏览器中登录”。</small>
        </div>
      ) : null}
      {mode === 'none' || mode === 'oauth' ? null : (
        <div className="mcp-auth-fields">
          <McpSecretInput ref={secret} label="密钥" saved={saved} disabled={disabled} />
          <small>密钥只保存在 Windows 凭据管理器。</small>
        </div>
      )}
    </fieldset>
  );
}

/** A program's secret environment variables: names here, values only in the secret channel. */
export function SecretEnvironment({
  rows,
  onRows,
  inputs,
  disabled,
}: {
  rows: SecretRow[];
  onRows: (rows: SecretRow[]) => void;
  inputs: MutableRefObject<Map<number, McpSecretInputHandle | null>>;
  disabled: boolean;
}) {
  const next = rows.reduce((max, row) => Math.max(max, row.key + 1), 0);
  return (
    <fieldset>
      <legend>带密钥的环境变量</legend>
      {rows.map((row) => (
        <div className="mcp-form-row" key={row.key}>
          <label>
            变量名
            <input
              value={row.name}
              placeholder="GITHUB_TOKEN"
              onChange={(event) =>
                onRows(
                  rows.map((r) => (r.key === row.key ? { ...r, name: event.target.value } : r)),
                )
              }
            />
          </label>
          <McpSecretInput
            ref={(handle) => {
              inputs.current.set(row.key, handle);
            }}
            label={`${row.name.trim() || '这个变量'}的值`}
            saved={row.secretId !== undefined}
            disabled={disabled}
          />
          <button type="button" onClick={() => onRows(rows.filter((r) => r.key !== row.key))}>
            移除
          </button>
        </div>
      ))}
      <button
        type="button"
        disabled={disabled || rows.length >= 8}
        onClick={() => onRows([...rows, { key: next, name: '', secretId: undefined }])}
      >
        添加变量
      </button>
      <small>值只保存在 Windows 凭据管理器，启动程序时作为环境变量交给它。</small>
    </fieldset>
  );
}
