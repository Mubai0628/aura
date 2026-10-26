import type { AgentAttachmentDraft, AgentTaskMode, GoalBudget } from '@aura/contracts/generated';
import { useRef, useState } from 'react';
import { isUnconfirmedCommandOutcome } from '../../../shared/ipc-request-failure';
import { copySessionReasoning, sessionReasoning } from '../reasoning-effort';
import { getComposerDraft, updateComposerDraft } from './composer-drafts';
import { acceptSending, endSending, startSending } from './sending';
import { userMessage } from './user-message';

export interface SubmissionPreparation {
  prepare: (modelId: string) => Promise<string>;
  accepted: (sessionId: string, text: string) => void;
}
export function useComposerSubmission(
  sessionId: string,
  activeTurn: string | undefined,
  modelId: string | undefined,
  queuePending: boolean,
  onChanged: () => void,
  preparation?: SubmissionPreparation,
) {
  const [draft, updateDraft] = useState(() => getComposerDraft(sessionId)?.text ?? '');
  const [mode, setMode] = useState<AgentTaskMode>('normal');
  const [budgetTokens, setBudgetTokens] = useState('');
  const [budgetTurns, setBudgetTurns] = useState('');
  const [busy, setBusy] = useState(false);
  const sending = useRef(false);
  const [error, setError] = useState<string>();
  const [needsReview, updateNeedsReview] = useState(
    () => getComposerDraft(sessionId)?.needsReview ?? false,
  );
  const setNeedsReview = (value: boolean) => {
    updateComposerDraft(sessionId, {
      text: getComposerDraft(sessionId)?.text ?? draft,
      needsReview: value,
    });
    updateNeedsReview(value);
  };
  const setDraft = (text: string) => {
    if (!updateComposerDraft(sessionId, { text, needsReview }))
      setError('草稿暂时无法保存到本机，请保留内容后再离开。');
    updateDraft(text);
  };
  /**
   * Sends the draft; `skillIds` are the Skills chosen for the turn it starts, `attachments` the
   * pictures and files added to it. While a task runs the
   * message waits in the queue for the next turn. Resolves
   * true when the message was accepted, so the composer can clear its choices.
   */
  const send = async (
    skillIds: readonly string[] = [],
    attachments: readonly AgentAttachmentDraft[] = [],
  ): Promise<boolean> => {
    if (sending.current || needsReview || (!draft.trim() && !attachments.length)) return false;
    const skills = skillIds.length ? { skill_ids: [...skillIds] } : {};
    const attached = attachments.length ? { attachments: [...attachments] } : {};
    sending.current = true;
    setBusy(true);
    setError(undefined);
    let requestStarted = false;
    // A message that starts a turn shows at once where the conversation will show it, and the
    // composer empties; if it is not accepted the text comes back. The saved draft keeps it
    // until it is accepted.
    let shown: { text: string; files: string[] } | undefined;
    const showing = new Set<string>();
    const show = (key: string) => {
      if (!shown || showing.has(key)) return;
      startSending(key, shown);
      showing.add(key);
    };
    const unshow = () => {
      showing.forEach(endSending);
      if (!shown) return;
      const typed = getComposerDraft(sessionId)?.text;
      setDraft(typed && typed !== shown.text ? `${shown.text}\n${typed}` : shown.text);
    };
    try {
      const special = !activeTurn && !queuePending && mode !== 'normal';
      const budget: GoalBudget = {
        max_tokens: budgetTokens ? Number(budgetTokens) : null,
        max_turns: budgetTurns ? Number(budgetTurns) : null,
      };
      if (!activeTurn && !queuePending && !modelId) {
        setError('请先选择已配置的模型。');
        return false;
      }
      if (mode !== 'normal' && (Boolean(activeTurn) || queuePending)) {
        setError('切换 Plan 或 Goal 只能在空闲时开始。这条消息不会进入队列或追加。');
        return false;
      }
      if (
        special &&
        mode === 'goal' &&
        [budget.max_tokens, budget.max_turns].some(
          (n) => n != null && (!Number.isSafeInteger(n) || n <= 0),
        )
      ) {
        setError('Goal 预算需填写正整数，留空表示不设置该项。');
        return false;
      }
      if (!activeTurn && !queuePending) {
        shown = { text: draft, files: attachments.map(attachmentName) };
        show(sessionId);
        updateDraft('');
      }
      const targetSession = preparation && modelId ? await preparation.prepare(modelId) : sessionId;
      show(targetSession);
      // The conversation's own thinking level for its model goes with the message; a new
      // conversation takes the one its draft chose.
      copySessionReasoning(sessionId, targetSession);
      const level = modelId ? sessionReasoning(targetSession, modelId) : undefined;
      const reasoning = level ? { reasoning_effort: level } : {};
      requestStarted = true;
      const result =
        special && modelId
          ? await window.aura.v1.startAgent({
              session_id: targetSession,
              model_profile_id: modelId,
              text: draft,
              mode,
              goal_budget: mode === 'goal' ? budget : null,
              ...skills,
              ...reasoning,
              ...attached,
            })
          : await window.aura.v1.submitAgentMessage({
              session_id: targetSession,
              text: draft,
              destination: { lane: 'next_turn' },
              ...skills,
              ...reasoning,
              ...attached,
            });
      if (!result.ok) {
        unshow();
        setError(userMessage(result.error.message, '消息未发送，请检查模型与连接后重试。'));
        if (isUnconfirmedCommandOutcome(result.error)) setNeedsReview(true);
        onChanged();
        return false;
      }
      const firstName =
        attachments[0]?.kind === 'file' ? attachments[0].path : attachments[0]?.name;
      preparation?.accepted(targetSession, draft.trim() ? draft : (firstName ?? draft));
      setDraft('');
      setMode('normal');
      showing.forEach((key) => (key === targetSession ? acceptSending(key) : endSending(key)));
      onChanged();
      return true;
    } catch (reason) {
      unshow();
      setError(userMessage(reason, '发送结果未确认。草稿已保留，请核对消息历史与队列。'));
      if (requestStarted || !preparation || !getComposerDraft(sessionId)?.serverSessionId)
        setNeedsReview(true);
      onChanged();
      return false;
    } finally {
      sending.current = false;
      setBusy(false);
    }
  };
  /** `/init`: start the initialization task; the draft is the user's optional note. */
  const sendInit = async (): Promise<boolean> => {
    if (sending.current || needsReview) return false;
    if (activeTurn || queuePending) {
      setError('当前任务结束、排队消息处理完之后才能初始化。');
      return false;
    }
    if (!modelId) {
      setError('请先选择已配置的模型。');
      return false;
    }
    sending.current = true;
    setBusy(true);
    setError(undefined);
    let requestStarted = false;
    try {
      const targetSession = preparation ? await preparation.prepare(modelId) : sessionId;
      requestStarted = true;
      const result = await window.aura.v1.startWorkspaceInit({
        session_id: targetSession,
        model_profile_id: modelId,
        text: draft,
      });
      if (!result.ok) {
        setError(userMessage(result.error.message, '初始化没有开始，请检查模型与连接后重试。'));
        if (isUnconfirmedCommandOutcome(result.error)) setNeedsReview(true);
        onChanged();
        return false;
      }
      preparation?.accepted(targetSession, '初始化工作区');
      setDraft('');
      onChanged();
      return true;
    } catch (reason) {
      setError(userMessage(reason, '初始化是否开始尚未确认。草稿已保留，请核对消息历史。'));
      if (requestStarted || !preparation || !getComposerDraft(sessionId)?.serverSessionId)
        setNeedsReview(true);
      onChanged();
      return false;
    } finally {
      sending.current = false;
      setBusy(false);
    }
  };
  /**
   * `/compact` while nothing runs: a one-off task that compacts the conversation first; the
   * draft is what the summary should keep.
   */
  const sendCompaction = async (): Promise<boolean> => {
    if (sending.current || needsReview) return false;
    if (activeTurn || queuePending) {
      setError('当前任务结束、排队消息处理完之后才能开始压缩。');
      return false;
    }
    if (!modelId) {
      setError('请先选择已配置的模型。');
      return false;
    }
    sending.current = true;
    setBusy(true);
    setError(undefined);
    try {
      const result = await window.aura.v1.startAgentCompaction({
        session_id: sessionId,
        model_profile_id: modelId,
        focus: draft.trim() || null,
      });
      if (!result.ok) {
        setError(userMessage(result.error.message, '压缩没有开始，请检查模型与连接后重试。'));
        if (isUnconfirmedCommandOutcome(result.error)) setNeedsReview(true);
        onChanged();
        return false;
      }
      setDraft('');
      onChanged();
      return true;
    } catch (reason) {
      setError(userMessage(reason, '压缩是否开始尚未确认。草稿已保留，请核对消息历史。'));
      setNeedsReview(true);
      onChanged();
      return false;
    } finally {
      sending.current = false;
      setBusy(false);
    }
  };
  return {
    draft,
    setDraft,
    mode,
    setMode,
    budgetTokens,
    setBudgetTokens,
    budgetTurns,
    setBudgetTurns,
    busy,
    error,
    needsReview,
    acknowledge: () => {
      setNeedsReview(false);
      setError(undefined);
    },
    send,
    sendInit,
    sendCompaction,
  };
}

/** The name a picture or file is shown by while it is sent. */
function attachmentName(attachment: AgentAttachmentDraft): string {
  return attachment.kind === 'file'
    ? (attachment.path.split(/[\\/]/u).at(-1) ?? attachment.path)
    : attachment.name;
}
