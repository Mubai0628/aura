import { userMessage } from '../../agent-session/user-message';
import { useEffect, useState } from 'react';
import type { NetworkSessionConsentView } from '@aura/contracts/generated';
import { packageDisplayName, usePackageSummaries } from '../../tool-packages/package-labels';

const labels: Record<NetworkSessionConsentView['availability'], string> = {
  in_force: '生效中',
  revoked: '已撤销',
  expired: '已到期',
  not_yet_valid: '尚未生效',
};
interface Loaded {
  key: string;
  grants?: NetworkSessionConsentView[];
  error?: string;
}

export function SessionNetworkGrants({ sessionId, open }: { sessionId: string; open: boolean }) {
  const [loaded, setLoaded] = useState<Loaded>();
  const [reload, setReload] = useState(0);
  const [pending, setPending] = useState<string>();
  const [message, setMessage] = useState<string>();
  const key = `${sessionId}:${reload}`;
  useEffect(() => {
    if (!open) return;
    let live = true;
    void window.aura.v1
      .listNetworkSessionConsents({ session_id: sessionId, include_inactive: true })
      .then((result) => {
        if (live)
          setLoaded(
            result.ok
              ? { key, grants: result.value }
              : { key, error: userMessage(result.error, '网络批准读取失败，请重试。') },
          );
      })
      .catch(() => {
        if (live) setLoaded({ key, error: '读取联网批准失败，请重试。' });
      });
    return () => {
      live = false;
    };
  }, [sessionId, open, key]);
  const current = loaded?.key === key ? loaded : undefined;
  const packages = usePackageSummaries(
    open && (loaded?.grants?.some((grant) => grant.scope.caller === 'package_tools') ?? false),
  );
  async function revoke(grant: NetworkSessionConsentView) {
    if (pending) return;
    setPending(grant.grant_id);
    setMessage('正在撤销并停止依赖此批准的请求…');
    try {
      const result = await window.aura.v1.revokeNetworkSessionConsent({
        session_id: sessionId,
        grant_id: grant.grant_id,
        expected_revision: grant.grant_revision,
      });
      setMessage(
        result.ok
          ? '已撤销；依赖此批准且需要停止的请求已确认停止。'
          : userMessage(result.error, '撤销没有完成，请刷新后核对。'),
      );
      setReload((value) => value + 1);
    } catch {
      setMessage('撤销结果未确认，请重新读取；不要假定请求已经停止。');
    } finally {
      setPending(undefined);
    }
  }
  return (
    <section aria-label="会话联网批准">
      <h3>会话联网批准</h3>
      {message ? <p role="status">{message}</p> : null}
      {current?.error ? (
        <p role="alert">
          {current.error}{' '}
          <button type="button" onClick={() => setReload((value) => value + 1)}>
            重试
          </button>
        </p>
      ) : null}
      {!current ? <p role="status">正在读取联网批准…</p> : null}
      {current?.grants?.length === 0 ? <p>当前没有会话联网批准。</p> : null}
      <ul className="compact-list">
        {current?.grants?.map((grant) => (
          <li key={grant.grant_id}>
            <span>
              {grant.scope.caller === 'git'
                ? 'Git 普通推送批准'
                : grant.scope.caller === 'built_in_shells'
                  ? 'Shell 联网批准'
                  : grant.scope.caller === 'package_tools'
                    ? '工具包批准'
                    : '联网（普通网络工具）'}{' '}
              ·{' '}
              {typeof grant.scope.destinations === 'object' && 'git' in grant.scope.destinations
                ? `${grant.scope.destinations.git.target.remote_url} → ${grant.scope.destinations.git.target.remote_branch}（仓库 ${grant.scope.destinations.git.target.repository.hex.slice(0, 12)}）`
                : typeof grant.scope.destinations === 'object' &&
                    'package' in grant.scope.destinations
                  ? `工具包「${packageDisplayName(packages, grant.scope.destinations.package.package_id)}」（内容指纹 ${grant.scope.destinations.package.package_fingerprint.hex.slice(0, 12)}）`
                  : '全部目标'}{' '}
              · {labels[grant.availability]} ·{' '}
              {grant.expires_at ? `到期：${grant.expires_at}` : '本会话有效'}
            </span>
            {grant.availability === 'in_force' ? (
              <button
                type="button"
                disabled={pending !== undefined}
                onClick={() => void revoke(grant)}
              >
                {pending === grant.grant_id ? '正在撤销…' : '撤销联网批准'}
              </button>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}
