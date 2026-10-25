import type { ExecutionApprovalView } from '@aura/contracts/generated';
import type { PackageResource } from './approval-facts';
import { packageDisplayName, usePackageSummaries } from '../tool-packages/package-labels';

/** The one request a package HTTP tool sends, with its query hidden like other web calls. */
function requestLine(action: Record<string, unknown>): string {
  const request = (action.request ?? {}) as { method?: unknown; url?: unknown };
  const method = typeof request.method === 'string' ? request.method : 'GET';
  if (typeof request.url !== 'string') return `${method}（地址未知）`;
  try {
    const url = new URL(request.url);
    return `${method} ${url.origin}${url.pathname}${url.search ? '?（查询参数已隐藏）' : ''}`;
  } catch {
    return `${method} 无效地址`;
  }
}

/**
 * What approving an installed package tool means. Program tools run as the user
 *, so the dialog says so plainly instead of implying
 * an isolation Aura does not provide.
 */
export function PackageApprovalFacts({
  view,
  resource,
}: {
  view: ExecutionApprovalView;
  resource: PackageResource;
}) {
  const packages = usePackageSummaries(true);
  const content = view.operation.content;
  const action = content.action as Record<string, unknown>;
  const network = content.capabilities.includes('ordinary_network');
  const session = view.approval.allowed_decisions.includes('allow_session');
  // A program's operation is the model's arguments; an HTTP operation wraps them
  // beside the exact request Core built.
  const argumentsShown = resource.runtime === 'http' ? action.arguments : action;
  return (
    <section aria-label="工具包调用">
      <p>
        工具包「{packageDisplayName(packages, resource.package_id)}」的工具 {content.action_kind} ·
        内容指纹 {resource.package_fingerprint.hex.slice(0, 12)}
      </p>
      {resource.runtime === 'program' ? (
        <>
          <p>
            这个工具会以当前 Windows 用户身份运行程序：权限不限于工作区，Aura
            不隔离它的文件写入。停止时 Aura 结束它的整个进程树。
          </p>
          <p>
            {network
              ? '工具包声明这个程序会联网：由程序自己联网，Aura 不限定它连接的地址，也不隔离私网；程序可以忽略代理设置。'
              : '工具包声明这个程序不联网；程序以你的身份运行，Aura 不在系统层面拦截它联网。'}
          </p>
          {view.mode === 'read_only' ? (
            <p>只读模式下这个工具每次运行都会询问，不能「本会话允许」。</p>
          ) : null}
        </>
      ) : (
        <>
          <p>
            Aura 代为发出一个 HTTPS 请求：{requestLine(action)}
            。不跟随重定向，返回结果按工具声明校验。
          </p>
          {content.credentials.length ? (
            <p>
              请求会附带你在工具包页为这个工具绑定的凭据；凭据只发往它允许的网站，模型看不到凭据。
            </p>
          ) : null}
        </>
      )}
      {session ? (
        <p>
          「本会话允许」只覆盖这个工具包这一版本内容的工具在本会话内的调用，不覆盖内置工具或其他工具包；工具包内容变化后须重新批准。撤销、到期或删除会话时失效，可在会话联网批准中撤销。
        </p>
      ) : null}
      <details>
        <summary>调用参数</summary>
        <pre>{JSON.stringify(argumentsShown ?? null, null, 2)}</pre>
      </details>
    </section>
  );
}
