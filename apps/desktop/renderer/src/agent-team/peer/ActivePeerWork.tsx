import type { AgentInboxProjection, PeerBudget } from '@aura/contracts/generated';
import { useId, useState } from 'react';
import { AppIcon } from '../../app-icons';
import { ConfirmationDialog } from '../../ConfirmationDialog';
import { PeerBudgetForm } from './PeerBudgetForm';
import { PeerReviewForm } from './PeerReviewForm';
import { usePeerInbox } from './usePeerInbox';
import './active-peer-work.css';

export function ActivePeerWork({
  sessionId,
  names,
  inputQueue,
  onChanged,
}: {
  sessionId: string;
  names: ReadonlyMap<string, string>;
  inputQueue?: AgentInboxProjection | undefined;
  onChanged: () => void;
}) {
  const inbox = usePeerInbox(sessionId, true, 'active');
  const [stop, setStop] = useState<{
    deliveryId: string;
    label: '拒绝投递' | '取消投递' | '停止本次全部协作';
    revision?: number;
  }>();
  const [budgetId, setBudgetId] = useState<string>();
  const [resuming, setResuming] = useState(false);
  const [resumeError, setResumeError] = useState(false);
  const titleId = useId();
  const entries =
    inbox.value?.entries.filter(
      ({ delivery }) => delivery.state === 'queued' || delivery.state === 'started',
    ) ?? [];
  const activeBudget = inbox.value?.active_budget;
  if (!inbox.supported || (!entries.length && !activeBudget && !inbox.error && !stop)) return null;
  const unseenPause =
    inputQueue?.paused &&
    !inputQueue.pause_reason &&
    !inputQueue.messages.length &&
    entries.some(
      ({ delivery }) => delivery.target_session_id === sessionId && delivery.state === 'queued',
    );
  const resume = async () => {
    if (resuming || !inputQueue) return;
    setResuming(true);
    setResumeError(false);
    try {
      const outcome = await window.aura.v1.mutateAgentInbox({
        session_id: inputQueue.session_id,
        expected_revision: inputQueue.revision,
        action: { action: 'resume' },
      });
      if (!outcome.ok) setResumeError(true);
    } catch {
      setResumeError(true);
    } finally {
      setResuming(false);
      onChanged();
      void inbox.refresh();
    }
  };
  const managedRuns = new Set<string>();
  const runControls = (deliveryId: string, budget: PeerBudget) => ({
    quota: (
      <span className="active-peer-quota">
        额度 {budget.used} / {budget.limit}
      </span>
    ),
    buttons: (
      <>
        <button
          className="button is-small"
          type="button"
          disabled={inbox.busy || budget.limit >= 1024}
          onClick={() => setBudgetId(deliveryId)}
        >
          增加本次额度
        </button>
        <button
          className="button is-small"
          type="button"
          disabled={inbox.busy}
          onClick={() =>
            setStop({ deliveryId, label: '停止本次全部协作', revision: budget.revision })
          }
        >
          停止本次全部协作
        </button>
      </>
    ),
    editor:
      budgetId === deliveryId ? (
        <PeerBudgetForm
          key={budget.revision}
          min={budget.limit + 1}
          initial={budget.limit + 1}
          label="本次新上限"
          busy={inbox.busy}
          onSave={(limit) => {
            void inbox
              .change({
                kind: 'increase',
                delivery_id: deliveryId,
                expected_revision: budget.revision,
                limit,
              })
              .then((ok) => {
                if (ok) setBudgetId(undefined);
              });
          }}
        />
      ) : null,
  });
  const currentRun = activeBudget
    ? runControls(activeBudget.delivery_id, activeBudget.budget)
    : null;
  return (
    <section className="active-peer-work" aria-label="当前同级协作">
      {unseenPause ? (
        <button
          className="button is-small"
          type="button"
          disabled={resuming}
          onClick={() => void resume()}
        >
          继续队列
        </button>
      ) : null}
      {resumeError ? <p role="alert">恢复结果待核对，请查看当前队列。</p> : null}
      {inbox.error ? (
        <div role="alert">
          {inbox.error}
          <button className="button is-small" type="button" onClick={() => void inbox.refresh()}>
            重新读取
          </button>
        </div>
      ) : null}
      {currentRun ? (
        <article className="active-peer-entry">
          <div className="active-peer-actions">
            {currentRun.quota}
            {currentRun.buttons}
          </div>
          {currentRun.editor}
        </article>
      ) : null}
      {entries.map((entry) => {
        const { delivery, budget } = entry;
        const outgoing = delivery.source_session_id === sessionId;
        const peer = outgoing ? delivery.target_session_id : delivery.source_session_id;
        const manages =
          entry.manages_run && !budget.stopped && !managedRuns.has(budget.root_event_id);
        if (manages) managedRuns.add(budget.root_event_id);
        const controls = manages ? runControls(delivery.delivery_id, budget) : null;
        const cancelLabel = entry.review_text != null ? '拒绝投递' : '取消投递';
        const label = `${outgoing ? '发往' : '来自'} ${names.get(peer) ?? '关联会话'}`;
        return (
          <article
            key={delivery.delivery_id}
            className={`active-peer-entry${outgoing ? ' is-outgoing' : ''}`}
          >
            <header>
              <AppIcon name="agents" />
              <strong title={outgoing ? label : undefined}>{label}</strong>
              <span className="active-peer-status">
                {entry.review_text != null
                  ? '待确认'
                  : delivery.state === 'started'
                    ? '执行中'
                    : '排队中'}
              </span>
              {outgoing ? controls?.quota : null}
            </header>
            {entry.review_text != null ? (
              <PeerReviewForm
                original={entry.review_text}
                busy={inbox.busy}
                onRelease={(revised_text) =>
                  void inbox.change({
                    kind: 'release',
                    delivery_id: delivery.delivery_id,
                    revised_text,
                  })
                }
              />
            ) : delivery.reason ? (
              <p role="status">{delivery.reason}</p>
            ) : null}
            <div className="active-peer-actions">
              <button
                className="button is-small"
                type="button"
                disabled={inbox.busy}
                onClick={() => setStop({ deliveryId: delivery.delivery_id, label: cancelLabel })}
              >
                {cancelLabel}
              </button>
              {outgoing ? controls?.buttons : null}
            </div>
            {!outgoing && controls ? (
              <div className="active-peer-actions">
                {controls.quota}
                {controls.buttons}
              </div>
            ) : null}
            {controls?.editor}
          </article>
        );
      })}
      {inbox.value?.before ? (
        <button className="button is-small" type="button" onClick={() => void inbox.older()}>
          更多待处理
        </button>
      ) : null}
      {stop ? (
        <ConfirmationDialog
          title={`${stop.label}？`}
          titleId={titleId}
          busy={inbox.busy}
          description={
            stop.revision != null ? '停止当前协作任务的在途投递。' : '操作范围为当前投递。'
          }
          confirmLabel={stop.revision != null ? '确认停止' : stop.label}
          onCancel={() => setStop(undefined)}
          onConfirm={() => {
            setStop(undefined);
            void inbox.change(
              stop.revision != null
                ? {
                    kind: 'stop_run',
                    delivery_id: stop.deliveryId,
                    expected_revision: stop.revision,
                  }
                : { kind: 'cancel', delivery_id: stop.deliveryId },
            );
          }}
        />
      ) : null}
    </section>
  );
}
