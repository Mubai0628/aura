import type { ExecutionApprovalView } from '@aura/contracts/generated';
import { NetworkRequestDetails } from './NetworkRequestDetails';

export function NetworkApprovalFacts({ view }: { view: ExecutionApprovalView }) {
  const content = view.operation.content;
  const action = content.action as Record<string, unknown>;
  const missing = view.requested_capabilities ?? content.capabilities;
  const asksNetwork = missing.includes('ordinary_network');
  const labels: Record<string, string> = {
    ordinary_network: '联网',
    workspace_file_create: '新建工作区文件',
    workspace_file_write: '覆盖工作区文件',
    external_file_create: '新建外部文件',
    external_file_write: '覆盖外部文件',
  };
  let destination = '本次普通网络请求';
  if (typeof action.url === 'string') {
    try {
      const url = new URL(action.url);
      destination = `${url.origin}${url.pathname}${url.search ? '?（查询参数已隐藏）' : ''}`;
    } catch {
      destination = '无效地址';
    }
  }
  return (
    <section aria-label="联网批准范围">
      <p>联网（普通网络工具） · {destination}</p>
      {view.network_proxy_notice ? <p>{view.network_proxy_notice}</p> : null}
      <p>
        本次需要批准：{missing.map((cap) => labels[cap] ?? cap).join('、')}
        。已有有效批准覆盖的能力保持。
      </p>
      {asksNetwork ? (
        <p>
          「本会话允许」覆盖本会话普通网络工具的全部目标；重启和放宽、收紧模式后仍有效。只在撤销、到期或删除会话时失效，可在会话授权管理中撤销。
        </p>
      ) : (
        <p>联网已由当前模式或已有批准覆盖，本次只确认文件目标。</p>
      )}
      <p>
        联网批准不增加文件写入权限；下载位置仍按文件规则判断。凭据只会发往用户在工具凭据管理中指定的允许网站。
      </p>
      {content.action_kind === 'http.download' ? (
        <p>
          下载目标：{typeof action.path === 'string' ? action.path : '见文件目标'} ·{' '}
          {action.overwrite === true ? '允许覆盖已存在文件' : '仅创建新文件，不覆盖'}
        </p>
      ) : null}
      <NetworkRequestDetails kind={content.action_kind} action={action} />
    </section>
  );
}
