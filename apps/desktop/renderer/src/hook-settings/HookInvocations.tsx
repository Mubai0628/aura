import { useEffect, useState } from 'react';
import type { HookInvocationView } from '@aura/contracts/generated';
import { hookEffectLabel, hookError, hookEventLabel, hookOutcomeLabel } from './hook-labels';

const time = (at: string) => {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? at : date.toLocaleString('zh-CN', { hour12: false });
};

/** One Hook's recent calls, newest first, read when the user opens them. */
export function HookInvocations({ path, name }: { path: string; name: string }) {
  const [calls, setCalls] = useState<HookInvocationView[]>();
  const [error, setError] = useState<string>();
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let live = true;
    void window.aura.v1.listHookInvocations({ path, turn_task_id: null, limit: 20 }).then(
      (result) => {
        if (!live) return;
        if (result.ok) {
          setCalls(result.value.invocations);
          setError(undefined);
        } else setError(hookError(result.error.message, '调用记录读取失败，请重试。'));
      },
      (reason: unknown) => {
        if (live) setError(hookError(reason, '调用记录读取失败，请重试。'));
      },
    );
    return () => {
      live = false;
    };
  }, [path, reload]);
  return (
    <section className="hook-invocations" aria-label={`${name} 的最近调用`}>
      <div className="hook-invocations-heading">
        <h4>最近调用</h4>
        <button
          type="button"
          className="button is-ghost is-small"
          onClick={() => setReload((value) => value + 1)}
        >
          刷新
        </button>
      </div>
      {error ? (
        <p role="alert">{error}</p>
      ) : !calls ? (
        <p role="status">正在读取调用记录…</p>
      ) : !calls.length ? (
        <p>还没有调用记录。</p>
      ) : (
        <ol>
          {calls.map((call) => {
            const effect = hookEffectLabel(call.effect);
            return (
              <li key={call.invocation_id} data-outcome={call.outcome}>
                <div className="hook-invocation-line">
                  <span>{time(call.started_at)}</span>
                  <span>{hookEventLabel(call.event)}</span>
                  <strong>{hookOutcomeLabel(call.outcome, call.failure)}</strong>
                  <span>{call.duration_ms} 毫秒</span>
                  <span>
                    版本 {call.hook_version} · 指纹 <code>{call.fingerprint.slice(0, 12)}</code>
                  </span>
                </div>
                {effect ? <p>{effect}</p> : null}
                {call.reason ? <p>原因：{call.reason}</p> : null}
                {call.summary ? <p>变换：{call.summary}</p> : null}
                {call.stderr ? (
                  <details>
                    <summary>标准错误输出</summary>
                    <pre>{call.stderr}</pre>
                  </details>
                ) : null}
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
