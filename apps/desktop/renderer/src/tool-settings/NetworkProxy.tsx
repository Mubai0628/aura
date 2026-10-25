import { useState } from 'react';
import type { AgentToolSettingsView } from '@aura/contracts/generated';
import { AppIcon } from '../app-icons';
import { userMessage } from '../agent-session/user-message';
import { ExecutionStrategy } from './ExecutionStrategy';
import type { ConfigurationOwner } from './useAgentConfiguration';
import { NetworkProxyRecovery } from './NetworkProxyRecovery';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { WebService } from './WebService';
import './execution-strategy.css';

type Protocol = 'http' | 'socks5';
interface ProxyDraft {
  enabled: boolean;
  protocol: Protocol;
  host: string;
  port: string;
}

const EMPTY: ProxyDraft = { enabled: false, protocol: 'http', host: '', port: '' };

export function ToolPolicySections({
  configuration,
  strategyReady,
}: {
  configuration: ConfigurationOwner;
  strategyReady: boolean;
}) {
  return (
    <>
      <section className="settings-surface tool-policy-surface">
        <header className="tool-section-heading">
          <div>
            <h3>执行策略</h3>
          </div>
        </header>
        {strategyReady ? (
          <ExecutionStrategy configuration={configuration} />
        ) : (
          <div className="tool-projection-placeholder" role="status">
            <AppIcon name="refresh" />
            <span>等待 Tool Settings 快照</span>
          </div>
        )}
      </section>
      <section className="settings-surface tool-policy-surface" aria-label="网络代理">
        <header className="tool-section-heading">
          <div>
            <h3>网络代理</h3>
          </div>
        </header>
        <NetworkProxy configuration={configuration} />
      </section>
      <section className="settings-surface tool-policy-surface" aria-label="网页搜索与抓取">
        <header className="tool-section-heading">
          <div>
            <h3>网页搜索与抓取</h3>
          </div>
        </header>
        <WebService configuration={configuration} />
      </section>
    </>
  );
}

function fromView(view: AgentToolSettingsView): ProxyDraft {
  const draft = view.network_proxy_draft;
  const saved = view.network_proxy;
  const source =
    draft ??
    (saved === undefined || saved === null
      ? undefined
      : {
          enabled: saved.enabled,
          protocol: saved.protocol,
          host: saved.host,
          port: saved.port,
        });
  if (source === undefined) return EMPTY;
  return {
    enabled: source.enabled,
    protocol: source.protocol === 'socks5' ? 'socks5' : 'http',
    host: source.host,
    port: source.port === 0 ? '' : String(source.port),
  };
}

function loopbackHost(host: string): boolean {
  if (host.toLowerCase() === 'localhost' || host === '::1') return true;
  const match = /^127\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host);
  return match !== null && match.slice(1).every((part) => Number(part) <= 255);
}

/** The contract spells the alias in lower case; the form accepts any case. */
function wireHost(host: string): string {
  return host.toLowerCase() === 'localhost' ? 'localhost' : host;
}

function portNumber(port: string): number | undefined {
  if (!/^\d+$/.test(port)) return undefined;
  const value = Number(port);
  return value >= 1 && value <= 65535 ? value : undefined;
}

