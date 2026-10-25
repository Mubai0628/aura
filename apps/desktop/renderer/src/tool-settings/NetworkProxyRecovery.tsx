import { useState } from 'react';
import type { AgentNetworkProxyRecoveryPreviewV1 } from '@aura/contracts/generated';
import { userMessage } from '../agent-session/user-message';
import type { ConfigurationOwner } from './useAgentConfiguration';

export function NetworkProxyRecovery({
  configuration,
  onRestored,
}: {
  configuration: ConfigurationOwner;
  onRestored: () => void;
}) {
  const [preview, setPreview] = useState<AgentNetworkProxyRecoveryPreviewV1>();
  const [message, setMessage] = useState<string>();
  const [error, setError] = useState<string>();
  const available = configuration.view?.network_proxy_recovery;

  const apply = async () => {
    if (preview === undefined) return;
    setError(undefined);
    const result = await configuration.commit(
      { network_proxy: preview.proxy ?? null },
      preview.expected_revision,
    );
    if (result.error !== undefined) setError(result.error);
    else if (result.view !== undefined) {
      onRestored();
      setPreview(undefined);
      setMessage('网络代理已恢复。');
    }
  };

  return (
    <div>
      <button
        type="button"
        disabled={configuration.busy || available == null}
        onClick={() => {
          if (available != null) setPreview(available);
          setMessage(undefined);
          setError(undefined);
        }}
      >
        恢复上一版有效代理
      </button>
      {available == null ? <p>没有上一版有效代理可恢复。</p> : null}
      {preview !== undefined ? (
        <div role="region" aria-label="代理恢复预览">
          <p>只恢复网络代理，其他工具设置保持当前值。来源修订：{preview.source_revision}。</p>
          <p>
            {preview.proxy == null
              ? '关闭网络代理，恢复为直连。'
              : `${preview.proxy.enabled ? '启用' : '停用'} ${preview.proxy.protocol === 'http' ? 'HTTP' : 'SOCKS5'} 代理 ${preview.proxy.host}:${preview.proxy.port}`}
          </p>
          <button type="button" disabled={configuration.busy} onClick={() => void apply()}>
            应用代理恢复
          </button>
          <button type="button" disabled={configuration.busy} onClick={() => setPreview(undefined)}>
            取消代理恢复
          </button>
          <button
            type="button"
            disabled={configuration.busy}
            onClick={() => {
              setPreview(undefined);
              setError(undefined);
              void configuration.reload();
            }}
          >
            重新读取网络代理
          </button>
        </div>
      ) : null}
      {error !== undefined ? <p role="alert">{userMessage(error)}</p> : null}
      {message !== undefined ? <p role="status">{message}</p> : null}
    </div>
  );
}
