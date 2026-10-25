import { userMessage } from '../agent-session/user-message';
import { useEffect, useState } from 'react';
import type { FileSessionConsentView } from '@aura/contracts/generated';

const labels: Record<FileSessionConsentView['availability'], string> = {
  in_force: '生效中',
  revoked: '已撤销',
  expired: '已到期',
  target_changed: '原路径的对象已变化，该路径需重新批准',
  unavailable: '暂时无法核验目标',
  not_yet_valid: '尚未生效',
};

/** One finished load, tagged with the request it answers so a newer request shows as loading. */
interface LoadResult {
  key: string;
  grants?: FileSessionConsentView[];
  error?: string;
}

export function SessionFileGrants({ sessionId, open }: { sessionId: string; open: boolean }) {
  const [loaded, setLoaded] = useState<LoadResult>();
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState<string>();
  const [reload, setReload] = useState(0);
  const requestKey = `${sessionId}:${reload}`;
  useEffect(() => {
    if (!open) return;
    let current = true;
    void window.aura.v1
      .listFileSessionConsents({ session_id: sessionId, include_inactive: true })
      .then((result) => {
        if (!current) return;
        setLoaded(
          result.ok
            ? { key: requestKey, grants: result.value }
            : { key: requestKey, error: userMessage(result.error, '文件批准读取失败，请重试。') },
        );
      })
      .catch((reason: unknown) => {
        if (current)
          setLoaded({
            key: requestKey,
            error: userMessage(reason, '读取文件批准失败。'),
          });
      });
    return () => {
      current = false;
    };
  }, [sessionId, open, requestKey]);
  // A result for an older request is not shown: the newer request is still loading.
  const fresh = loaded?.key === requestKey ? loaded : undefined;
  const grants = fresh?.grants;
  const loadError = fresh?.error;

  async function revoke(grant: FileSessionConsentView) {
    if (pending) return;
    setPending(grant.grant_id);
    setError(undefined);
    try {
      const result = await window.aura.v1.revokeFileSessionConsent({
        session_id: sessionId,
        grant_id: grant.grant_id,
        expected_revision: grant.grant_revision,
      });
      if (!result.ok) setError(userMessage(result.error, '撤销没有完成，请刷新后核对。'));
      setReload((value) => value + 1);
    } catch (reason) {
      setError(userMessage(reason, '撤销文件批准失败。'));
    } finally {
      setPending(undefined);
    }
  }
  return (
    <section aria-label="会话文件批准">
      <h3>会话文件批准</h3>
      {error || loadError ? (
        <p role="alert">
          {error ?? loadError}{' '}
          <button type="button" onClick={() => setReload((value) => value + 1)}>
            重试
          </button>
        </p>
      ) : null}
      {!grants && !loadError ? <p role="status">正在读取文件批准…</p> : null}
      {grants?.length === 0 ? <p>当前没有会话文件批准。</p> : null}
      <ul className="compact-list">
        {grants?.map((grant) => (
          <li key={grant.grant_id}>
            <span>
              {grant.scope.targets
                .map(
                  (target) =>
                    `${target.canonical_path} · ${target.recursive ? '该目录及其下内容' : '仅该对象（不含其下内容）'}`,
                )
                .join('；')}
              {' · '}
              {grant.scope.capabilities.some((cap) => /_(write|create|delete|move)$/.test(cap))
                ? '写入（含覆盖、删除、重命名）'
                : '读取'}
              {' · '}
              {labels[grant.availability]}
            </span>
            {grant.availability !== 'revoked' ? (
              <button
                type="button"
                disabled={pending !== undefined}
                onClick={() => void revoke(grant)}
              >
                {pending === grant.grant_id ? '正在撤销…' : '撤销文件批准'}
              </button>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}
