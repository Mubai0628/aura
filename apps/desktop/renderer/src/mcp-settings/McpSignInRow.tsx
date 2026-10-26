import type { McpServerView } from '@aura/contracts/generated';
import { useState } from 'react';
import { copyText } from '../clipboard';

/**
 * A server's OAuth sign-in in its row: the state, the last problem, the link to copy
 * when no browser opened, and signing in or out. Tokens never reach this page. The link is a long
 * unbroken address, so it gets its own block that may break anywhere instead of widening the page.
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
  const [copied, setCopied] = useState<{ error: string | undefined }>();
  const sign = server.sign_in;
  if (!sign) return null;
  const copy = async (address: string) =>
    setCopied({ error: await copyText(address, '链接没有复制成功，请选中地址后手动复制。') });
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
        <div className="mcp-sign-in-address">
          <p>如果浏览器没有打开，请把这个地址复制到浏览器：</p>
          <code className="mcp-sign-in-link">{link}</code>
          <div className="settings-card-actions">
            <button type="button" className="button is-small" onClick={() => void copy(link)}>
              复制链接
            </button>
            {copied === undefined ? null : copied.error === undefined ? (
              <span role="status">已复制</span>
            ) : (
              <span role="alert">{copied.error}</span>
            )}
          </div>
        </div>
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
