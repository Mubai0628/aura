import { useState } from 'react';
import type { AgentToolSettingsView, ToolNetworkProxyV1 } from '@aura/contracts/generated';
import {
  PROBE_NOT_SENT,
  probeFailure,
  settingsNotRead,
  settingsNotSaved,
} from '../tool-settings/failure-words';
import {
  type ConfigurationOwner,
  useAgentConfiguration,
} from '../tool-settings/useAgentConfiguration';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { NetworkProxyRecovery } from './NetworkProxyRecovery';
import './network-proxy.css';

type Protocol = 'http' | 'socks5';
interface ProxyDraft {
  enabled: boolean;
  protocol: Protocol;
  host: string;
  port: string;
}

const EMPTY: ProxyDraft = { enabled: false, protocol: 'http', host: '', port: '' };

/**
 * Aura's own proxy. Model requests and tools take it while it is switched on; otherwise they
 * follow the system, whose proxy is shown here as well.
 */
export function NetworkProxySection() {
  const configuration = useAgentConfiguration();
  return (
    <section aria-labelledby="network-proxy-heading" className="network-proxy-section">
      <div className="section-heading-row">
        <div className="model-section-title">
          <h3 id="network-proxy-heading">网络代理</h3>
        </div>
      </div>
      <NetworkProxy configuration={configuration} />
    </section>
  );
}

function fromView(view: AgentToolSettingsView): ProxyDraft {
  const source = view.network_proxy_draft ?? view.network_proxy;
  if (source === undefined || source === null) return EMPTY;
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

/** A proxy as protocol and address. */
function proxyText(proxy: ToolNetworkProxyV1): string {
  const host = proxy.host.includes(':') ? `[${proxy.host}]` : proxy.host;
  return `${proxy.protocol === 'socks5' ? 'SOCKS5' : 'HTTP'} ${host}:${proxy.port}`;
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
  const system = view?.system_network_proxy;
  // Followed only while Aura's own proxy is switched off.
  const followed = value?.enabled === true ? undefined : (system?.proxy ?? undefined);
  const port = value === undefined ? undefined : portNumber(value.port);
  const hostError =
    value !== undefined && value.host !== '' && !loopbackHost(value.host)
      ? '只允许本机回环地址（127.0.0.0/8、::1 或 localhost）'
      : undefined;
  const addressReady = value !== undefined && loopbackHost(value.host) && port !== undefined;
  const canSave =
    value !== undefined &&
    (addressReady || (!value.enabled && value.host === '' && value.port === ''));
  const probeTarget =
    addressReady && port !== undefined
      ? { protocol: value.protocol, host: wireHost(value.host), port }
      : followed;

  const edit = (next: ProxyDraft) => {
    setDraft(next);
    setSaved(false);
    setError(undefined);
    setProbe(undefined);
  };

  const save = async () => {
    if (value === undefined || !canSave) return;
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
    if (probeTarget === undefined) return;
    setProbe(undefined);
    setError(undefined);
    try {
      const result = await window.aura.v1.testAgentNetworkProxy({
        protocol: probeTarget.protocol,
        host: probeTarget.host,
        port: probeTarget.port,
      });
      setProbe(result.ok ? result.value.message : probeFailure(result.error));
    } catch {
      setProbe(PROBE_NOT_SENT);
    }
  };

  if (loadError !== undefined && view === undefined) {
    return (
      <div className="network-proxy">
        <p role="alert">
          {settingsNotRead(loadError, '代理设置', '现在不能修改代理；请稍后点“重新读取网络代理”。')}
        </p>
        <button type="button" onClick={() => void reload()}>
          重新读取网络代理
        </button>
      </div>
    );
  }
  if (value === undefined) return <p className="network-proxy-pending">正在读取网络代理…</p>;

  return (
    <div className="network-proxy">
      <div className="network-proxy-fields">
        <label>
          <span className="network-proxy-label">启用</span>
          <ToggleSwitch
            checked={value.enabled}
            disabled={busy}
            label="启用网络代理"
            onCheckedChange={(enabled) => edit({ ...value, enabled })}
          />
        </label>
        <label>
          <span className="network-proxy-label">协议</span>
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
          <span className="network-proxy-label">地址</span>
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
          <span className="network-proxy-label">端口</span>
          <input
            aria-label="代理端口"
            className="network-proxy-port"
            inputMode="numeric"
            value={value.port}
            disabled={busy}
            placeholder="10809"
            onChange={(event) => edit({ ...value, port: event.target.value.trim() })}
          />
        </label>
        <div className="network-proxy-actions">
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
            disabled={probeTarget === undefined || busy}
            onClick={() => void testConnection()}
          >
            测试连接
          </button>
        </div>
      </div>
      {value.enabled ? null : followed !== undefined ? (
        <p className="network-proxy-system">
          <span>系统代理</span>
          <output>{proxyText(followed)}</output>
        </p>
      ) : system?.issue ? (
        <p className="network-proxy-system" role="status">
          {system.issue}
        </p>
      ) : null}
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
      {error ? (
        <p role="alert">
          {settingsNotSaved(error, '代理设置没有保存', '仍按原来的设置联网', '请检查后再保存')}
        </p>
      ) : null}
      {probe ? <p role="status">{probe}</p> : null}
      {saved ? <p role="status">网络代理已保存。</p> : null}
    </div>
  );
}
