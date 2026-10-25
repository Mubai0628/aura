import { userMessage } from '../../agent-session/user-message';
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

function localTime(value: string): string {
  const time = new Date(value);
  return Number.isNaN(time.getTime()) ? value : time.toLocaleString('zh-CN', { hour12: false });
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
              : { key, error: userMessage(result.error, '记忆批准读取失败，请重试。') },
          );
      })
      .catch(() => {
        if (live) setLoaded({ key, error: '读取记忆批准失败，请重试。' });
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
      if (!result.ok)
        setError(
          result.error.code === 'permission.grant_state_changed'
            ? '该批准状态已变化，已重新读取。'
            : userMessage(result.error, '撤销没有完成，请刷新后核对。'),
        );
      setReload((value) => value + 1);
    } catch {
      setError('撤销结果未确认，请重新读取。');
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
