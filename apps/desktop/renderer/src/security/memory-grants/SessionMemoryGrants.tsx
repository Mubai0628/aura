import { localTime } from '../../../../shared/local-time';
import { grantsNotRead, revokeFailure } from '../grant-words';
import { useEffect, useState } from 'react';
import type { MemorySessionConsentView } from '@aura/contracts/generated';

interface Loaded {
  key: string;
  grants?: MemorySessionConsentView[];
  error?: string;
}

function vaultLabel(grant: MemorySessionConsentView): string {
  return grant.scope.scope.kind === 'global' ? '全局记忆库' : '本项目记忆库';
}

/** A ReadOnly session's "本会话允许" answers for memory writes, one per Vault. */
export function SessionMemoryGrants({ sessionId, open }: { sessionId: string; open: boolean }) {
  const [loaded, setLoaded] = useState<Loaded>();
  const [reload, setReload] = useState(0);
  const [pending, setPending] = useState<string>();
  const [error, setError] = useState<string>();
  const key = `${sessionId}:${reload}`;
  useEffect(() => {
    if (!open) return;
    let live = true;
    void window.aura.v1
      .listMemorySessionConsents({ session_id: sessionId, include_inactive: true })
      .then((result) => {
        if (live)
          setLoaded(
            result.ok
              ? { key, grants: result.value }
              : { key, error: grantsNotRead('记忆批准', result.error) },
          );
      })
      .catch((reason: unknown) => {
        if (live) setLoaded({ key, error: grantsNotRead('记忆批准', reason) });
      });
    return () => {
      live = false;
    };
  }, [sessionId, open, key]);
  // An answer to an older request is not shown: the newer request is still loading.
  const current = loaded?.key === key ? loaded : undefined;
  const grants = current?.grants;
  const loadError = current?.error;
  async function revoke(grant: MemorySessionConsentView) {
    if (pending) return;
    setPending(grant.grant_id);
    setError(undefined);
    try {
      const result = await window.aura.v1.revokeMemorySessionConsent({
        session_id: sessionId,
        grant_id: grant.grant_id,
        expected_revision: grant.grant_revision,
      });
      if (!result.ok) setError(revokeFailure(result.error, '记忆批准', '它仍然有效'));
      setReload((value) => value + 1);
    } catch (reason) {
      setError(revokeFailure(reason, '记忆批准', '它仍然有效'));
      setReload((value) => value + 1);
    } finally {
      setPending(undefined);
    }
  }
  return (
    <section aria-label="会话记忆批准">
      <h3>会话记忆批准</h3>
      {error || loadError ? (
        <p role="alert">
          {error ?? loadError}{' '}
          <button type="button" onClick={() => setReload((value) => value + 1)}>
            重试
          </button>
        </p>
      ) : null}
      {!grants && !loadError ? <p role="status">正在读取记忆批准…</p> : null}
      {grants?.length === 0 ? <p>当前会话没有记忆批准。</p> : null}
      <ul className="compact-list">
        {grants?.map((grant) => (
          <li key={grant.grant_id}>
            <span>
              {vaultLabel(grant)} · 写入记忆 · {grant.in_force ? '生效中' : '已撤销'} · 批准于{' '}
              {localTime(grant.issued_at)}
            </span>
            {grant.in_force ? (
              <button
                type="button"
                disabled={pending !== undefined}
                onClick={() => void revoke(grant)}
              >
                {pending === grant.grant_id ? '正在撤销…' : '撤销记忆批准'}
              </button>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}
