import type { McpServerView } from '@aura/contracts/generated';

/**
 * A server's OAuth sign-in in its row: the state, the last problem, the link to copy
 * when no browser opened, and signing in or out. Tokens never reach this page.
 */
export function McpSignInRow({
  server,
  link,
  locked,
  onSignIn,
  onSignOut,
}: {
  server: McpServerView;
  link: string | undefined;
  locked: boolean;
  onSignIn: () => void;
  onSignOut: () => void;
}) {
  const sign = server.sign_in;
  if (!sign) return null;
  const status = sign.signed_in
    ? '已登录'
    : sign.pending
      ? `正在等待浏览器里的登录${sign.authorization_server ? `（${sign.authorization_server}）` : ''}…`
      : server.state === 'sign_in_required'
        ? '需要在浏览器中登录'
        : undefined;
  return (
    <div className="mcp-sign-in" role="group" aria-label={`${server.config.display_name} 的登录`}>
      {status ? <p>{status}</p> : null}
      {sign.last_error && !sign.signed_in ? (
        <p role="alert">上次登录没有完成：{sign.last_error}</p>
      ) : null}
      {link && !sign.signed_in ? (
        <p>
          如果浏览器没有打开，请把这个地址复制到浏览器：<code>{link}</code>
        </p>
      ) : null}
      <div className="settings-card-actions">
        {server.state === 'sign_in_required' ? (
          <button
            type="button"
            className="button is-primary is-small"
            disabled={locked}
            onClick={onSignIn}
          >
            在浏览器中登录
          </button>
        ) : null}
        {sign.signed_in ? (
          <button type="button" className="button is-small" disabled={locked} onClick={onSignOut}>
            退出登录
          </button>
        ) : null}
      </div>
    </div>
  );
}
