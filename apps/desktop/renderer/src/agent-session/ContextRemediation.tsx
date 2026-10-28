import { userMessage } from './user-message';
import type {
  AgentContextRemediation,
  AgentContextWindowCorrection,
} from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { isUnconfirmedCommandOutcome } from '../../../shared/ipc-request-failure';
import { ContextWindowCorrectionPanel } from '../ContextWindowCorrectionPanel';

const uncertain = new Set<string>();
const actions: Record<string, string> = {
  reduce_user_managed_context: '减少附件、自定义指令等可调整内容，或切换到上下文窗口更大的模型',
  compact_sealed_history: '压缩可处理的历史内容',
};

export function ContextRemediation({
  sessionId,
  runId,
  onChanged,
  onOpenModelSettings,
  onStop,
}: {
  sessionId: string;
  runId: string;
  onChanged: () => void;
  onOpenModelSettings: () => void;
  onStop: () => void;
}) {
  const [record, setRecord] = useState<AgentContextRemediation>();
  const [provider, setProvider] = useState<AgentContextWindowCorrection>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [unknown, setUnknown] = useState(() => uncertain.has(runId));
  const [refresh, setRefresh] = useState(0);
  const generation = useRef(0);
  const version = useRef(0);
  const sending = useRef(false);
  useEffect(() => {
    let alive = true;
    const current = ++generation.current;
    const readVersion = version.current;
    void window.aura.v1
      .readAgentContextRemediation({ session_id: sessionId, agent_run_id: runId })
      .then((result) => {
        if (!alive || current !== generation.current || readVersion !== version.current) return;
        if (!result.ok) {
          setError(result.error.message);
          return;
        }
        if (result.value.session_id !== sessionId || result.value.agent_run_id !== runId) return;
        setRecord(result.value.remediation ?? undefined);
        setProvider(result.value.provider_correction ?? undefined);
        setError(undefined);
      })
      .catch(() => {
        if (alive) setError('无法读取上下文诊断，请检查 Core 连接。');
      });
    return () => {
      alive = false;
      generation.current += 1;
    };
  }, [sessionId, runId, refresh]);
  const recheck = async () => {
    if (!record || record.state !== 'awaiting' || sending.current || unknown) return;
    sending.current = true;
    version.current += 1;
    const current = generation.current;
    setBusy(true);
    const unconfirmed = () => {
      uncertain.add(runId);
      setUnknown(true);
    };
    try {
      const result = await window.aura.v1.recheckAgentContext({
        session_id: sessionId,
        agent_run_id: runId,
        source_model_step_id: record.model_step_id,
        expected_revision: record.revision,
      });
      if (current !== generation.current) return;
      if (result.ok) {
        if (result.value.session_id !== sessionId || result.value.agent_run_id !== runId) {
          setError('返回状态与当前任务不一致，请刷新核对。');
          unconfirmed();
        } else {
          setRecord(result.value.remediation ?? undefined);
          setError(undefined);
        }
      } else {
        setError(result.error.message);
        if (isUnconfirmedCommandOutcome(result.error)) unconfirmed();
      }
    } catch {
      if (current === generation.current) {
        setError('重新检查的结果未确认，请刷新任务状态。');
        unconfirmed();
      }
    } finally {
      sending.current = false;
      if (current === generation.current) {
        setBusy(false);
        onChanged();
      }
    }
  };
  if (provider?.state === 'awaiting')
    return (
      <ContextWindowCorrectionPanel
        key={`${runId}:${provider.revision}`}
        sessionId={sessionId}
        record={provider}
        onChanged={() => {
          onChanged();
          setRefresh((v) => v + 1);
        }}
        onOpenModelSettings={onOpenModelSettings}
        onStop={onStop}
      />
    );
  return (
    <section className="agent-context-remediation" aria-label="上下文需要处理">
      <strong>上下文需要处理</strong>
      <p>任务已暂停模型调用。调整配置或减少可由你修改的内容后，再重新检查。</p>
      {record ? (
        <>
          {record.plan.overflow.error_code === 'context.provider_rebuild_no_progress' ? (
            <p>模型服务商拒绝了这次请求：超出它的上下文窗口，压缩后也没能缩小。</p>
          ) : null}
          <p>
            预计输入 {record.budget.estimated_input_tokens.toLocaleString('zh-CN')} Token / 可用{' '}
            {record.budget.request_input_budget_tokens.toLocaleString('zh-CN')} Token
          </p>
          <p>
            不可压缩内容 {record.plan.overflow.non_compressible_tokens.toLocaleString('zh-CN')}{' '}
            Token · 可由你减少 {record.plan.overflow.user_reducible_tokens.toLocaleString('zh-CN')}{' '}
            Token
          </p>
          <ul>
            {record.plan.actions
              .filter((action) => actions[action])
              .map((action) => (
                <li key={action}>{actions[action]}</li>
              ))}
          </ul>
        </>
      ) : (
        <p role="status">正在读取诊断…</p>
      )}
      {error ? <p role="alert">{userMessage(error)}</p> : null}
      {unknown ? <p role="alert">上次操作结果未确认。请刷新并核对任务状态后再试。</p> : null}
      <div className="agent-actions">
        <button type="button" onClick={onOpenModelSettings}>
          模型设置
        </button>
        <button
          type="button"
          disabled={busy || unknown || record?.state !== 'awaiting'}
          onClick={() => void recheck()}
        >
          {busy ? '检查中…' : '重新检查'}
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => {
            onChanged();
            setRefresh((v) => v + 1);
          }}
        >
          刷新状态
        </button>
        {unknown ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              uncertain.delete(runId);
              setUnknown(false);
            }}
          >
            已核对状态
          </button>
        ) : null}
        <button type="button" onClick={onStop}>
          停止任务
        </button>
      </div>
    </section>
  );
}
