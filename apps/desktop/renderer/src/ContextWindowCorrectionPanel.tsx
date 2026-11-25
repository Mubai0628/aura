import type {
  AgentContextWindowAction,
  AgentContextWindowCorrection,
} from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';

import { isUnconfirmedCommandOutcome } from '../../shared/ipc-request-failure';
import { failedAction } from './agent-session/failure-text';

/** What a correction that did not change the window says; the task stays paused. */
const WINDOW_UNCHANGED = '上下文窗口没有改';
const WINDOW_NEXT = '任务仍暂停着；请核对数值后再试。';

const uncertain = new Set<string>();
const drafts = new Map<string, string>();

export function ContextWindowCorrectionPanel({
  sessionId,
  record,
  onChanged,
  refreshStatus,
  refreshing,
  readFeedback,
  onOpenModelSettings,
  onStop,
}: {
  sessionId: string;
  record: AgentContextWindowCorrection;
  onChanged: () => void;
  refreshStatus: () => void;
  refreshing: boolean;
  readFeedback: ReactNode;
  onOpenModelSettings: () => void;
  onStop: () => void;
}) {
  const correction = record.correction;
  const resolution = correction.resolution;
  const input =
    resolution.type === 'user_correction_required' ||
    resolution.type === 'automatic_candidates_exhausted'
      ? resolution.input_contract
      : undefined;
  const initial =
    resolution.type === 'user_correction_required' ||
    resolution.type === 'automatic_candidates_exhausted'
      ? String(resolution.current_effective_window_tokens)
      : '';
  const key = `${sessionId}:${correction.agent_run_id}:${correction.model_step_id}`;
  const [tokens, setTokens] = useState(() => drafts.get(key) ?? initial);
  const [busy, setBusy] = useState(false);
  const [unknown, setUnknown] = useState(() => uncertain.has(key));
  const [error, setError] = useState<string>();
  const alive = useRef(true);
  const sending = useRef(false);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const numeric = Number(tokens);
  const valid =
    input !== undefined &&
    /^[1-9]\d*$/u.test(tokens) &&
    Number.isSafeInteger(numeric) &&
    numeric >= input.minimum_tokens &&
    (input.maximum_tokens == null || numeric <= input.maximum_tokens);
  const run = async (action: AgentContextWindowAction, save = false) => {
    if (sending.current || refreshing || unknown || record.state !== 'awaiting' || (save && !valid))
      return;
    sending.current = true;
    setBusy(true);
    setError(undefined);
    let outcomePending = false;
    const fail = (message: string, uncertainOutcome: boolean) => {
      if (uncertainOutcome) uncertain.add(key);
      if (alive.current) {
        setError(message);
        if (uncertainOutcome) setUnknown(true);
      }
    };
    try {
      if (save) {
        const settings = await window.aura.v1.getModelSettings();
        if (!alive.current) return;
        if (!settings.ok) {
          fail(failedAction(settings.error, WINDOW_UNCHANGED, WINDOW_NEXT), false);
          return;
        }
        outcomePending = true;
        const saved = await window.aura.v1.correctModelContextWindow({
          expected_revision: settings.value.models_revision,
          model_profile_id: correction.model_profile_id,
          window_tokens: numeric,
        });
        if (!saved.ok) {
          fail(
            failedAction(saved.error, WINDOW_UNCHANGED, WINDOW_NEXT),
            isUnconfirmedCommandOutcome(saved.error),
          );
          return;
        }
        outcomePending = false;
        if (!alive.current) return;
      }
      outcomePending = true;
      const result = await window.aura.v1.resolveAgentContextWindow({
        session_id: sessionId,
        agent_run_id: correction.agent_run_id,
        source_model_step_id: correction.model_step_id,
        expected_revision: record.revision,
        action,
      });
      if (!result.ok) {
        fail(
          failedAction(
            result.error,
            save ? '上下文窗口已经保存，但任务没有重新检查' : '这次处理没有生效',
            '任务仍暂停着；可以再点一次。',
          ),
          isUnconfirmedCommandOutcome(result.error),
        );
        return;
      }
      if (
        result.value.session_id !== sessionId ||
        result.value.agent_run_id !== correction.agent_run_id
      ) {
        fail('返回的是另一个任务的结果，没有采用；请重新打开这个会话核对。', true);
        return;
      }
      outcomePending = false;
      drafts.delete(key);
      uncertain.delete(key);
      if (alive.current) setUnknown(false);
    } catch {
      fail(
        '这次处理的结果没有确认（与 Aura Core 的连接出了问题），没有自动重发；请重新打开这个会话，并到模型设置里核对上下文窗口。',
        outcomePending,
      );
    } finally {
      sending.current = false;
      if (alive.current) {
        setBusy(false);
        onChanged();
      }
    }
  };
  return (
    <section className="agent-context-remediation" aria-label="上下文窗口修正">
      <strong>模型服务报告上下文超限</strong>
      <div
        className="context-remediation-details"
        role="region"
        aria-label="窗口修正详情"
        tabIndex={0}
      >
        <p>当前任务等待处理。输入完整 Token 数修正窗口，或取消手工修正并尝试较小窗口。</p>
        {resolution.type === 'automatic_candidates_exhausted' ? (
          <p>较小窗口候选已用尽，请再次修正窗口或调整模型配置。</p>
        ) : null}
        {correction.provider_error ? (
          <div className="provider-error-summary">
            <strong>模型服务错误（已脱敏）</strong>
            <p>
              {correction.provider_error.provider_message ?? correction.provider_error.issue_code}
            </p>
          </div>
        ) : null}
        <label>
          完整上下文窗口（Token）
          <input
            inputMode="numeric"
            type="text"
            value={tokens}
            disabled={busy || unknown}
            onChange={(event) => {
              setTokens(event.target.value);
              drafts.set(key, event.target.value);
            }}
          />
        </label>
        {error ? <p role="alert">{error}</p> : null}
        {unknown ? (
          <p role="alert">
            上次处理的结果没有确认，没有自动重发；请先到模型设置里核对上下文窗口，再看任务是否还在等你处理。
          </p>
        ) : null}
      </div>
      {readFeedback}
      <div className="agent-actions">
        <button
          className="button is-small is-primary"
          type="button"
          disabled={busy || refreshing || unknown || !valid}
          onClick={() => void run('recheck', true)}
        >
          保存并重新检查
        </button>
        <button
          className="button is-small"
          type="button"
          disabled={
            busy || refreshing || unknown || resolution.type === 'automatic_candidates_exhausted'
          }
          onClick={() => void run('try_smaller_windows')}
        >
          取消手工修正，尝试较小窗口
        </button>
        <button className="button is-small" type="button" onClick={onOpenModelSettings}>
          模型设置
        </button>
        <button
          className="button is-small"
          type="button"
          disabled={busy || refreshing || unknown}
          onClick={() => void run('recheck')}
        >
          按当前配置重新检查
        </button>
        <button
          className="button is-small"
          type="button"
          disabled={busy || refreshing}
          aria-busy={refreshing}
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
              uncertain.delete(key);
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
