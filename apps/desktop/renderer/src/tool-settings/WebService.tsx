import { useState } from 'react';
import type { WebServiceSettingV1 } from '@aura/contracts/generated';
import { userMessage } from '../agent-session/user-message';
import type { ConfigurationOwner } from './useAgentConfiguration';
import './execution-strategy.css';

/**
 * Which service web.search and web.fetch use — DeepSeek's
 * through a DeepSeek connection's own key, or this computer's own search and fetch.
 */
export function WebService({ configuration }: { configuration: ConfigurationOwner }) {
  const [error, setError] = useState<string>();
  const view = configuration.view;
  if (view === undefined) return null;
  const connections = view.web_service_connections ?? [];
  const current = view.web_service ?? { kind: 'automatic' };
  const value = current.kind === 'deepseek' ? `deepseek:${current.endpoint_id}` : current.kind;
  const choose = async (next: string) => {
    const web_service: WebServiceSettingV1 = next.startsWith('deepseek:')
      ? { kind: 'deepseek', endpoint_id: next.slice('deepseek:'.length) }
      : next === 'local'
        ? { kind: 'local' }
        : { kind: 'automatic' };
    const outcome = await configuration.commit({ web_service });
    setError(outcome.error);
  };
  return (
    <div className="execution-strategy">
      <div className="execution-strategy-fields">
        <label>
          <span className="execution-strategy-label">服务</span>
          <select
            aria-label="网页搜索与抓取服务"
            disabled={configuration.busy}
            onChange={(event) => void choose(event.target.value)}
            value={value}
          >
            <option value="automatic">自动</option>
            {connections.map((connection) => (
              <option key={connection.endpoint_id} value={`deepseek:${connection.endpoint_id}`}>
                DeepSeek · {connection.display_name}
              </option>
            ))}
            {current.kind === 'deepseek' &&
            !connections.some((c) => c.endpoint_id === current.endpoint_id) ? (
              <option value={value}>DeepSeek · 已不可用的连接</option>
            ) : null}
            <option value="local">本机</option>
          </select>
        </label>
      </div>
      {error === undefined ? null : <p role="alert">{userMessage(error)}</p>}
    </div>
  );
}