function NetworkProxy({ configuration }: { configuration: ConfigurationOwner }) {
  const { view, loadError, busy, reload, commit } = configuration;
  const [draft, setDraft] = useState<ProxyDraft>();
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string>();
  const [probe, setProbe] = useState<string>();
  const value = draft ?? (view === undefined ? undefined : fromView(view));
  const port = value === undefined ? undefined : portNumber(value.port);
  const hostError =
    value !== undefined && value.host !== '' && !loopbackHost(value.host)
      ? '只允许本机回环地址（127.0.0.0/8、::1 或 localhost）'
      : undefined;
  const addressReady = value !== undefined && loopbackHost(value.host) && port !== undefined;
  const canSave =
    value !== undefined &&
    (addressReady || (!value.enabled && value.host === '' && value.port === ''));

  const edit = (next: ProxyDraft) => {
    setDraft(next);
    setSaved(false);
    setError(undefined);
    setProbe(undefined);
  };

  const save = async () => {
    if (value === undefined || !canSave || (port === undefined && value.host !== '')) return;
    setSaved(false);
    setError(undefined);
    const network_proxy =
      value.host === '' && !value.enabled
        ? null
        : {
            enabled: value.enabled,
            protocol: value.protocol,
            host: wireHost(value.host),
            port: port ?? 0,
          };
    const outcome = await commit({ network_proxy });
    if (outcome.error !== undefined) setError(outcome.error);
    else {
      setSaved(true);
      setDraft(undefined);
    }
  };

  const testConnection = async () => {
    if (value === undefined || !addressReady || port === undefined) return;
    setProbe(undefined);
    setError(undefined);
    try {
      const result = await window.aura.v1.testAgentNetworkProxy({
        protocol: value.protocol,
        host: wireHost(value.host),
        port,
      });
      setProbe(result.ok ? result.value.message : userMessage(result.error.message));
    } catch {
      setProbe('无法连接代理端口（拒绝连接或连接失败）');
    }
  };

  if (loadError !== undefined && view === undefined) {
    return (
      <div className="execution-strategy">
        <p role="alert">{userMessage(loadError)}</p>
        <button type="button" onClick={() => void reload()}>
          重新读取网络代理
        </button>
      </div>
    );
  }
  if (value === undefined) return <p className="execution-strategy-pending">正在读取网络代理…</p>;

  return (
    <div className="execution-strategy">
      <div className="execution-strategy-fields">
        <label>
          <span className="execution-strategy-label">启用</span>
          <ToggleSwitch
            checked={value.enabled}
            disabled={busy}
            label="启用网络代理"
            onCheckedChange={(enabled) => edit({ ...value, enabled })}
          />
        </label>
        <label>
          <span className="execution-strategy-label">协议</span>
          <select
            aria-label="代理协议"
            value={value.protocol}
            disabled={busy}
            onChange={(event) => edit({ ...value, protocol: event.target.value as Protocol })}
          >
            <option value="http">HTTP</option>
            <option value="socks5">SOCKS5</option>
          </select>
        </label>
        <label data-invalid={hostError ? 'true' : undefined}>
          <span className="execution-strategy-label">地址</span>
          <input
            aria-label="代理地址"
            value={value.host}
            disabled={busy}
            placeholder="127.0.0.1"
            aria-invalid={hostError ? true : undefined}
            onChange={(event) => edit({ ...value, host: event.target.value.trim() })}
          />
        </label>
        <label>
          <span className="execution-strategy-label">端口</span>
          <input
            aria-label="代理端口"
            inputMode="numeric"
            value={value.port}
            disabled={busy}
            placeholder="10809"
            onChange={(event) => edit({ ...value, port: event.target.value.trim() })}
          />
        </label>
      </div>
      <div className="execution-strategy-actions">
        <button
          type="button"
          className="button is-primary"
          disabled={!canSave || busy}
          onClick={() => void save()}
        >
          {busy ? '保存中…' : '保存网络代理'}
        </button>
        <button
          type="button"
          className="button"
          disabled={!addressReady || busy}
          onClick={() => void testConnection()}
        >
          测试连接
        </button>
      </div>
      <NetworkProxyRecovery
        configuration={configuration}
        onRestored={() => {
          setDraft(undefined);
          setSaved(false);
          setError(undefined);
          setProbe(undefined);
        }}
      />
      {view?.network_proxy_diagnostic && draft === undefined ? (
        <p role="alert">{view.network_proxy_diagnostic}</p>
      ) : null}
      {hostError ? <p role="alert">{hostError}</p> : null}
      {error ? <p role="alert">{userMessage(error)}</p> : null}
      {probe ? <p role="status">{probe}</p> : null}
      {saved ? <p role="status">网络代理已保存。</p> : null}
    </div>
  );
}
