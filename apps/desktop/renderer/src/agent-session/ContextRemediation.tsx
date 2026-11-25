import type {
  AgentContextRemediation,
  AgentContextWindowCorrection,
} from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { isUnconfirmedCommandOutcome } from '../../../shared/ipc-request-failure';
import { localTime } from '../../../shared/local-time';
import { ContextWindowCorrectionPanel } from '../ContextWindowCorrectionPanel';
import { failedAction } from './failure-text';

/** What a diagnosis that was not read says; the task stays paused. */
const DIAGNOSIS_UNREAD = '上下文诊断没有读到';
const DIAGNOSIS_NEXT = '任务仍暂停着；请稍后重新打开这个会话。';

const uncertain = new Set<string>();
const actions: Record<string, string> = {
  reduce_user_managed_context: '减少附件、自定义指令等可调整内容',
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
  const [readState, setReadState] = useState<
    'loading' | 'refreshing' | 'ready' | 'refreshed' | 'failed'
  >('loading');
  const refreshPending = useRef(false);
  const generation = useRef(0);
  const version = useRef(0);
  const sending = useRef(false);
  useEffect(() => {
    let alive = true;
    const current = ++generation.current;
    const readVersion = version.current;
    const manual = refreshPending.current;
    const next = manual ? '可以再点“刷新状态”读取最新诊断。' : DIAGNOSIS_NEXT;
    const isCurrent = () =>
      alive && current === generation.current && readVersion === version.current;
    const failed = (message: string) => {
      setError(message);
      setReadState('failed');
      refreshPending.current = false;
    };
    void window.aura.v1
      .readAgentContextRemediation({ session_id: sessionId, agent_run_id: runId })
      .then((result) => {
        if (!isCurrent()) return;
        if (!result.ok) {
          failed(failedAction(result.error, DIAGNOSIS_UNREAD, next));
          return;
        }
        if (result.value.session_id !== sessionId || result.value.agent_run_id !== runId) {
          failed('读取到的诊断属于另一个任务，请重新打开当前会话。');
          return;
        }
        setRecord(result.value.remediation ?? undefined);
        setProvider(result.value.provider_correction ?? undefined);
        setError(undefined);
        setReadState(manual ? 'refreshed' : 'ready');
        refreshPending.current = false;
      })
      .catch((reason: unknown) => {
        if (isCurrent()) failed(failedAction(reason, DIAGNOSIS_UNREAD, next));
      });
    return () => {
      alive = false;
      generation.current += 1;
    };
  }, [sessionId, runId, refresh]);
  const refreshStatus = () => {
    if (refreshPending.current || sending.current) return;
    refreshPending.current = true;
    setReadState('refreshing');
    setError(undefined);
    setRefresh((v) => v + 1);
    onChanged();
  };
  const recheck = async () => {
    if (
      !record ||
      record.state !== 'awaiting' ||
      sending.current ||
      refreshPending.current ||
      unknown
    )
      return;
    sending.current = true;
    version.current += 1;
    const current = generation.current;
    setBusy(true);
    setReadState('ready');
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
          setError('重新检查返回的是另一个任务的结果，没有采用；请重新打开这个会话核对。');
          unconfirmed();
        } else {
          setRecord(result.value.remediation ?? undefined);
          setError(undefined);
        }
      } else {
        setError(
          failedAction(result.error, '重新检查没有完成', '任务仍暂停着；可以再点“重新检查”。'),
        );
        if (isUnconfirmedCommandOutcome(result.error)) unconfirmed();
      }
    } catch {
      if (current === generation.current) {
        setError(
          '重新检查的结果没有确认（与 Aura Core 的连接出了问题），没有自动重发；请重新打开这个会话核对。',
        );
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
  const refreshing = readState === 'refreshing';
  const feedback = error ? (
    <p role="alert">{error}</p>
  ) : refreshing ? (
    <p role="status">正在读取最新状态…</p>
  ) : readState === 'refreshed' ? (
    <p role="status">
      {provider?.state === 'awaiting' || record?.state === 'awaiting'
        ? '已刷新，任务仍在等待处理。'
        : record?.state === 'rechecking'
          ? '已刷新，正在重新检查上下文。'
          : '已刷新，当前没有待处理的上下文问题。'}
    </p>
  ) : null;
  if (provider?.state === 'awaiting')
    return (
      <ContextWindowCorrectionPanel
        key={`${runId}:${provider.revision}`}
        sessionId={sessionId}
        record={provider}
        refreshStatus={refreshStatus}
        refreshing={refreshing}
        readFeedback={feedback}
        onChanged={() => {
          onChanged();
          setRefresh((v) => v + 1);
        }}
        onOpenModelSettings={onOpenModelSettings}
        onStop={onStop}
      />
    );
  const reducible = (record?.plan.overflow.user_reducible_tokens ?? 0) > 0;
  const reason = record?.plan.overflow.error_code;
  const summaryInvalidated =
    reason === 'context.compaction_window_insufficient_after_summary_invalidated';
  const summaryTooLarge = summaryInvalidated || reason === 'context.compaction_window_insufficient';
  const suggestions = record?.plan.actions.filter(
    (action) =>
      actions[action] &&
      (reducible || action !== 'reduce_user_managed_context') &&
      (!summaryTooLarge || action !== 'compact_sealed_history'),
  );
  return (
    <section className="agent-context-remediation" aria-label="上下文需要处理">
      <header>
        <strong>上下文需要处理</strong>
        {record?.created_at ? (
          <time dateTime={record.created_at}>预检 {localTime(record.created_at)}</time>
        ) : null}
      </header>
      <div
        className="context-remediation-details"
        role="region"
        aria-label="上下文诊断详情"
        tabIndex={0}
      >
        {record?.state === 'awaiting' ? (
          <>
            <p>任务已暂停模型调用。完成调整后，点“重新检查”继续。</p>
            {record.plan.overflow.error_code === 'context.provider_rebuild_no_progress' ? (
              <p>模型服务商拒绝了这次请求，因为输入超出了它的上下文窗口。</p>
            ) : null}
            {summaryInvalidated ? (
              <p>上次摘要的来源已变更或当前无法核验，本次按历史原文重新估算。</p>
            ) : null}
            {summaryTooLarge ? <p>生成新摘要所需的输入也超出了当前模型的可用窗口。</p> : null}
            <p>
              本次请求预计输入 {record.budget.estimated_input_tokens.toLocaleString('zh-CN')} Token
              / 可用 {record.budget.request_input_budget_tokens.toLocaleString('zh-CN')} Token
            </p>
            <p>
              窗口 {record.budget.runtime_effective_window_tokens.toLocaleString('zh-CN')} ·
              输出预留 {record.budget.output_reserve_tokens.toLocaleString('zh-CN')} · 安全预留{' '}
              {record.budget.safety_reserve_tokens.toLocaleString('zh-CN')} Token
            </p>
            <p>
              不可压缩内容 {record.plan.overflow.non_compressible_tokens.toLocaleString('zh-CN')}{' '}
              Token · 可由你减少{' '}
              {record.plan.overflow.user_reducible_tokens.toLocaleString('zh-CN')} Token
            </p>
            <ul>
              {suggestions?.map((action) => (
                <li key={action}>{actions[action]}</li>
              ))}
              <li>在模型设置中核对上下文窗口，或切换到上下文窗口更大的模型。</li>
              {summaryTooLarge ? <li>也可以新建会话，带入继续任务所需的背景。</li> : null}
            </ul>
          </>
        ) : readState === 'loading' ? (
          <p role="status">正在读取诊断…</p>
        ) : readState === 'ready' && record?.state === 'rechecking' ? (
          <p role="status">正在重新检查上下文。</p>
        ) : readState === 'ready' ? (
          <p role="status">当前没有待处理的上下文问题。</p>
        ) : null}
        {unknown ? (
          <p role="alert">
            上次重新检查的结果没有确认，没有自动重发；请重新打开这个会话，看任务是否还在等你处理，再点“已核对状态”。
          </p>
        ) : null}
      </div>
      {feedback}
      <div className="agent-actions">
        <button className="button is-small" type="button" onClick={onOpenModelSettings}>
          模型设置
        </button>
        <button
          className="button is-small is-primary"
          aria-busy={busy}
          type="button"
          disabled={busy || refreshing || unknown || record?.state !== 'awaiting'}
          onClick={() => void recheck()}
        >
          {busy ? '检查中…' : '重新检查'}
        </button>
        <button
          className="button is-small"
          aria-busy={refreshing}
          type="button"
          disabled={busy || refreshing || readState === 'loading'}
          onClick={refreshStatus}
        >
          {refreshing ? '刷新中…' : '刷新状态'}
        </button>
        {unknown ? (
          <button
            className="button is-small"
            type="button"
            disabled={busy || refreshing}
            onClick={() => {
              uncertain.delete(runId);
              setUnknown(false);
            }}
          >
            已核对状态
          </button>
        ) : null}
        <button className="button is-small is-danger" type="button" onClick={onStop}>
          停止任务
        </button>
      </div>
    </section>
  );
}
